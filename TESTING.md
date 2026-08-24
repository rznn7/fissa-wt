# Testing strategy

`fissa` is a solo TUI whose side effects are git shell-outs, filesystem operations, and
terminal rendering. Tests exist to drive design and catch logic and interaction bugs in
a tight loop — not to freeze pixels. We test hard where feedback is unambiguous and
lightly where it is not.

## Layers

| Layer | What we assert | How | TDD? |
|---|---|---|---|
| **Pure logic** | name derivation, porcelain parsing, prefix options, row labels, step planning, skip decisions | call fn, `assert_eq!` | Yes — primary |
| **Filesystem logic** | target discovery, strategy availability | build a tree with `tempfile`, assert | Yes — primary |
| **Component state** | `KeyEvent` → state change + `KeyEventResponse`; focus, cycling, preview | construct component, feed events, assert | Yes — primary |
| **Render smoke** | no panic; key substrings present; column alignment | `TestBackend`, scan the buffer | a few per component |
| **Shell function** | exit status, resulting `pwd`, passed-through stdout | source `SHELL_FUNCTION` into real `bash`, run against a stub binary on `PATH` | Yes |
| **Join point** | `build_rows` over a real temp git repo | `git init` in a `tempfile` dir | two tests |
| **Concurrency** | chains overlap; a chain stops at its first failure | inject a runner into `run_chains`, count peak concurrency | Yes |
| **Remotes** | default remote choice, upstream resolution, remote branch deletion | push to a bare repo in a `tempfile` dir | Yes |
| **`npm ci`, real 1 GB repos, real terminals** | — | not tested | No |

No golden snapshots, no `insta`, no `mockall`. Std `assert_eq!` plus ratatui's
`TestBackend` only.

## Conventions

- Inline tests: `#[cfg(test)] mod tests { use super::*; … }` at the bottom of the file
  under test.
- Runner: `cargo test`.
- Names: `test_<unit>_<condition>_<expected>`.
- One behaviour per test. Arrange / act / assert, no shared mutable fixtures.
- Shared helpers (`buffer_to_string`, `key`) live in `src/components/mod.rs` behind
  `#[cfg(test)]`.

## Two lessons worth keeping

**String assertions cannot test shell behaviour.** The emitted shell function was once
asserted only with `SHELL_FUNCTION.contains(...)`, which passed while two real bugs sat
in it: an ordinary quit returned exit 1, and `--help`/`--version` were swallowed and fed
to `cd`. Both are properties of how a shell *evaluates* the line, invisible to any
substring check. The function is now exercised by running it.

**Assert on character columns, not byte offsets.** A render test compared `str::find`
results to check column alignment and failed on correctly aligned output, because `…`
and `│` are three bytes each. If a render assertion involves a position, convert to
`chars().count()` first.

## What we deliberately don't test

- `npm ci` — slow, network-dependent, and the wrapper is three lines.
- Exact layout and styling — smoke tests check presence and alignment, not pixels.
- zsh — the shell function's tests run under `bash`; zsh is verified by hand.

## Manual verification checklist

Run before a release:

1. `eval "$(fissa init bash)"` in bash and `eval "$(fissa init zsh)"` in zsh, then
   `enter` actually cds.
2. `fissa --help`, `fissa --version` and `fissa init bash` all still print with the
   wrapper active.
3. Creating a worktree with `npm ci` leaves a `node_modules` in each locked package and
   the result builds.
4. `q` and a panic both leave the terminal usable (`stty -a` shows `icanon`).
5. The footer shows the shell-init hint when `FISSA_SHELL_INIT` is unset.
6. On a repo with no `package.json`, the form's `deps` row is absent.
7. Deleting the worktree you launched `fissa` from succeeds and lands back on the list.
8. `r` on a pushed branch really removes it from the remote; the row shows no `r` hint
   for a branch that was never pushed.

`cargo build` does not update an installed binary; re-run `make install` before any
manual check, or you will be testing the previous version.
