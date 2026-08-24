use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

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
            Strategy::None => "skip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Npm,
    Pnpm,
}

impl Manager {
    pub fn label(&self) -> &'static str {
        match self {
            Manager::Npm => "npm ci",
            Manager::Pnpm => "pnpm install",
        }
    }

    pub fn command(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Manager::Npm => ("npm", &["ci"]),
            Manager::Pnpm => ("pnpm", &["install", "--frozen-lockfile"]),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub rel: PathBuf,
    pub lockfile: Option<Manager>,
}

pub fn discover_targets(source_root: &Path) -> Vec<Target> {
    let mut found = Vec::new();
    let mut stack = vec![source_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        let mut has_package_json = false;
        let mut lockfile = None;
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
            if file_type.is_file()
                && let Some(manager) = manager_for_lockfile(&name)
            {
                lockfile = Some(manager);
                continue;
            }
            if name == "node_modules" {
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
                lockfile,
            });
        }

        stack.extend(subdirs);
    }

    found.sort_by(|a, b| a.rel.cmp(&b.rel));
    found
}

fn manager_for_lockfile(name: &str) -> Option<Manager> {
    match name {
        "package-lock.json" | "npm-shrinkwrap.json" => Some(Manager::Npm),
        "pnpm-lock.yaml" => Some(Manager::Pnpm),
        _ => None,
    }
}

pub fn targets_for(strategy: Strategy, targets: &[Target]) -> Vec<&Target> {
    match strategy {
        Strategy::Install => targets.iter().filter(|t| t.lockfile.is_some()).collect(),
        Strategy::None => Vec::new(),
    }
}

/// Captures the installer's output rather than inheriting it: the TUI owns stderr.
pub fn install(manager: Manager, dir: &Path) -> Result<()> {
    let (program, args) = manager.command();
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run {program} — is it installed and on PATH?"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "{}: {}",
            manager.label(),
            error_summary(&output.stderr)
        ));
    }
    Ok(())
}

fn error_summary(stderr: &[u8]) -> String {
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
        .unwrap_or_else(|| String::from("see the installer output"))
}

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
            .then_some("no lockfile next to a package.json"),
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
        assert_eq!(app.lockfile, Some(Manager::Npm));
        assert_eq!(tools.lockfile, None);
    }

    #[test]
    fn test_discover_targets_accepts_a_shrinkwrap_as_a_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        fs::write(root.join("app/npm-shrinkwrap.json"), "{}").unwrap();

        let targets = discover_targets(root);
        assert_eq!(targets[0].lockfile, Some(Manager::Npm));
    }

    #[test]
    fn test_discover_targets_recognises_a_pnpm_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "app");
        fs::write(root.join("app/pnpm-lock.yaml"), "").unwrap();

        let targets = discover_targets(root);
        assert_eq!(targets[0].lockfile, Some(Manager::Pnpm));
    }

    #[test]
    fn test_discover_targets_keeps_each_managers_own_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(root, "api");
        write_lock(root, "api");
        write_pkg(root, "web");
        fs::write(root.join("web/pnpm-lock.yaml"), "").unwrap();

        let targets = discover_targets(root);
        let managers: Vec<Option<Manager>> = targets.iter().map(|t| t.lockfile).collect();
        assert_eq!(managers, vec![Some(Manager::Npm), Some(Manager::Pnpm)]);
    }

    #[test]
    fn test_manager_commands_are_the_lockfile_respecting_ones() {
        assert_eq!(Manager::Npm.command(), ("npm", &["ci"][..]));
        assert_eq!(
            Manager::Pnpm.command(),
            ("pnpm", &["install", "--frozen-lockfile"][..])
        );
    }

    #[test]
    fn test_targets_for_install_skips_packages_without_a_lockfile() {
        let targets = vec![
            Target {
                rel: PathBuf::new(),
                lockfile: None,
            },
            Target {
                rel: PathBuf::from("app"),
                lockfile: Some(Manager::Npm),
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
            unavailable_reason(Strategy::Install, &targets),
            Some("no lockfile next to a package.json")
        );
    }

    #[test]
    fn test_error_summary_reports_the_reason_not_the_code() {
        let stderr = b"npm error code EUSAGE\n\
                       npm error\n\
                       npm error The `npm ci` command can only install with an existing package-lock.json or\n\
                       npm error npm-shrinkwrap.json with lockfileVersion >= 1.\n";
        assert_eq!(
            error_summary(stderr),
            "The `npm ci` command can only install with an existing package-lock.json or"
        );
    }

    #[test]
    fn test_error_summary_falls_back_when_stderr_is_empty() {
        assert_eq!(error_summary(b""), "see the installer output");
    }

    #[test]
    fn test_targets_for_none_selects_nothing() {
        let targets = vec![Target {
            rel: PathBuf::from("app"),
            lockfile: Some(Manager::Npm),
        }];
        assert!(targets_for(Strategy::None, &targets).is_empty());
    }

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

    #[test]
    fn test_strategy_labels_name_the_command_that_runs() {
        assert_eq!(Strategy::Install.label(), "install");
        assert_eq!(Strategy::None.label(), "skip");
    }
}
