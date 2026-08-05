use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Hardlink,
    Symlink,
    Install,
    None,
}

impl Strategy {
    pub const ALL: [Strategy; 4] = [
        Strategy::Hardlink,
        Strategy::Symlink,
        Strategy::Install,
        Strategy::None,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Strategy::Hardlink => "hardlink",
            Strategy::Symlink => "symlink",
            Strategy::Install => "install",
            Strategy::None => "none",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub rel: PathBuf,
    pub has_source_modules: bool,
    pub has_lockfile: bool,
}

pub fn discover_targets(source_root: &Path) -> Vec<Target> {
    let mut found = Vec::new();
    let mut stack = vec![source_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        let mut has_package_json = false;
        let mut has_lockfile = false;
        let mut has_modules = false;
        let mut subdirs = Vec::new();

        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if name == "package.json" && file_type.is_file() {
                has_package_json = true;
                continue;
            }
            if (name == "package-lock.json" || name == "npm-shrinkwrap.json") && file_type.is_file()
            {
                has_lockfile = true;
                continue;
            }
            if name == "node_modules" {
                has_modules = file_type.is_dir();
                continue;
            }
            if name.starts_with('.') {
                continue;
            }
            if file_type.is_dir() {
                subdirs.push(entry.path());
            }
        }

        if has_package_json && let Ok(rel) = dir.strip_prefix(source_root) {
            found.push(Target {
                rel: rel.to_path_buf(),
                has_source_modules: has_modules,
                has_lockfile,
            });
        }

        stack.extend(subdirs);
    }

    found.sort_by(|a, b| a.rel.cmp(&b.rel));
    found
}

pub fn targets_for(strategy: Strategy, targets: &[Target]) -> Vec<&Target> {
    match strategy {
        Strategy::Hardlink | Strategy::Symlink => {
            targets.iter().filter(|t| t.has_source_modules).collect()
        }
        Strategy::Install => targets.iter().filter(|t| t.has_lockfile).collect(),
        Strategy::None => Vec::new(),
    }
}

pub fn same_filesystem(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(a)?.dev() == std::fs::metadata(b)?.dev())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmState {
    Own,
    Link,
    Missing,
}

impl NmState {
    pub fn label(&self) -> &'static str {
        match self {
            NmState::Own => "own",
            NmState::Link => "link",
            NmState::Missing => "—",
        }
    }
}

pub fn nm_state(worktree_root: &Path, targets: &[Target]) -> NmState {
    let mut state = NmState::Missing;
    for target in targets {
        let modules = worktree_root.join(&target.rel).join("node_modules");
        let Ok(metadata) = std::fs::symlink_metadata(&modules) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            return NmState::Link;
        }
        if metadata.file_type().is_dir() {
            state = NmState::Own;
        }
    }
    state
}

pub fn package_count(modules: &Path) -> usize {
    std::fs::read_dir(modules)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

pub fn hardlink_modules(src: &Path, dst: &Path) -> Result<()> {
    let status = Command::new("cp")
        .arg("-al")
        .arg(src)
        .arg(dst)
        .status()
        .context("failed to run cp — is coreutils installed?")?;

    if !status.success() {
        return Err(anyhow!("cp -al {src:?} {dst:?} failed"));
    }
    Ok(())
}

pub fn symlink_modules(src: &Path, dst: &Path) -> Result<()> {
    std::os::unix::fs::symlink(src, dst)
        .with_context(|| format!("failed to symlink {dst:?} -> {src:?}"))
}

/// Captures npm's output rather than inheriting it: the TUI owns stderr.
pub fn npm_ci(dir: &Path) -> Result<()> {
    let output = Command::new("npm")
        .arg("ci")
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .context("failed to run npm — is it installed and on PATH?")?;

    if !output.status.success() {
        return Err(anyhow!("npm ci: {}", npm_error_summary(&output.stderr)));
    }
    Ok(())
}

fn npm_error_summary(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter_map(|line| {
            let line = line
                .trim_start()
                .strip_prefix("npm error")
                .or_else(|| line.trim_start().strip_prefix("npm ERR!"))
                .unwrap_or(line)
                .trim();
            (!line.is_empty() && !line.starts_with("code ")).then(|| line.to_string())
        })
        .next()
        .unwrap_or_else(|| String::from("see npm output"))
}

pub fn available_strategies(
    source_root: &Path,
    dest_parent: &Path,
    targets: &[Target],
) -> Vec<Strategy> {
    Strategy::ALL
        .into_iter()
        .filter(|strategy| {
            unavailable_reason(*strategy, source_root, dest_parent, targets).is_none()
        })
        .collect()
}

pub fn unavailable_reason(
    strategy: Strategy,
    source_root: &Path,
    dest_parent: &Path,
    targets: &[Target],
) -> Option<&'static str> {
    match strategy {
        Strategy::Hardlink => {
            if targets_for(strategy, targets).is_empty() {
                return Some("no node_modules in the source worktree");
            }
            match same_filesystem(source_root, dest_parent) {
                Ok(true) => None,
                Ok(false) => Some("source and destination are on different filesystems"),
                Err(_) => Some("could not compare filesystems"),
            }
        }
        Strategy::Symlink => {
            if targets_for(strategy, targets).is_empty() {
                return Some("no node_modules in the source worktree");
            }
            None
        }
        Strategy::Install => {
            if targets_for(strategy, targets).is_empty() {
                return Some("no package-lock.json next to a package.json");
            }
            None
        }
        Strategy::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_pkg(root: &Path, rel: &str) {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), "{}").unwrap();
    }

    fn write_modules(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel).join("node_modules")).unwrap();
    }

    fn write_lock(root: &Path, rel: &str) {
        fs::write(root.join(rel).join("package-lock.json"), "{}").unwrap();
    }

    #[test]
    fn test_discover_targets_finds_nested_packages_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, ".");
        write_pkg(root, "app");
        write_modules(root, "app");
        write_pkg(root, "app/e2e");
        write_modules(root, "app/e2e");

        let targets = discover_targets(root);
        let rels: Vec<String> = targets
            .iter()
            .map(|t| t.rel.to_string_lossy().to_string())
            .collect();
        assert_eq!(rels, vec!["", "app", "app/e2e"]);
    }

    #[test]
    fn test_discover_targets_flags_missing_source_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_modules(root, "app");
        write_pkg(root, "tools");

        let targets = discover_targets(root);
        let app = targets.iter().find(|t| t.rel == Path::new("app")).unwrap();
        let tools = targets
            .iter()
            .find(|t| t.rel == Path::new("tools"))
            .unwrap();
        assert!(app.has_source_modules);
        assert!(!tools.has_source_modules);
    }

    #[test]
    fn test_discover_targets_does_not_descend_into_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_modules(root, "app");
        write_pkg(root, "app/node_modules/some-dep");

        let targets = discover_targets(root);
        let rels: Vec<String> = targets
            .iter()
            .map(|t| t.rel.to_string_lossy().to_string())
            .collect();
        assert_eq!(rels, vec!["app"]);
    }

    #[test]
    fn test_discover_targets_skips_dot_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_pkg(root, ".cache/thing");

        let targets = discover_targets(root);
        let rels: Vec<String> = targets
            .iter()
            .map(|t| t.rel.to_string_lossy().to_string())
            .collect();
        assert_eq!(rels, vec!["app"]);
    }

    #[test]
    fn test_discover_targets_symlinked_modules_are_not_source_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        fs::create_dir_all(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("app/node_modules")).unwrap();

        let targets = discover_targets(root);
        assert_eq!(targets.len(), 1);
        assert!(!targets[0].has_source_modules);
    }

    #[test]
    fn test_targets_for_hardlink_requires_source_modules() {
        let targets = vec![
            Target {
                rel: PathBuf::from("app"),
                has_source_modules: true,
                has_lockfile: true,
            },
            Target {
                rel: PathBuf::from("tools"),
                has_source_modules: false,
                has_lockfile: true,
            },
        ];
        let selected = targets_for(Strategy::Hardlink, &targets);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].rel, PathBuf::from("app"));
    }

    #[test]
    fn test_targets_for_install_includes_packages_without_modules() {
        let targets = vec![
            Target {
                rel: PathBuf::from("app"),
                has_source_modules: true,
                has_lockfile: true,
            },
            Target {
                rel: PathBuf::from("tools"),
                has_source_modules: false,
                has_lockfile: true,
            },
        ];
        assert_eq!(targets_for(Strategy::Install, &targets).len(), 2);
    }

    #[test]
    fn test_discover_targets_flags_the_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_lock(root, "app");
        write_pkg(root, "tools");

        let targets = discover_targets(root);
        let app = targets.iter().find(|t| t.rel == Path::new("app")).unwrap();
        let tools = targets
            .iter()
            .find(|t| t.rel == Path::new("tools"))
            .unwrap();
        assert!(app.has_lockfile);
        assert!(!tools.has_lockfile);
    }

    #[test]
    fn test_discover_targets_accepts_a_shrinkwrap_as_a_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        fs::write(root.join("app/npm-shrinkwrap.json"), "{}").unwrap();

        let targets = discover_targets(root);
        assert!(targets[0].has_lockfile);
    }

    #[test]
    fn test_targets_for_install_skips_packages_without_a_lockfile() {
        let targets = vec![
            Target {
                rel: PathBuf::new(),
                has_source_modules: false,
                has_lockfile: false,
            },
            Target {
                rel: PathBuf::from("app"),
                has_source_modules: false,
                has_lockfile: true,
            },
        ];
        let selected = targets_for(Strategy::Install, &targets);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].rel, PathBuf::from("app"));
    }

    /// `npm ci` cannot run on a marker root, and used to fail creation there before
    /// reaching the packages that can install.
    #[test]
    fn test_targets_for_install_on_a_marker_root_over_locked_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, ".");
        write_pkg(root, "app");
        write_lock(root, "app");
        write_pkg(root, "app/e2e");
        write_lock(root, "app/e2e");
        write_pkg(root, "app/target/dist/vendored");

        let targets = discover_targets(root);
        let rels: Vec<String> = targets_for(Strategy::Install, &targets)
            .iter()
            .map(|t| t.rel.to_string_lossy().to_string())
            .collect();
        assert_eq!(rels, vec!["app", "app/e2e"]);
    }

    #[test]
    fn test_unavailable_reason_install_needs_a_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, ".");

        let targets = discover_targets(root);
        assert_eq!(
            unavailable_reason(Strategy::Install, root, root, &targets),
            Some("no package-lock.json next to a package.json")
        );
    }

    #[test]
    fn test_npm_error_summary_reports_the_reason_not_the_code() {
        let stderr = b"npm error code EUSAGE\n\
                       npm error\n\
                       npm error The `npm ci` command can only install with an existing package-lock.json or\n\
                       npm error npm-shrinkwrap.json with lockfileVersion >= 1.\n";
        assert_eq!(
            npm_error_summary(stderr),
            "The `npm ci` command can only install with an existing package-lock.json or"
        );
    }

    #[test]
    fn test_npm_error_summary_falls_back_when_stderr_is_empty() {
        assert_eq!(npm_error_summary(b""), "see npm output");
    }

    #[test]
    fn test_targets_for_none_selects_nothing() {
        let targets = vec![Target {
            rel: PathBuf::from("app"),
            has_source_modules: true,
            has_lockfile: true,
        }];
        assert!(targets_for(Strategy::None, &targets).is_empty());
    }

    #[test]
    fn test_same_filesystem_within_one_tempdir_is_true() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert!(same_filesystem(&a, &b).unwrap());
    }

    #[test]
    fn test_nm_state_real_directory_is_own() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_modules(root, "app");
        let targets = discover_targets(root);
        assert_eq!(nm_state(root, &targets), NmState::Own);
    }

    #[test]
    fn test_nm_state_symlink_is_link() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("app/node_modules")).unwrap();
        let targets = discover_targets(root);
        assert_eq!(nm_state(root, &targets), NmState::Link);
    }

    #[test]
    fn test_nm_state_absent_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        let targets = discover_targets(root);
        assert_eq!(nm_state(root, &targets), NmState::Missing);
    }

    #[test]
    fn test_package_count_counts_top_level_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let modules = tmp.path().join("node_modules");
        std::fs::create_dir_all(modules.join("a")).unwrap();
        std::fs::create_dir_all(modules.join("b")).unwrap();
        assert_eq!(package_count(&modules), 2);
    }

    #[test]
    fn test_hardlink_modules_shares_inodes_and_separates_trees() {
        use std::os::unix::fs::MetadataExt;
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/node_modules");
        fs::create_dir_all(src.join("dep")).unwrap();
        fs::write(src.join("dep/index.js"), "module.exports = 1;").unwrap();
        let dst = tmp.path().join("dst/node_modules");
        fs::create_dir_all(tmp.path().join("dst")).unwrap();

        hardlink_modules(&src, &dst).unwrap();

        let a = fs::metadata(src.join("dep/index.js")).unwrap();
        let b = fs::metadata(dst.join("dep/index.js")).unwrap();
        assert_eq!(a.ino(), b.ino());

        fs::remove_dir_all(dst.join("dep")).unwrap();
        assert!(src.join("dep/index.js").exists());
    }

    #[test]
    fn test_hardlink_modules_preserves_symlinks_as_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/node_modules");
        fs::create_dir_all(src.join(".bin")).unwrap();
        fs::write(src.join("real.js"), "x").unwrap();
        std::os::unix::fs::symlink("../real.js", src.join(".bin/tool")).unwrap();
        let dst = tmp.path().join("dst/node_modules");
        fs::create_dir_all(tmp.path().join("dst")).unwrap();

        hardlink_modules(&src, &dst).unwrap();

        let metadata = fs::symlink_metadata(dst.join(".bin/tool")).unwrap();
        assert!(metadata.file_type().is_symlink());
    }

    #[test]
    fn test_symlink_modules_creates_a_symlink_to_the_source() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/node_modules");
        fs::create_dir_all(&src).unwrap();
        let dst = tmp.path().join("dst/node_modules");
        fs::create_dir_all(tmp.path().join("dst")).unwrap();

        symlink_modules(&src, &dst).unwrap();

        let metadata = fs::symlink_metadata(&dst).unwrap();
        assert!(metadata.file_type().is_symlink());
        assert_eq!(fs::read_link(&dst).unwrap(), src);
    }

    #[test]
    fn test_available_strategies_include_hardlink_on_one_filesystem() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_modules(root, "app");
        write_lock(root, "app");
        let targets = discover_targets(root);
        let available = available_strategies(root, root, &targets);
        assert!(available.contains(&Strategy::Hardlink));
        assert!(available.contains(&Strategy::Install));
        assert!(available.contains(&Strategy::None));
    }

    #[test]
    fn test_available_strategies_exclude_sharing_without_source_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_lock(root, "app");
        let targets = discover_targets(root);
        let available = available_strategies(root, root, &targets);
        assert!(!available.contains(&Strategy::Hardlink));
        assert!(!available.contains(&Strategy::Symlink));
        assert!(available.contains(&Strategy::Install));
    }

    #[test]
    fn test_unavailable_reason_names_the_missing_source_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        let targets = discover_targets(root);
        assert_eq!(
            unavailable_reason(Strategy::Symlink, root, root, &targets),
            Some("no node_modules in the source worktree")
        );
    }

    #[test]
    fn test_unavailable_reason_is_none_when_available() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        write_modules(root, "app");
        let targets = discover_targets(root);
        assert_eq!(
            unavailable_reason(Strategy::Hardlink, root, root, &targets),
            None
        );
    }
}
