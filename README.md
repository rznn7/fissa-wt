# fissa-wt

A TUI for git worktrees. Create one from a short slug, install its deps, and `cd`
straight into it.

## Features

- **Create** from a slug: `login-fix` with prefix `feature/` gives branch
  `feature/login-fix` in `~/work/myrepo-login-fix`, previewed live.
- **Prefixes and base refs** read from the repo's default remote.
- **Search** with `/`, narrowing the list by directory or branch as you type.
- **Delete in bulk**: extend the selection, confirm once, deletions run in parallel.
- **Dirty worktrees** are marked `●` and refused unless you force with `f`.
- **Branch cleanup**: optionally drop the local branch, the remote one, or both.
- **npm, pnpm, yarn, bun and deno**: new worktrees install with whichever manager
  each lockfile names, at any depth, so they build on arrival.

## Install

Requires a Rust toolchain and `git`. Linux only.

```sh
cargo install --git https://github.com/rznn7/fissa-wt
```

Or from source:

```sh
git clone https://github.com/rznn7/fissa-wt
cd fissa-wt
make install
```

Then enable `cd` on exit by adding the matching line to `~/.zshrc` or `~/.bashrc`, and
restart your shell:

```sh
eval "$(fissa init zsh)"
eval "$(fissa init bash)"
```

## Configuration

Optional, at `~/.config/fissa/config.toml` (or `$XDG_CONFIG_HOME/fissa/config.toml`):

```toml
default_mode = "list"   # list | create
```

`create` opens straight on the creation form; `Esc` still returns to the list.

## Development

```sh
make test
make lint
```
