# Simplifying node dependency handling

## Problem

`fissa` offers four dependency strategies at worktree creation: `hardlink`, `symlink`,
`install`, `none`. Two problems with that set:

- **Too many knobs.** Four options is more than a creation form should ask you to reason
  about, and choosing between them requires knowing what a hardlink is.
- **Silent wrongness.** `hardlink` and `symlink` reuse the source worktree's
  `node_modules` unconditionally. If the new branch's `package-lock.json` differs from
  the source's, the worktree looks ready and is running the wrong dependencies.

The sharing strategies were an optimization — measured at 1s and 52 MB against `npm ci`'s
minutes on a 1073 MB, 1118-package tree. The optimization is real, but it is not worth the
comprehension cost or the correctness risk at this stage.

## Decision

Reduce the strategy set to `install` and `none`, defaulting to `install`, and rename the
UI to describe the command that runs rather than the mechanism or the directory.

`npm ci` is correct by construction: it deletes `node_modules` and installs from the
lockfile, so there is nothing to be stale. This removes the need for any freshness check.

## Scope

This covers node only. Rust needs no work: `fissa` already runs on any git repository, and
on a repo with no `package.json` the deps row and modules column already disappear. No
Rust prepare step is wanted yet — revisit once usage shows what is missing.

## Changes

### `src/node.rs`

`Strategy` becomes two variants:

```rust
pub enum Strategy {
    Install,
    None,
}

impl Strategy {
    pub const ALL: [Strategy; 2] = [Strategy::Install, Strategy::None];
}
```

`label()` returns the command, not the mechanism: `Install => "npm ci"`,
`None => "skip"`.

`unavailable_reason` keeps one rule — `Install` requires a `package-lock.json` or
`npm-shrinkwrap.json` next to the `package.json`, reported as
`"no package-lock.json next to a package.json"`. `None` is always available.

`Target` loses `has_source_modules`; only the sharing strategies read it.
`discover_targets` still refuses to descend into `node_modules`, but stops recording
whether it is a directory.

Removed entirely: `hardlink_modules`, `symlink_modules`, `same_filesystem`,
`package_count`, `NmState`, `nm_state`.

### `src/create.rs`

`Action` becomes `AddWorktree` and `Install`. `plan_steps` labels each step by command
and location instead of by path: `npm ci` at the repository root, `npm ci  app` for a
nested package. `run_step` keeps the `Install` arm unchanged, returning `"installed"`.

`skip_reason` is unchanged and still reports `"skipped (not in this worktree)"` for a
package present in the source but absent from the new worktree.

### `src/components/create_form.rs`

The row label becomes `deps` and cycles over the command names from `Strategy::label()`:

```
  slug     spe-11667
  prefix   ‹ feature/ ›
  base     develop
> deps     ‹ npm ci ›
```

The row appears only when `Install` is available. A repo with a `package.json` but no
lockfile leaves `allowed == [Strategy::None]`, which would render a focusable row with a
single value that does nothing; in that case the row is hidden, as it already is for a
repo with no `package.json` at all.

`Field::Modules` is renamed `Field::Deps` and `shows_modules()` becomes `shows_deps()`.

### `src/components/list.rs` and `src/app.rs`

The modules column is dropped. With sharing gone, `own` no longer contrasts with `link` —
it only restates that an install happened, which the create flow already reports.

This removes `Row::nm`, the `show_modules` constructor parameter, the `nm_width` layout
arithmetic, and the `node::nm_state` call in `app.rs`.

### `src/components/progress.rs`

No code change. The relabelling in `plan_steps` is what the screen renders:

```
✓ git worktree add  feature/spe-11667   created
⠹ npm ci  app
  npm ci  tools
```

### `README.md`

The strategy table and the hardlink measurement paragraph go. The section becomes a short
statement: new worktrees run `npm ci` in every directory that has a `package.json` next to
a lockfile, or nothing. The monorepo behaviour and the `skipped (not in this worktree)`
note stay.

## Testing

Tests for the removed mechanisms go with them: the `hardlink_modules`, `symlink_modules`,
`same_filesystem`, `package_count` and `nm_state` tests, and the `Hardlink` and `Symlink`
cases in `plan_steps` and `targets_for`.

Tests that stay, because the behaviour they cover is unchanged:

- `discover_targets` finds nested packages sorted, skips dot directories, does not descend
  into `node_modules`, and flags the lockfile
- `targets_for(Install, ..)` skips packages without a lockfile and covers a marker root
  over locked packages
- `skip_reason` never skips the worktree add, and skips a directory absent from the
  destination
- `npm_error_summary` reports the reason rather than the code, and falls back on empty
  stderr

New tests:

- `Strategy::label()` returns `npm ci` and `skip`
- `plan_steps` labels a root package `npm ci` and a nested one `npm ci  app`
- the form renders the `deps` row showing `npm ci`
- the form hides the `deps` row when only `Strategy::None` is available
- the list renders no modules column

## Out of scope

- Any Rust prepare step. A shared `CARGO_TARGET_DIR` is the likely candidate, but cargo
  locks the target directory, so concurrent builds across worktrees would serialize. That
  is a real design problem and needs its own spec.
- Restoring a fast path for large `node_modules` trees. The hardlink implementation
  remains in git history if measurement later justifies bringing it back, gated on a
  lockfile comparison rather than applied unconditionally.
- Package managers other than npm.
