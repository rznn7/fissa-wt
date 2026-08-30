# Copying declared local files into a new worktree

## Problem

A new worktree contains only tracked files. Most of what is missing is
regenerable — `node_modules/`, `target/`, `dist/` — and the install step already
covers it. A small residue is not regenerable: `.env`, `.env.local`,
`config/secrets.local.yml`. These are gitignored precisely because they are
secret or machine-specific, nothing recreates them, and their absence is the
most common reason a fresh worktree does not run.

Discovering them heuristically was considered and rejected. Enumerating ignored
files finds real hazards alongside the useful ones — a multi-gigabyte VM image
sitting at the repo root is an ignored regular file like any other — and the
error cost is wildly asymmetric: a false positive fills the disk or stalls the
create, while a false negative costs the user one manual copy. No heuristic
earns that trade.

## Approach

The repository declares the files. fissa copies exactly what is declared and
never guesses.

```toml
# .fissa.toml at the repository root, committed
[worktree]
copy = [".env", "config/secrets.local.yml"]
```

The file is committed rather than ignored, for three reasons: whoever set the
project up writes it once for everyone; a committed file arrives in each new
worktree as an ordinary tracked file, so it never has to copy itself; and a repo
without one behaves exactly as fissa does today. This is the `.editorconfig`
pattern — a declaration honoured when present and inert when absent.

Because the manifest is committed, it is attacker-controlled input in any cloned
repository. Entry validation is a security boundary, not a convenience.

## Manifest

New module `src/manifest.rs`. `src/config.rs` is untouched: it owns user
preferences under XDG, this owns repository facts, and the two have different
lookups and lifetimes.

Loaded from `Repo.source` — the worktree fissa was invoked from, matching
`Repo::has_submodules` — not `main_clone`, which may never have held a `.env`.

Parsed with the existing `toml` dependency. `#[serde(default)]` throughout, so a
`.fissa.toml` carrying only future tables yields an empty copy list. A missing
file is an empty list rather than an error, mirroring `config::load_from`.

### Entry validation

Every entry is a literal relative path to a single file. Each rule below rejects
at parse time with a message naming the offending entry:

- absolute paths
- any `.` or `..` component
- `*`, `?` or `[` — rejected explicitly rather than treated as literals, so that
  someone who expects globbing is told it is unsupported instead of watching a
  copy silently do nothing
- trailing slash or any directory form
- the empty string

Literal paths are a strict subset of globs, so supporting patterns later is
additive and breaks no existing manifest. Directories are excluded deliberately:
a directory listed while small can grow without anyone revisiting the manifest,
which is the one route by which an enormous file could still arrive.

Two rules depend on the filesystem and are therefore enforced at copy time:

- inspection uses `symlink_metadata`, never `metadata`. A symlink is skipped
  rather than followed; otherwise `.env -> /var/lib/libvirt/images/debian.qcow2`
  measures 40 bytes at every check and copies 40 GB.
- regular files only.

No size cap. A cap was load-bearing while a pattern could match a large file by
accident; against literal filenames it can only fire on a file the user named
themselves. The step detail reports bytes copied instead, so an unexpectedly
large copy is visible after the fact.

### Failure handling

A malformed `.fissa.toml` warns on stderr and exits before the TUI starts,
naming the file — the same treatment `config.rs` gives bad user configuration,
and consistent with `main` returning `anyhow::Result<()>` prior to raw mode.

Neither load may run on the `fissa init <shell>` path. That branch is evaluated
during every shell startup, and a broken manifest in one repository must not
break the user's shell. It returns before any load today; the manifest load goes
after it.

## Create step

```rust
Action::CopyLocal { rel: PathBuf }
```

One step per entry, labelled `copy  .env` in the style of the install labels.
Per-file steps make each outcome an ordinary step detail that the progress list
already renders, rather than an aggregate summary string, and entries are
hand-written literals so the list runs to a handful at most.

`plan_steps` pushes these after `InitSubmodules` and before every `Install`.
That position is load-bearing twice: `run_steps` runs everything preceding the
first `Install` sequentially, which is where copying belongs, and a `postinstall`
script may read `.env`, so copying must precede the installs. It follows that
copying is not parallelised — running it alongside the installs would reintroduce
the ordering hazard the placement exists to prevent, to save microseconds on
small text files in a pipeline whose real cost is process spawn.

`run_steps` needs no changes.

### Per-file outcomes

| Condition | Outcome |
| --- | --- |
| source missing | skip, `skipped (not in the source)` |
| destination exists | skip; covers a declared path that is tracked, and never clobbers |
| symlink or non-regular file | skip |
| read or write error | fail the step |

Skipping missing sources matters because `report` halts the sequential prefix on
`Err`. A teammate who never created `.env.local` must not lose their installs
over it. A genuine IO error — permission denied, a full disk — is worth halting
for.

The skip decision needs the source root, which `skip_reason` does not receive.
Copy skips are therefore decided inside the action, returning `Ok` with the
detail string that `run_step` already propagates. `skip_reason` is unchanged.

Destination parents get `create_dir_all`, since `config/` may not exist in a
fresh worktree when it holds nothing tracked. `std::fs::copy` preserves Unix
permission bits, so a 0600 `.env` stays 0600.

## Surfacing

No create-form field and no toggle. Writing the manifest is the opt-in, so there
is nothing further to confirm; the copies appear as steps in the progress list,
which keeps them visible. `create_form.rs` is untouched.

## Out of scope

- A copy list in `~/.config/fissa/config.toml` applying to every repository. It
  raises its own questions — whether it unions or overrides, and how it should
  behave in an untrusted repo — and can be added later without altering the
  manifest format.
- Glob patterns, directories, symlink recreation, `post_create` hooks, and any
  `.fissa.toml` key beyond `[worktree] copy`.

## Testing

Unit tests in-module with `tempfile`, following the existing pattern.

`manifest.rs`:
- a missing file and an empty file both parse to an empty list
- `[worktree] copy` parses
- unknown tables are ignored
- each invalid entry form is rejected, and the message names the entry

`create.rs`:
- `plan_steps` emits one `CopyLocal` per entry
- every `CopyLocal` follows `InitSubmodules` and precedes every `Install`,
  mirroring `test_plan_steps_puts_the_submodule_init_before_every_install`

Copy behaviour against a temp directory:
- copies a regular file
- preserves 0600
- creates a missing parent directory
- skips a missing source
- skips an existing destination without modifying it
- skips a symlink without following it

## Documentation

- a Features bullet in `README.md`
- a `.fissa.toml` section in the README Configuration block
- the same in `HELP` in `src/main.rs`, which documents only the user config today
