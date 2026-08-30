# Worktree Local File Copy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Copy the local files a repository declares in `.fissa.toml` into each newly created worktree.

**Architecture:** A new `src/manifest.rs` owns both halves of the feature, mirroring how `src/node.rs` owns discovery and `install`: it parses `.fissa.toml` from the repo root and it performs a single file copy. `src/create.rs` gains an `Action::CopyLocal { rel }` step per declared entry, planned after `InitSubmodules` and before every `Install` so it lands in the sequential prefix of `run_steps`. `src/main.rs` loads the manifest after `Repo::discover` and before the terminal enters raw mode, so a malformed file exits with a message instead of corrupting the display.

**Tech Stack:** Rust 2024, `serde` + `toml` (both already dependencies), `anyhow` for errors, `tempfile` for tests. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-30-worktree-local-file-copy-design.md`

## Global Constraints

- No new crate dependencies. `Cargo.toml` is unchanged by this plan.
- Linux only. Unix-specific APIs (`std::os::unix::fs`) are acceptable.
- `make lint` runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`. Every task must end with both clean, so no task may leave an unused public item behind — this is why the tasks are vertical slices of behaviour rather than one module per task.
- The project forbids code comments. Names and structure carry the meaning.
- Commit messages are a single line. No body, no bullet list, no trailers.
- Test names are full sentences in the existing style: `test_<what>_<does what>`.
- Every task ends with `make test` and `make lint` both green, then a commit.

---

### Task 1: Walking skeleton — declared files are copied

Delivers the smallest end-to-end slice: a manifest listing one file, a planned step per entry, and a copy that works for the ordinary case. Validation and skip cases arrive in Tasks 2 and 3.

**Files:**
- Create: `src/manifest.rs`
- Modify: `src/main.rs` (add `mod manifest;`, load the manifest, pass it to `App::new`)
- Modify: `src/app.rs` (store the copy list, pass it to `plan_steps`)
- Modify: `src/create.rs` (`Action::CopyLocal`, `plan_steps` parameter, `run_step` arm)
- Test: in-module `#[cfg(test)]` blocks in `src/manifest.rs` and `src/create.rs`

**Interfaces:**
- Consumes: `git::Repo { source, main_clone }`, `create::{Step, Action, plan_steps, run_step}`, `config::load_from` as the pattern for a missing file.
- Produces:
  - `manifest::load(source_root: &Path) -> anyhow::Result<Vec<PathBuf>>`
  - `manifest::parse(text: &str) -> anyhow::Result<Vec<PathBuf>>`
  - `manifest::copy_file(source_root: &Path, dest_root: &Path, rel: &Path) -> anyhow::Result<String>`
  - `create::Action::CopyLocal { rel: PathBuf }`
  - `create::plan_steps(branch, base, strategy, submodules, targets, copy: &[PathBuf]) -> Vec<Step>` — note `copy` is the new **last** parameter.

- [ ] **Step 1: Write the failing manifest parse tests**

Create `src/manifest.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_text_declares_nothing() {
        assert_eq!(parse("").unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_parse_reads_the_worktree_copy_list() {
        let text = "[worktree]\ncopy = [\".env\", \"config/local.yml\"]";
        assert_eq!(
            parse(text).unwrap(),
            vec![PathBuf::from(".env"), PathBuf::from("config/local.yml")]
        );
    }

    #[test]
    fn test_parse_ignores_tables_it_does_not_know() {
        let text = "[hooks]\npost_create = [\"x\"]";
        assert_eq!(parse(text).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_parse_rejects_malformed_toml() {
        assert!(parse("[worktree]\ncopy = ").is_err());
    }

    #[test]
    fn test_load_from_a_repository_without_a_manifest_declares_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_load_reads_the_manifest_at_the_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".fissa.toml"), "[worktree]\ncopy = [\".env\"]").unwrap();
        assert_eq!(load(dir.path()).unwrap(), vec![PathBuf::from(".env")]);
    }

    #[test]
    fn test_load_error_names_the_offending_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".fissa.toml");
        std::fs::write(&path, "[worktree]\ncopy = ").unwrap();
        let message = load(dir.path()).unwrap_err().to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
    }
}
```

Add `mod manifest;` to the module list at the top of `src/main.rs`, keeping the list alphabetical: after `mod git;` and before `mod naming;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test manifest 2>&1 | head -30`
Expected: compilation failure — `cannot find function 'parse' in this scope`, `cannot find function 'load' in this scope`.

- [ ] **Step 3: Implement parse and load**

Put this above the test module in `src/manifest.rs`:

```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Manifest {
    worktree: Worktree,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Worktree {
    copy: Vec<String>,
}

pub fn parse(text: &str) -> Result<Vec<PathBuf>> {
    let manifest: Manifest = toml::from_str(text)?;
    Ok(manifest.worktree.copy.iter().map(PathBuf::from).collect())
}

/// A missing manifest is not an error: it is how most repositories run.
pub fn load(source_root: &Path) -> Result<Vec<PathBuf>> {
    let path = source_root.join(".fissa.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    parse(&text).with_context(|| format!("invalid manifest in {}", path.display()))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test manifest`
Expected: 7 passed.

- [ ] **Step 5: Write the failing copy test**

Add to the test module in `src/manifest.rs`:

```rust
    #[test]
    fn test_copy_file_puts_the_declared_file_in_the_worktree() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".env"), "TOKEN=1").unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.path().join(".env")).unwrap(),
            "TOKEN=1"
        );
        assert!(detail.starts_with("copied"), "{detail}");
    }
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test test_copy_file_puts 2>&1 | head -20`
Expected: `cannot find function 'copy_file' in this scope`.

- [ ] **Step 7: Implement copy_file**

Add to `src/manifest.rs`:

```rust
pub fn copy_file(source_root: &Path, dest_root: &Path, rel: &Path) -> Result<String> {
    let source = source_root.join(rel);
    let dest = dest_root.join(rel);

    let bytes = std::fs::copy(&source, &dest)
        .with_context(|| format!("could not copy {}", rel.display()))?;

    Ok(format!("copied ({})", human_bytes(bytes)))
}

fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}
```

- [ ] **Step 8: Run it to verify it passes**

Run: `cargo test manifest`
Expected: 8 passed.

- [ ] **Step 9: Write the failing planning tests**

Add to the test module in `src/create.rs`:

```rust
    #[test]
    fn test_plan_steps_adds_a_copy_step_for_every_declared_file() {
        let copy = [PathBuf::from(".env"), PathBuf::from("config/local.yml")];
        let steps = plan_steps("feature/x", "develop", Strategy::None, false, &[], &copy);

        let labels: Vec<&str> = steps
            .iter()
            .filter(|step| matches!(step.action, Action::CopyLocal { .. }))
            .map(|step| step.label.as_str())
            .collect();

        assert_eq!(labels, vec!["copy  .env", "copy  config/local.yml"]);
    }

    #[test]
    fn test_plan_steps_puts_every_copy_after_the_submodule_init() {
        let copy = [PathBuf::from(".env")];
        let steps = plan_steps("feature/x", "develop", Strategy::None, true, &[], &copy);

        let submodules = steps
            .iter()
            .position(|step| step.action == Action::InitSubmodules)
            .unwrap();
        let first_copy = steps
            .iter()
            .position(|step| matches!(step.action, Action::CopyLocal { .. }))
            .unwrap();

        assert!(submodules < first_copy);
    }

    #[test]
    fn test_plan_steps_puts_every_copy_before_every_install() {
        let copy = [PathBuf::from(".env")];
        let steps = plan_steps(
            "feature/x",
            "develop",
            Strategy::Install,
            false,
            &targets(),
            &copy,
        );

        let last_copy = steps
            .iter()
            .rposition(|step| matches!(step.action, Action::CopyLocal { .. }))
            .unwrap();
        let first_install = steps
            .iter()
            .position(|step| matches!(step.action, Action::Install { .. }))
            .unwrap();

        assert!(last_copy < first_install);
    }
```

- [ ] **Step 10: Run them to verify they fail**

Run: `cargo test plan_steps 2>&1 | head -20`
Expected: compilation failure — `plan_steps` takes 5 arguments but 6 were supplied, and `no variant named CopyLocal`.

- [ ] **Step 11: Add the action, the parameter and the planning**

In `src/create.rs`, add the variant to `Action`:

```rust
    CopyLocal {
        rel: PathBuf,
    },
```

Give `plan_steps` the new trailing parameter and emit the steps immediately after the submodule block, before the install loop:

```rust
pub fn plan_steps(
    branch: &str,
    base: &str,
    strategy: Strategy,
    submodules: bool,
    targets: &[Target],
    copy: &[PathBuf],
) -> Vec<Step> {
```

```rust
    for rel in copy {
        steps.push(Step {
            label: format!("copy  {}", rel.display()),
            action: Action::CopyLocal { rel: rel.clone() },
        });
    }
```

Add `CopyLocal` to the early-return arm of `skip_reason`, whose filesystem checks apply only to installs:

```rust
        Action::AddWorktree { .. } | Action::InitSubmodules | Action::CopyLocal { .. } => {
            return None;
        }
```

Handle it in `run_step`:

```rust
        Action::CopyLocal { rel } => {
            crate::manifest::copy_file(repo_source, &request.dest, rel)
        }
```

Update the eleven existing `plan_steps` calls in the test module by appending an empty copy list:

```bash
sed -i 's/&targets())\;/\&targets(), \&[]);/; s/&targets)\;/\&targets, \&[]);/' src/create.rs
```

Verify the substitution touched only test call sites and not the new tests you just wrote, which already pass six arguments:

```bash
grep -n 'plan_steps(' src/create.rs
```

- [ ] **Step 12: Run the tests to verify they pass**

Run: `cargo test create`
Expected: all `create::tests` pass, including the three new ones.

- [ ] **Step 13: Wire the manifest through main and app**

In `src/main.rs`, load the manifest after `Repo::discover` and before `init_terminal`, so a bad file exits before raw mode:

```rust
    let repo = git::Repo::discover(&std::env::current_dir()?)?;
    let copy = manifest::load(&repo.source)?;
    let mut terminal = app::init_terminal()?;
    let result = app::App::new(repo, config, copy).and_then(|app| app.run(&mut terminal));
```

This sits after the `fissa init <shell>` branch, which returns before any load and must stay that way: it is evaluated on every shell startup, and a broken manifest in one repository must not break the user's shell.

In `src/app.rs`, add the field to `App`:

```rust
    copy: Vec<PathBuf>,
```

Take it in the constructor and store it:

```rust
    pub fn new(repo: Repo, config: Config, copy: Vec<PathBuf>) -> Result<App> {
```

```rust
            repo,
            copy,
            list,
```

Pass it as the new trailing argument at `src/app.rs:363`:

```rust
        let steps = create::plan_steps(
            &branch,
            form.base(),
            form.strategy(),
            form.submodules(),
            &targets,
            &self.copy,
        );
```

- [ ] **Step 14: Verify the whole suite and the lint**

Run: `make test && make lint`
Expected: all tests pass; `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` both silent.

- [ ] **Step 15: Commit**

```bash
git add src/manifest.rs src/main.rs src/app.rs src/create.rs
git commit -m "Copy the local files a repository declares into each new worktree"
```

---

### Task 2: Entry validation

The manifest is committed, so in any cloned repository it is attacker-controlled input. These rules are a security boundary, not a convenience.

**Files:**
- Modify: `src/manifest.rs` (add `validate`, call it from `parse`)
- Test: in-module `#[cfg(test)]` block in `src/manifest.rs`

**Interfaces:**
- Consumes: `manifest::parse` from Task 1.
- Produces: no new public surface. `parse` keeps the signature `parse(text: &str) -> anyhow::Result<Vec<PathBuf>>` and now fails on an invalid entry.

- [ ] **Step 1: Write the failing validation tests**

Add to the test module in `src/manifest.rs`:

```rust
    fn rejection(entry: &str) -> String {
        let text = format!("[worktree]\ncopy = [\"{entry}\"]");
        parse(&text)
            .expect_err("expected the entry to be rejected")
            .to_string()
    }

    #[test]
    fn test_parse_rejects_an_absolute_path() {
        assert!(rejection("/etc/passwd").contains("/etc/passwd"));
    }

    #[test]
    fn test_parse_rejects_a_path_that_climbs_out_of_the_repository() {
        assert!(rejection("../../.ssh/id_rsa").contains("id_rsa"));
    }

    #[test]
    fn test_parse_rejects_a_single_dot_component() {
        assert!(rejection("./.env").contains(".env"));
    }

    #[test]
    fn test_parse_rejects_a_wildcard() {
        let message = rejection(".env*");
        assert!(message.contains("literal"), "{message}");
    }

    #[test]
    fn test_parse_rejects_a_directory() {
        let message = rejection("config/");
        assert!(message.contains("file"), "{message}");
    }

    #[test]
    fn test_parse_rejects_an_empty_entry() {
        assert!(parse("[worktree]\ncopy = [\"\"]").is_err());
    }

    #[test]
    fn test_parse_keeps_accepting_a_nested_literal_path() {
        assert_eq!(
            parse("[worktree]\ncopy = [\"config/secrets.local.yml\"]").unwrap(),
            vec![PathBuf::from("config/secrets.local.yml")]
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test manifest 2>&1 | tail -30`
Expected: `rejection` panics with "expected the entry to be rejected" for the absolute, climbing, dot, wildcard and directory cases; `test_parse_rejects_an_empty_entry` fails on `is_err()`.

- [ ] **Step 3: Implement validation**

In `src/manifest.rs`, extend the import to `use std::path::{Component, Path, PathBuf};`, route `parse` through the new function, and add it:

```rust
pub fn parse(text: &str) -> Result<Vec<PathBuf>> {
    let manifest: Manifest = toml::from_str(text)?;
    manifest
        .worktree
        .copy
        .iter()
        .map(String::as_str)
        .map(validate)
        .collect()
}

fn validate(entry: &str) -> Result<PathBuf> {
    if entry.is_empty() {
        return Err(anyhow!("a copy entry is empty"));
    }
    if entry.contains(['*', '?', '[']) {
        return Err(anyhow!(
            "copy entry '{entry}' looks like a pattern; only literal file paths are supported"
        ));
    }
    if entry.ends_with('/') {
        return Err(anyhow!(
            "copy entry '{entry}' is a directory; only files are supported"
        ));
    }

    let path = PathBuf::from(entry);
    if path.is_absolute() {
        return Err(anyhow!(
            "copy entry '{entry}' must be relative to the repository root"
        ));
    }
    if path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(anyhow!(
            "copy entry '{entry}' must not leave the repository root"
        ));
    }

    Ok(path)
}
```

Add `anyhow!` to the `anyhow` import: `use anyhow::{Context, Result, anyhow};`

The `Component::Normal` check is what actually forbids `.` and `..`; the `is_absolute` check precedes it only so an absolute path gets the more specific message.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test manifest`
Expected: all manifest tests pass, 15 total.

- [ ] **Step 5: Verify the whole suite and the lint**

Run: `make test && make lint`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add src/manifest.rs
git commit -m "Reject copy entries that are absolute, climbing, patterned or directories"
```

---

### Task 3: Skip cases and file fidelity

`report` in `src/create.rs` halts the sequential prefix on `Err`, so a teammate who never created `.env.local` must not lose their installs over it. Only a genuine IO error fails the step.

**Files:**
- Modify: `src/manifest.rs` (`copy_file` gains its guards)
- Test: in-module `#[cfg(test)]` block in `src/manifest.rs`

**Interfaces:**
- Consumes: `manifest::copy_file(source_root: &Path, dest_root: &Path, rel: &Path) -> anyhow::Result<String>` from Task 1.
- Produces: no signature change. The returned detail is now one of `copied (<size>)`, `skipped (not in the source)`, `skipped (symlink)`, `skipped (not a regular file)`, or `skipped (already in the worktree)`.

- [ ] **Step 1: Write the failing skip and fidelity tests**

Add to the test module in `src/manifest.rs`:

```rust
    #[test]
    fn test_copy_file_skips_a_source_that_does_not_exist() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (not in the source)");
        assert!(!dest.path().join(".env").exists());
    }

    #[test]
    fn test_copy_file_leaves_a_destination_that_already_exists() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".env"), "from source").unwrap();
        std::fs::write(dest.path().join(".env"), "already here").unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (already in the worktree)");
        assert_eq!(
            std::fs::read_to_string(dest.path().join(".env")).unwrap(),
            "already here"
        );
    }

    #[test]
    fn test_copy_file_skips_a_symlink_without_following_it() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let secret = source.path().join("secret");
        std::fs::write(&secret, "not yours").unwrap();
        std::os::unix::fs::symlink(&secret, source.path().join(".env")).unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (symlink)");
        assert!(!dest.path().join(".env").exists());
    }

    #[test]
    fn test_copy_file_skips_a_directory_standing_where_a_file_was_declared() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join(".env")).unwrap();

        let detail = copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        assert_eq!(detail, "skipped (not a regular file)");
    }

    #[test]
    fn test_copy_file_creates_a_parent_the_worktree_does_not_have() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("config")).unwrap();
        std::fs::write(source.path().join("config/local.yml"), "k: v").unwrap();

        copy_file(source.path(), dest.path(), Path::new("config/local.yml")).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.path().join("config/local.yml")).unwrap(),
            "k: v"
        );
    }

    #[test]
    fn test_copy_file_preserves_the_permissions_of_a_private_file() {
        use std::os::unix::fs::PermissionsExt;

        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let path = source.path().join(".env");
        std::fs::write(&path, "TOKEN=1").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        copy_file(source.path(), dest.path(), Path::new(".env")).unwrap();

        let mode = std::fs::metadata(dest.path().join(".env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test copy_file 2>&1 | tail -30`
Expected: the missing-source, existing-destination, symlink, directory and missing-parent tests all fail — the first four because `std::fs::copy` returns `Err` or clobbers, the fifth because the parent does not exist. `test_copy_file_preserves_the_permissions_of_a_private_file` may already pass, since `std::fs::copy` carries the mode across.

- [ ] **Step 3: Add the guards to copy_file**

Replace the body of `copy_file` in `src/manifest.rs`:

```rust
pub fn copy_file(source_root: &Path, dest_root: &Path, rel: &Path) -> Result<String> {
    let source = source_root.join(rel);
    let Ok(meta) = std::fs::symlink_metadata(&source) else {
        return Ok(String::from("skipped (not in the source)"));
    };
    if meta.file_type().is_symlink() {
        return Ok(String::from("skipped (symlink)"));
    }
    if !meta.is_file() {
        return Ok(String::from("skipped (not a regular file)"));
    }

    let dest = dest_root.join(rel);
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Ok(String::from("skipped (already in the worktree)"));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    let bytes = std::fs::copy(&source, &dest)
        .with_context(|| format!("could not copy {}", rel.display()))?;

    Ok(format!("copied ({})", human_bytes(bytes)))
}
```

`symlink_metadata` is load-bearing in both places. On the source it stops `.env -> /var/lib/libvirt/images/debian.qcow2` measuring 40 bytes at every check and copying 40 GB. On the destination it catches a dangling symlink, which `exists()` would report as absent.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test manifest`
Expected: all manifest tests pass, 21 total.

- [ ] **Step 5: Verify the whole suite and the lint**

Run: `make test && make lint`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add src/manifest.rs
git commit -m "Skip a declared file that is missing, symlinked or already in the worktree"
```

---

### Task 4: Documentation

**Files:**
- Modify: `README.md` (a Features bullet, a Configuration section)
- Modify: `src/main.rs` (the `HELP` constant, which documents only the user config today)

**Interfaces:**
- Consumes: the behaviour built in Tasks 1 to 3.
- Produces: nothing other tasks depend on.

- [ ] **Step 1: Add the Features bullet**

In `README.md`, insert directly before the `- **npm, pnpm, yarn, bun and deno**` bullet, so the list keeps its create-order shape:

```markdown
- **Declared local files**: a repo's `.fissa.toml` names the gitignored files a
  worktree needs — `.env` and friends — and each new worktree gets them.
```

- [ ] **Step 2: Add the Configuration section**

In `README.md`, append to the `## Configuration` section, after the paragraph describing `ascii`:

````markdown
A repository can also declare, in a committed `.fissa.toml` at its root, the
gitignored files every worktree needs:

```toml
[worktree]
copy = [".env", "config/secrets.local.yml"]
```

Each entry is a literal path to a single file, relative to the repository root.
Patterns, directories and paths leaving the root are refused. A file that is
missing, is a symlink, or already exists in the new worktree is skipped rather
than copied.
````

- [ ] **Step 3: Extend the help text**

In `src/main.rs`, append to the `CONFIGURATION:` block of `HELP`, after the `icons` line:

```rust
    .fissa.toml at the repository root, committed

        [worktree]
        copy = [\".env\"]   # literal file paths, copied into new worktrees
```

- [ ] **Step 4: Verify the help still renders and the suite is green**

Run: `cargo run -- --help && make test && make lint`
Expected: the configuration block shows both files; tests and lint green.

- [ ] **Step 5: Commit**

```bash
git add README.md src/main.rs
git commit -m "Document the declared copy of local files"
```
