# fissa-wt

A TUI for git worktrees — create one from a short slug, run `npm ci` for it,
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

## npm

This is an add-on, not the core — `fissa` works on any git repo in any language, and the
whole `deps` form row disappears on a repo with no `package.json`.

New worktrees can run `npm ci` for you, so the worktree is ready to build the moment you
land in it. Every directory holding a `package.json` next to a `package-lock.json` (or
`npm-shrinkwrap.json`) is installed, at any depth. A marker `package.json` at the root of
a monorepo has no lockfile and is therefore left alone rather than failing the creation,
since `npm ci` refuses to run without one.

| Strategy | Cost | Result |
|---|---|---|
| `npm ci` (default) | minutes | a worktree you can build immediately |
| `skip` | — | no `node_modules`; install it yourself |

The `deps` row only appears when at least one package has a lockfile.

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

See `TESTING.md` for the testing strategy and `docs/superpowers/specs/` for the design.
