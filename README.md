# fissa-wt

A TUI for git worktrees — create one from a short slug, reuse its npm `node_modules`,
and `cd` straight into it.

`fissa` is French slang for "quickly", which is the whole point: creating a worktree
should take seconds, not a coffee break.

## Install

Requires a Rust toolchain and `git` (≥ 2.31). Linux only.

```sh
cargo install fissa-wt
```

Then enable `cd` on exit by adding this to `~/.zshrc` or `~/.bashrc`:

```sh
eval "$(fissa --shell-init)"
```

Restart your shell. Without this step everything works except landing in the
selected worktree; the list footer will remind you.

## Use

Run `fissa` anywhere inside a git repository. Arrow keys and vim keys work
interchangeably on every screen.

| Screen | Keys |
|---|---|
| list | `↑`/`↓` or `j`/`k` move, `/` search, `n` new worktree, `enter` cd, `q` quit |
| create | `tab` or `↑`/`↓` next field, `←`/`→` cycle value, `enter` create, `esc` cancel |
| progress | when finished: `enter` cd into the new worktree, `esc` back to the list |

`/` opens a search bar and the list narrows as you type, matching either the directory or
the branch, case-insensitively. `enter` keeps the filter and puts the cursor on the first
match, so `↑`/`↓` then walks only those rows; `esc` backs out of the bar without applying
anything. A filter stays visible in the title until `esc` clears it — `q` quits regardless.

Creating `spe-11667` with prefix `feature/` in a repo cloned at `~/work/spectra` gives
branch `feature/spe-11667` in `~/work/spectra-spe-11667`. Type a `/` in the slug and it
becomes the branch name verbatim.

The form previews the branch and directory live, and refuses to create anything if the
directory exists, the branch is taken, or the base ref is unknown.

## node_modules

This is an add-on, not the core — `fissa` works on any git repo in any language, and the
whole modules column and form row disappear on a repo with no `package.json`.

New worktrees can reuse the modules of the worktree you launched from, instead of
running a full install. Every directory holding a `package.json` is handled, at any
depth — except under `install`, which covers only the directories that also hold a
`package-lock.json` (or `npm-shrinkwrap.json`), since `npm ci` refuses to run without
one. A marker `package.json` at the root of a monorepo is therefore left alone rather
than failing the creation.

| Strategy | Cost | Isolation |
|---|---|---|
| `hardlink` (default) | ~1s, ~5% of the tree | full — npm cannot affect other worktrees |
| `symlink` | instant, 0 bytes | none — one shared tree, `npm install` prunes it for everyone |
| `install` | `npm ci`, minutes | full — needs a lockfile |
| `none` | — | — |

Measured on a 1073 MB, 1118-package tree: **1 second** and **52 MB**. File contents are
shared via hardlinks; the 5% is the directory entries, which cannot be hardlinked. Note
that `du` on a single worktree still reports the full ~1 GB, because it counts each shared
inode once per traversal — which is why there is no size column in the list.

Removing a worktree created this way never touches the source; `git worktree remove`
deletes hardlinks, not the shared file contents.

A package that exists in the source but not in the new worktree — typically a vendored
`package.json` inside a gitignored build directory — is reported as
`skipped (not in this worktree)` rather than failing the creation.

npm only. Yarn, pnpm and bun are not supported.

## Development

```sh
make test
make lint
make install
```

See `TESTING.md` for the testing strategy and `design-docs/` for the design.
