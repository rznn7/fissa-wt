# Node Dependency Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `fissa`'s four node dependency strategies to `install` and `none`, and rename the UI to describe the command that runs.

**Architecture:** `Strategy` collapses to two variants, which deletes every mechanism that existed only to share `node_modules` between worktrees (`cp -al`, symlinks, filesystem comparison) and the list column that reported the result. The form row is then renamed from `modules` to `deps` and labelled with the literal command, and progress steps are labelled by command instead of by path.

**Tech Stack:** Rust 2024 edition, `ratatui` + `crossterm` for the TUI, `anyhow` for errors, `tempfile` for filesystem tests. No new dependencies.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-06-node-deps-simplification-design.md`
- Tests are inline: `#[cfg(test)] mod tests { use super::*; … }` at the bottom of the file under test. No new test files.
- Test naming: `test_<unit>_<condition>_<expected>`.
- One behaviour per test, arrange/act/assert, no shared mutable fixtures.
- Assertions on rendered output must convert to `chars().count()` before comparing positions — `…` and `│` are three bytes each.
- Full check before every commit: `make test` then `make lint` (`cargo fmt --check` and `cargo clippy -- -D warnings`, warnings are errors).
- Commit messages are a single line. No body, no bullet list, no trailers.
- npm only. No yarn, pnpm or bun.
- No Rust prepare step — Rust repos are already supported by virtue of having no `package.json`.

---

### Task 1: Collapse `Strategy` to `install` and `none`

Removing the two sharing strategies is one atomic change: the crate does not compile with a `Strategy::Hardlink` variant that no `match` arm handles, so the enum, the actions, the mechanisms, and the test fixtures all move together.

**Files:**
- Modify: `src/node.rs`
- Modify: `src/create.rs`
- Modify: `src/app.rs:240`
- Modify: `src/components/create_form.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `node::Strategy` with exactly `Install` and `None`; `Strategy::ALL: [Strategy; 2]`; `Strategy::label(&self) -> &'static str`
  - `node::Target { rel: PathBuf, has_lockfile: bool }` — no `has_source_modules`
  - `node::available_strategies(targets: &[Target]) -> Vec<Strategy>`
  - `node::unavailable_reason(strategy: Strategy, targets: &[Target]) -> Option<&'static str>`
  - `create::Action` with exactly `AddWorktree { branch, base }` and `Install { rel }`

- [ ] **Step 1: Delete the tests for the mechanisms being removed**

In `src/node.rs`, delete these test functions entirely:

- `test_discover_targets_flags_missing_source_modules`
- `test_discover_targets_symlinked_modules_are_not_source_modules`
- `test_targets_for_hardlink_requires_source_modules`
- `test_targets_for_install_includes_packages_without_modules`
- `test_same_filesystem_within_one_tempdir_is_true`
- `test_package_count_counts_top_level_entries`
- `test_hardlink_modules_shares_inodes_and_separates_trees`
- `test_hardlink_modules_preserves_symlinks_as_symlinks`
- `test_symlink_modules_creates_a_symlink_to_the_source`
- `test_available_strategies_include_hardlink_on_one_filesystem`
- `test_available_strategies_exclude_sharing_without_source_modules`
- `test_unavailable_reason_names_the_missing_source_modules`
- `test_unavailable_reason_is_none_when_available`

In `src/create.rs`, delete:

- `test_plan_steps_hardlink_covers_only_targets_with_source_modules`
- `test_plan_steps_symlink_produces_symlink_actions`

In `src/components/create_form.rs`, delete:

- `test_modules_field_only_offers_allowed_strategies`

Keep the `write_modules` helper in `src/node.rs` — the `nm_state` tests still use it.

- [ ] **Step 2: Write the failing tests for the new availability rules**

Add to `src/node.rs` tests:

```rust
#[test]
fn test_available_strategies_offers_install_when_a_lockfile_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pkg(root, "app");
    write_lock(root, "app");

    let targets = discover_targets(root);
    assert_eq!(
        available_strategies(&targets),
        vec![Strategy::Install, Strategy::None]
    );
}

#[test]
fn test_available_strategies_offers_only_none_without_a_lockfile() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pkg(root, "app");

    let targets = discover_targets(root);
    assert_eq!(available_strategies(&targets), vec![Strategy::None]);
}
```

Update the surviving signature-dependent test in `src/node.rs` to the new two-argument form:

```rust
#[test]
fn test_unavailable_reason_install_needs_a_lockfile() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_pkg(root, ".");

    let targets = discover_targets(root);
    assert_eq!(
        unavailable_reason(Strategy::Install, &targets),
        Some("no package-lock.json next to a package.json")
    );
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib node::tests 2>&1 | tail -20`
Expected: FAIL to compile — `this function takes 2 arguments but 4 arguments were supplied` for `available_strategies` and `unavailable_reason`.

- [ ] **Step 4: Collapse the `Strategy` enum in `src/node.rs`**

Replace the `Strategy` enum and its `impl` block (currently lines 6–30) with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Install,
    None,
}

impl Strategy {
    pub const ALL: [Strategy; 2] = [Strategy::Install, Strategy::None];

    pub fn label(&self) -> &'static str {
        match self {
            Strategy::Install => "install",
            Strategy::None => "none",
        }
    }
}
```

- [ ] **Step 5: Drop `has_source_modules` from `Target` and `discover_targets`**

Replace the `Target` struct with:

```rust
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub rel: PathBuf,
    pub has_lockfile: bool,
}
```

In `discover_targets`, delete the `let mut has_modules = false;` binding, change the `node_modules` branch so it still stops the descent without recording anything:

```rust
            if name == "node_modules" {
                continue;
            }
```

and drop the field from the pushed value:

```rust
        if has_package_json && let Ok(rel) = dir.strip_prefix(source_root) {
            found.push(Target {
                rel: rel.to_path_buf(),
                has_lockfile,
            });
        }
```

- [ ] **Step 6: Simplify `targets_for` and the availability functions**

Replace `targets_for`:

```rust
pub fn targets_for(strategy: Strategy, targets: &[Target]) -> Vec<&Target> {
    match strategy {
        Strategy::Install => targets.iter().filter(|t| t.has_lockfile).collect(),
        Strategy::None => Vec::new(),
    }
}
```

Replace `available_strategies` and `unavailable_reason`:

```rust
pub fn available_strategies(targets: &[Target]) -> Vec<Strategy> {
    Strategy::ALL
        .into_iter()
        .filter(|strategy| unavailable_reason(*strategy, targets).is_none())
        .collect()
}

pub fn unavailable_reason(strategy: Strategy, targets: &[Target]) -> Option<&'static str> {
    match strategy {
        Strategy::Install => targets_for(strategy, targets)
            .is_empty()
            .then_some("no package-lock.json next to a package.json"),
        Strategy::None => None,
    }
}
```

- [ ] **Step 7: Delete the sharing mechanisms from `src/node.rs`**

Delete these functions entirely: `same_filesystem`, `hardlink_modules`, `symlink_modules`, `package_count`.

Then fix the now-unused imports at the top of the file. `Stdio` and `Command` are still used by `npm_ci`, so the import line stays as:

```rust
use std::process::{Command, Stdio};
```

but `anyhow::Context` and `anyhow::anyhow` are both still used by `npm_ci`, so `use anyhow::{Context, Result, anyhow};` is unchanged.

- [ ] **Step 8: Update the remaining `Target` literals in `src/node.rs` tests**

```rust
#[test]
fn test_targets_for_install_skips_packages_without_a_lockfile() {
    let targets = vec![
        Target {
            rel: PathBuf::new(),
            has_lockfile: false,
        },
        Target {
            rel: PathBuf::from("app"),
            has_lockfile: true,
        },
    ];
    let selected = targets_for(Strategy::Install, &targets);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].rel, PathBuf::from("app"));
}

#[test]
fn test_targets_for_none_selects_nothing() {
    let targets = vec![Target {
        rel: PathBuf::from("app"),
        has_lockfile: true,
    }];
    assert!(targets_for(Strategy::None, &targets).is_empty());
}
```

- [ ] **Step 9: Collapse `Action` in `src/create.rs`**

Replace the `Action` enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    AddWorktree { branch: String, base: String },
    Install { rel: PathBuf },
}
```

Replace the loop body in `plan_steps` — with only `Install` producing steps, the inner `match` disappears:

```rust
    for target in node::targets_for(strategy, targets) {
        let rel = target.rel.clone();
        let shown = if rel.as_os_str().is_empty() {
            String::from("node_modules")
        } else {
            format!("{}/node_modules", rel.to_string_lossy())
        };

        steps.push(Step {
            label: shown,
            action: Action::Install { rel },
        });
    }
```

Replace the match in `skip_reason`:

```rust
    let rel = match &step.action {
        Action::AddWorktree { .. } => return None,
        Action::Install { rel } => rel,
    };
```

Delete the `Action::Hardlink` and `Action::Symlink` arms from `run_step`, leaving:

```rust
    match &step.action {
        Action::AddWorktree { branch, base } => {
            let repo = crate::git::Repo::discover(repo_source)?;
            repo.add_worktree(&request.dest, branch, base)?;
            Ok(String::from("created"))
        }
        Action::Install { rel } => {
            let dir = request.dest.join(rel);
            node::npm_ci(&dir)?;
            Ok(String::from("installed"))
        }
    }
```

- [ ] **Step 10: Update the `src/create.rs` test fixtures**

```rust
    fn targets() -> Vec<Target> {
        vec![
            Target {
                rel: PathBuf::from("app"),
                has_lockfile: true,
            },
            Target {
                rel: PathBuf::from("tools"),
                has_lockfile: true,
            },
        ]
    }
```

In `test_plan_steps_install_leaves_out_a_marker_package_with_no_lockfile`, drop `has_source_modules` from both literals. In `test_skip_reason_is_none_when_the_directory_exists_in_the_destination`, change the action from `Action::Hardlink` to `Action::Install`:

```rust
    #[test]
    fn test_skip_reason_is_none_when_the_directory_exists_in_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("app")).unwrap();
        let step = Step {
            label: String::from("app/node_modules"),
            action: Action::Install {
                rel: PathBuf::from("app"),
            },
        };
        assert_eq!(skip_reason(tmp.path(), &step), None);
    }
```

- [ ] **Step 11: Update the `available_strategies` call site in `src/app.rs`**

In `open_form` (line 240), the source root and destination parent are no longer needed:

```rust
        let allowed = if targets.is_empty() {
            Vec::new()
        } else {
            node::available_strategies(&targets)
        };
```

- [ ] **Step 12: Update the `src/components/create_form.rs` test fixtures**

```rust
    fn form() -> CreateForm {
        CreateForm::new(
            String::from("spectra"),
            vec![
                String::from("feature/"),
                String::from("fix/"),
                String::new(),
            ],
            String::from("develop"),
            vec![Strategy::Install, Strategy::None],
        )
    }
```

```rust
    #[test]
    fn test_right_on_modules_field_selects_the_next_strategy() {
        let mut form = form();
        assert_eq!(form.strategy(), Strategy::Install);
        for _ in 0..3 {
            form.handle_event_key(key(KeyCode::Tab));
        }
        form.handle_event_key(key(KeyCode::Right));
        assert_eq!(form.strategy(), Strategy::None);
    }
```

In `test_render_shows_live_preview_and_error`, change the last assertion from `hardlink` to `install`:

```rust
        assert!(text.contains("install"), "{text}");
```

- [ ] **Step 13: Run the full suite**

Run: `make test`
Expected: PASS. If `cargo` reports an unused-import or dead-code warning, remove the offending item — `make lint` treats warnings as errors.

- [ ] **Step 14: Lint**

Run: `make lint`
Expected: no output, exit 0.

- [ ] **Step 15: Commit**

```bash
git add src/node.rs src/create.rs src/app.rs src/components/create_form.rs
git commit -m "Reduce node dependency strategies to install and none"
```

---

### Task 2: Drop the modules column from the list

With sharing gone, `own` no longer contrasts with `link` — the column only restates that an install happened, which the create flow already reports.

**Files:**
- Modify: `src/node.rs`
- Modify: `src/components/list.rs`
- Modify: `src/app.rs:38-59,89-98`

**Interfaces:**
- Consumes: `node::Target` from Task 1.
- Produces:
  - `list::Row { label: String, branch: String, dirty: bool, path: PathBuf }` — no `nm`
  - `ListComponent::new(repo_name: String, rows: Vec<Row>, shell_init: bool) -> ListComponent` — no `show_modules`
  - `app::build_rows(repo: &Repo) -> Result<Vec<Row>>` — no `targets` argument

- [ ] **Step 1: Write the failing test**

Replace `test_render_omits_the_modules_column_when_there_are_no_targets` in `src/components/list.rs` with a test that the column is gone unconditionally:

```rust
    #[test]
    fn test_render_shows_no_modules_column() {
        let text = dump(&mut component(true));
        assert!(!text.contains("link"), "{text}");
        assert!(!text.contains("own"), "{text}");
        assert!(text.contains("develop"), "{text}");
        assert!(text.contains('●'), "{text}");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib components::list::tests::test_render_shows_no_modules_column 2>&1 | tail -20`
Expected: FAIL to compile — `this function takes 1 argument but 2 arguments were supplied` for the `component` helper.

- [ ] **Step 3: Remove `NmState` from `src/node.rs`**

Delete the `NmState` enum, its `impl` block, and the `nm_state` function. Delete these tests:

- `test_nm_state_real_directory_is_own`
- `test_nm_state_symlink_is_link`
- `test_nm_state_absent_is_missing`

The `write_modules` test helper now has no callers except `test_discover_targets_does_not_descend_into_node_modules`, which keeps it. Leave it in place.

- [ ] **Step 4: Remove the column from `src/components/list.rs`**

Delete `use crate::node::NmState;`. Drop the field from `Row`:

```rust
pub struct Row {
    pub label: String,
    pub branch: String,
    pub dirty: bool,
    pub path: PathBuf,
}
```

Drop `show_modules` from the struct and constructor:

```rust
pub struct ListComponent {
    repo_name: String,
    rows: Vec<Row>,
    /// Indices into `rows` that survive the active filter, in display order.
    visible: Vec<usize>,
    /// The query `enter` committed.
    query: String,
    /// The live query while the search bar is open; `esc` drops it back to `query`.
    editing: Option<String>,
    state: ListState,
    shell_init: bool,
}

impl ListComponent {
    pub fn new(repo_name: String, rows: Vec<Row>, shell_init: bool) -> Self {
        let mut component = Self {
            repo_name,
            visible: (0..rows.len()).collect(),
            rows,
            query: String::new(),
            editing: None,
            state: ListState::default(),
            shell_init,
        };
        component.refilter();
        component
    }
```

In `render`, delete the `nm_width` binding and take it out of the `branch_width` arithmetic:

```rust
        let branch_width = inner.saturating_sub(2 + label_width + 1 + 2).max(8);
```

and delete the trailing `if self.show_modules { spans.push(...) }` block, so the item ends at:

```rust
                let spans = vec![
                    label,
                    Span::from(format!(
                        "{:<branch_width$}",
                        fit_tail(&row.branch, branch_width)
                    )),
                    Span::from(format!("{dirty} ")),
                ];
                ListItem::new(Line::from(spans))
```

Note the `let mut spans` becomes `let spans` — nothing pushes to it any more, and `clippy` will reject the redundant `mut`.

- [ ] **Step 5: Update the `src/components/list.rs` test fixtures**

```rust
    fn rows() -> Vec<Row> {
        vec![
            Row {
                label: String::from("spectra"),
                branch: String::from("develop"),
                dirty: false,
                path: PathBuf::from("/w/spectra"),
            },
            Row {
                label: String::from("spectra-ter"),
                branch: String::from("ter"),
                dirty: true,
                path: PathBuf::from("/w/spectra-ter"),
            },
        ]
    }

    fn component(shell_init: bool) -> ListComponent {
        ListComponent::new(String::from("spectra"), rows(), shell_init)
    }
```

Every call of the form `component(true, true)` or `component(false, true)` becomes `component(true)` / `component(false)`. In `test_render_keeps_the_branch_column_aligned_when_labels_are_long`, drop `nm` from both `Row` literals and change the constructor call to `ListComponent::new(String::from("r"), long_rows, true)`.

- [ ] **Step 6: Update `src/app.rs`**

`build_rows` no longer needs targets:

```rust
pub fn build_rows(repo: &Repo) -> Result<Vec<Row>> {
    let mut rows = Vec::new();

    for entry in repo.worktrees()? {
        if entry.bare {
            continue;
        }
        rows.push(Row {
            label: git::row_label(&entry.path, &repo.main_clone),
            branch: entry
                .branch
                .clone()
                .unwrap_or_else(|| String::from("(detached)")),
            dirty: repo.is_dirty(&entry.path),
            path: entry.path,
        });
    }

    rows.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(rows)
}
```

`make_list` no longer discovers targets at all:

```rust
fn make_list(repo: &Repo) -> Result<ListComponent> {
    let rows = build_rows(repo)?;
    Ok(ListComponent::new(
        repo.main_dir_name(),
        rows,
        shell::wrapper_active(),
    ))
}
```

Delete the `test_build_rows_reports_node_modules_state_per_worktree` test. In the surviving `build_rows` test, change `build_rows(&repo, &[])` to `build_rows(&repo)`.

- [ ] **Step 7: Run the full suite**

Run: `make test`
Expected: PASS.

- [ ] **Step 8: Lint**

Run: `make lint`
Expected: no output, exit 0. If `clippy` flags `use crate::node;` in `src/app.rs` as unused, keep it — `open_form` and `submit_form` still call `node::discover_targets`.

- [ ] **Step 9: Commit**

```bash
git add src/node.rs src/components/list.rs src/app.rs
git commit -m "Drop the modules column from the worktree list"
```

---

### Task 3: Rename the form row to `deps` and label it by command

**Files:**
- Modify: `src/node.rs`
- Modify: `src/components/create_form.rs`
- Modify: `src/app.rs:235-250`

**Interfaces:**
- Consumes: `node::Strategy` and `node::available_strategies` from Task 1.
- Produces:
  - `Strategy::label()` returning `"npm ci"` for `Install` and `"skip"` for `None`
  - `create_form::Field::Deps` (renamed from `Field::Modules`)
  - `CreateForm::shows_deps(&self) -> bool`

- [ ] **Step 1: Write the failing tests**

Add to `src/node.rs` tests:

```rust
#[test]
fn test_strategy_labels_name_the_command_that_runs() {
    assert_eq!(Strategy::Install.label(), "npm ci");
    assert_eq!(Strategy::None.label(), "skip");
}
```

Add to `src/components/create_form.rs` tests:

```rust
    #[test]
    fn test_render_labels_the_deps_row_with_the_command() {
        let mut form = form();
        let text = dump(&mut form);
        assert!(text.contains("deps"), "{text}");
        assert!(text.contains("npm ci"), "{text}");
        assert!(!text.contains("modules"), "{text}");
    }

    #[test]
    fn test_render_omits_the_deps_row_when_install_is_unavailable() {
        let mut form = CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![Strategy::None],
        );
        let text = dump(&mut form);
        assert!(!text.contains("deps"), "{text}");
        assert!(text.contains("slug"), "{text}");
    }

    #[test]
    fn test_focus_skips_deps_when_install_is_unavailable() {
        let mut form = CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![Strategy::None],
        );
        assert_eq!(form.focus(), Field::Slug);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Prefix);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Base);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Slug);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib 2>&1 | grep -E "^(test |failures:)" | head -20`
Expected: FAIL — `test_strategy_labels_name_the_command_that_runs` fails with `assertion \`left == right\` failed: left: "install", right: "npm ci"`, and the render tests fail on the missing `deps` substring.

- [ ] **Step 3: Change the strategy labels in `src/node.rs`**

```rust
    pub fn label(&self) -> &'static str {
        match self {
            Strategy::Install => "npm ci",
            Strategy::None => "skip",
        }
    }
```

- [ ] **Step 4: Rename the field and gate the row in `src/components/create_form.rs`**

Rename the enum variant:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Slug,
    Prefix,
    Base,
    Deps,
}
```

The row now appears only when `install` is genuinely on offer. A repo with a `package.json` but no lockfile leaves `allowed == [Strategy::None]`, which would otherwise render a focusable row with one value that does nothing:

```rust
    pub fn new(
        repo_dir: String,
        prefixes: Vec<String>,
        base: String,
        allowed: Vec<Strategy>,
    ) -> Self {
        let mut fields = vec![Field::Slug, Field::Prefix, Field::Base];
        if allowed.contains(&Strategy::Install) {
            fields.push(Field::Deps);
        }
```

```rust
    pub fn shows_deps(&self) -> bool {
        self.allowed.contains(&Strategy::Install)
    }
```

In `cycle_focused`, rename the arm:

```rust
            Field::Deps => {
                self.strategy_index = Self::cycle(self.strategy_index, self.allowed.len(), delta);
            }
```

In `render`, replace the conditional row. `deps` plus five spaces is nine characters, matching `slug`, `prefix`, `base` and the old `modules`:

```rust
        if self.shows_deps() {
            lines.push(format!(
                "{} deps     ‹ {} ›",
                marker(Field::Deps),
                self.strategy().label()
            ));
        }
```

- [ ] **Step 5: Update the remaining `Field::Modules` references in the tests**

In `src/components/create_form.rs`, rename `Field::Modules` to `Field::Deps` in `test_tab_cycles_focus_through_all_fields`, `test_up_arrow_from_the_first_field_wraps_to_the_last`, and `test_shift_tab_moves_to_the_previous_field`.

Rename `test_right_on_modules_field_selects_the_next_strategy` to `test_right_on_the_deps_field_selects_the_next_strategy`, `test_the_modules_field_hides_the_cursor` to `test_the_deps_field_hides_the_cursor`, and `test_render_omits_the_modules_row_when_no_strategies_are_available` to `test_render_omits_the_deps_row_when_no_strategies_are_available` (its body already asserts on `"modules"`; change that assertion to `"deps"`).

In `test_render_shows_live_preview_and_error`, change the `install` assertion from Task 1 to the new label:

```rust
        assert!(text.contains("npm ci"), "{text}");
```

Rename `test_focus_skips_modules_when_no_strategies_are_available` to `test_focus_skips_deps_when_no_strategies_are_available`.

- [ ] **Step 6: Simplify `open_form` in `src/app.rs`**

The empty-targets special case is now redundant: with no targets, `Install` is unavailable, `available_strategies` returns `[Strategy::None]`, and the form hides the row.

```rust
    fn open_form(&mut self) {
        let targets = node::discover_targets(&self.repo.source);
        let allowed = node::available_strategies(&targets);

        self.form = Some(CreateForm::new(
            self.repo.main_dir_name(),
            self.repo.prefixes(),
            self.repo.default_base(),
            allowed,
        ));
        self.screen = Screen::Create;
    }
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `make test`
Expected: PASS.

- [ ] **Step 8: Lint**

Run: `make lint`
Expected: no output, exit 0.

- [ ] **Step 9: Commit**

```bash
git add src/node.rs src/components/create_form.rs src/app.rs
git commit -m "Label the create form deps row with the command that runs"
```

---

### Task 4: Label progress steps by command

**Files:**
- Modify: `src/create.rs`
- Modify: `src/components/progress.rs`

**Interfaces:**
- Consumes: `create::plan_steps` and `create::Action` from Task 1.
- Produces: step labels of the form `npm ci` (repository root) and `npm ci  app` (nested package), matching the two-space separator already used by `git worktree add  feature/x`.

- [ ] **Step 1: Write the failing tests**

Add to `src/create.rs` tests:

```rust
    #[test]
    fn test_plan_steps_labels_a_root_package_with_the_bare_command() {
        let targets = vec![Target {
            rel: PathBuf::new(),
            has_lockfile: true,
        }];
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets);
        assert_eq!(steps[1].label, "npm ci");
    }

    #[test]
    fn test_plan_steps_labels_a_nested_package_with_its_path() {
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets());
        assert_eq!(steps[1].label, "npm ci  app");
        assert_eq!(steps[2].label, "npm ci  tools");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib create::tests 2>&1 | tail -20`
Expected: FAIL — `assertion \`left == right\` failed: left: "app/node_modules", right: "npm ci  app"`.

- [ ] **Step 3: Relabel the steps in `plan_steps`**

```rust
    for target in node::targets_for(strategy, targets) {
        let rel = target.rel.clone();
        let shown = if rel.as_os_str().is_empty() {
            String::from("npm ci")
        } else {
            format!("npm ci  {}", rel.to_string_lossy())
        };

        steps.push(Step {
            label: shown,
            action: Action::Install { rel },
        });
    }
```

- [ ] **Step 4: Update the existing label assertion**

In `test_plan_steps_install_leaves_out_a_marker_package_with_no_lockfile`:

```rust
        assert_eq!(steps[1].label, "npm ci  app");
```

- [ ] **Step 5: Update the `src/components/progress.rs` fixtures**

In `component_with_shell_init` and in `test_render_keeps_the_detail_column_aligned_when_labels_are_long`, change the second label from `String::from("app/node_modules")` to `String::from("npm ci  app")`.

In `test_render_shows_step_labels_and_detail`, change the label assertion:

```rust
        assert!(text.contains("npm ci  app"), "{text}");
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `make test`
Expected: PASS.

- [ ] **Step 7: Lint**

Run: `make lint`
Expected: no output, exit 0.

- [ ] **Step 8: Commit**

```bash
git add src/create.rs src/components/progress.rs
git commit -m "Label progress steps by the command they run"
```

---

### Task 5: Update the documentation

**Files:**
- Modify: `README.md`
- Modify: `TESTING.md`

**Interfaces:**
- Consumes: the behaviour delivered by Tasks 1–4.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Replace the `node_modules` section of `README.md`**

Replace the whole section from the `## node_modules` heading down to (but not including) `## Development` with:

```markdown
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
```

Also update the `create` and `progress` rows of the keys table if they mention modules — they do not, so no change is needed there.

- [ ] **Step 2: Sweep `README.md` for stale strategy references**

Search for any remaining mention of the removed strategies:

Run: `grep -n "hardlink\|symlink\|node_modules\|modules column" README.md`
Expected: only the `skipped (not in this worktree)` paragraph's context remains. Delete or reword any other hit, including the "Measured on a 1073 MB…" paragraph and the "Removing a worktree created this way never touches the source" paragraph, both of which describe hardlinking.

- [ ] **Step 3: Update `TESTING.md`**

In the layers table, change the **Filesystem logic** row's "What we assert" cell from:

```
target discovery, `node_modules` state, hardlink/symlink behaviour, strategy availability
```

to:

```
target discovery, strategy availability
```

In "What we deliberately don't test", delete the `cp -al` bullet:

```
- `cp -al` as invoked on a real 1 GB tree — the inode-sharing behaviour is covered on a
  small temp tree instead.
```

In the manual verification checklist, replace item 3:

```
3. Creating a worktree with `npm ci` leaves a `node_modules` in each locked package and
   the result builds.
```

and item 6:

```
6. On a repo with no `package.json`, the form's `deps` row is absent.
```

- [ ] **Step 4: Verify no stale references remain**

Run: `grep -rn "hardlink\|symlink\|NmState\|nm_state\|show_modules\|has_source_modules\|same_filesystem\|package_count" README.md TESTING.md src/`
Expected: no output. (`docs/superpowers/specs/` legitimately still names them — it is the record of the decision — so it is excluded from this check.)

- [ ] **Step 5: Run the full suite one last time**

Run: `make test && make lint`
Expected: PASS, then no output from lint.

- [ ] **Step 6: Commit**

```bash
git add README.md TESTING.md
git commit -m "Document npm ci as the only dependency strategy"
```

---

## Verification

After Task 5, confirm the end state by hand:

```bash
cargo install --path .
```

`cargo build` does not update an installed binary — re-run `cargo install --path .` before any manual check, or you will be testing the previous version.

1. In a repo with a `package.json` and a `package-lock.json`, press `n`: the form shows `deps ‹ npm ci ›`, and `←`/`→` cycles to `‹ skip ›`.
2. In a repo with a `package.json` but no lockfile, the `deps` row is absent.
3. In a repo with no `package.json` (this one), the `deps` row is absent.
4. The list shows no modules column in any of the three cases.
5. Creating with `npm ci` shows progress steps reading `npm ci  <package>`.
