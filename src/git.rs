use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

#[derive(Debug, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
}

pub fn parse_worktree_list(out: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            flush(&mut current, &mut entries);
            continue;
        }

        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, Some(v)),
            None => (line, None),
        };

        match (key, value) {
            ("worktree", Some(path)) => {
                flush(&mut current, &mut entries);
                current = Some(WorktreeEntry {
                    path: PathBuf::from(path),
                    head: None,
                    branch: None,
                    bare: false,
                    detached: false,
                });
            }
            ("HEAD", Some(head)) => {
                if let Some(entry) = current.as_mut() {
                    entry.head = Some(head.to_string());
                }
            }
            ("branch", Some(refname)) => {
                if let Some(entry) = current.as_mut() {
                    entry.branch = Some(short_branch(refname));
                }
            }
            ("bare", _) => {
                if let Some(entry) = current.as_mut() {
                    entry.bare = true;
                }
            }
            ("detached", _) => {
                if let Some(entry) = current.as_mut() {
                    entry.detached = true;
                }
            }
            _ => {}
        }
    }

    flush(&mut current, &mut entries);
    entries
}

fn flush(current: &mut Option<WorktreeEntry>, entries: &mut Vec<WorktreeEntry>) {
    if let Some(entry) = current.take() {
        entries.push(entry);
    }
}

pub fn short_branch(refname: &str) -> String {
    refname
        .strip_prefix("refs/heads/")
        .unwrap_or(refname)
        .to_string()
}

pub fn parse_origin_head(out: &str) -> Option<String> {
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .strip_prefix("refs/remotes/origin/")
        .map(str::to_string)
}

pub fn prefix_options(branches: &[String]) -> Vec<String> {
    let mut prefixes: Vec<String> = Vec::new();
    for branch in branches {
        if let Some((head, _)) = branch.split_once('/') {
            let prefix = format!("{head}/");
            if !prefixes.contains(&prefix) {
                prefixes.push(prefix);
            }
        }
    }
    prefixes.sort();

    if let Some(index) = prefixes.iter().position(|p| p == "feature/") {
        let feature = prefixes.remove(index);
        prefixes.insert(0, feature);
    }

    prefixes.push(String::new());
    prefixes
}

pub fn extract_branch_names(out: &str) -> Vec<String> {
    out.lines()
        .map(|line| line.trim().trim_start_matches("origin/"))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn row_label(path: &Path, main_clone: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(main_clone) {
        if relative.as_os_str().is_empty() {
            return file_name_or_path(path);
        }
        return relative.to_string_lossy().to_string();
    }
    file_name_or_path(path)
}

fn file_name_or_path(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => path.to_string_lossy().to_string(),
    }
}

pub struct Repo {
    pub main_clone: PathBuf,
    pub source: PathBuf,
}

impl Repo {
    pub fn discover(cwd: &Path) -> Result<Repo> {
        let common_dir = run_git(
            cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .context("not inside a git repository")?;
        let common_dir = PathBuf::from(common_dir.trim());
        let main_clone = common_dir
            .parent()
            .ok_or_else(|| anyhow!("could not resolve the main clone from {common_dir:?}"))?
            .to_path_buf();

        let source = run_git(
            cwd,
            &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        )
        .context("not inside a git worktree")?;

        Ok(Repo {
            main_clone,
            source: PathBuf::from(source.trim()),
        })
    }

    pub fn main_dir_name(&self) -> String {
        match self.main_clone.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => String::from("repo"),
        }
    }

    pub fn worktrees(&self) -> Result<Vec<WorktreeEntry>> {
        let out = run_git(&self.source, &["worktree", "list", "--porcelain"])?;
        Ok(parse_worktree_list(&out))
    }

    pub fn default_base(&self) -> String {
        run_git(
            &self.source,
            &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
        )
        .ok()
        .and_then(|out| parse_origin_head(&out))
        .unwrap_or_else(|| String::from("main"))
    }

    pub fn prefixes(&self) -> Vec<String> {
        let out = run_git(
            &self.source,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads",
                "refs/remotes/origin",
            ],
        )
        .unwrap_or_default();

        prefix_options(&extract_branch_names(&out))
    }

    pub fn branch_exists(&self, branch: &str) -> bool {
        run_git(
            &self.source,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_ok()
    }

    pub fn ref_exists(&self, reference: &str) -> bool {
        run_git(
            &self.source,
            &["rev-parse", "--verify", "--quiet", reference],
        )
        .is_ok()
    }

    pub fn is_dirty(&self, worktree: &Path) -> bool {
        run_git(worktree, &["status", "--porcelain"])
            .map(|out| !out.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn add_worktree(&self, dir: &Path, branch: &str, base: &str) -> Result<()> {
        let dir = dir.to_string_lossy().to_string();
        run_git(&self.source, &["worktree", "add", "-b", branch, &dir, base]).map(|_| ())
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to run git — is it installed and on PATH?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {} failed: {stderr}", args.join(" ")));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = "\
worktree /home/u/work/spectra
HEAD 06b7b933140338915947ef49aadda0aaae1b99ad
branch refs/heads/develop

worktree /home/u/work/spectra-ter
HEAD 3be5ed573ed3e76ccc0d949cc829dbb91743ffa5
branch refs/heads/ter

worktree /home/u/work/spectra/.claude/worktrees/agent-a
HEAD 06b7b933140338915947ef49aadda0aaae1b99ad
branch refs/heads/refactor/agent-a
";

    #[test]
    fn test_parse_worktree_list_returns_one_entry_per_record() {
        let entries = parse_worktree_list(PORCELAIN);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, PathBuf::from("/home/u/work/spectra"));
        assert_eq!(entries[0].branch.as_deref(), Some("develop"));
        assert_eq!(entries[2].branch.as_deref(), Some("refactor/agent-a"));
    }

    #[test]
    fn test_parse_worktree_list_marks_bare_and_detached() {
        let out = "\
worktree /repo/bare
bare

worktree /repo/loose
HEAD deadbeef
detached
";
        let entries = parse_worktree_list(out);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].bare);
        assert!(!entries[0].detached);
        assert!(entries[1].detached);
        assert_eq!(entries[1].branch, None);
    }

    #[test]
    fn test_parse_worktree_list_ignores_unknown_attributes() {
        let out = "\
worktree /repo/a
HEAD abc
branch refs/heads/main
locked
prunable gitdir file points to non-existent location
";
        let entries = parse_worktree_list(out);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_parse_worktree_list_empty_input_returns_empty() {
        assert!(parse_worktree_list("").is_empty());
    }

    #[test]
    fn test_short_branch_strips_refs_heads() {
        assert_eq!(short_branch("refs/heads/feature/x"), "feature/x");
        assert_eq!(short_branch("already-short"), "already-short");
    }

    #[test]
    fn test_parse_origin_head_extracts_branch() {
        assert_eq!(
            parse_origin_head("refs/remotes/origin/develop\n").as_deref(),
            Some("develop")
        );
    }

    #[test]
    fn test_parse_origin_head_unset_returns_none() {
        assert_eq!(parse_origin_head(""), None);
    }

    #[test]
    fn test_prefix_options_puts_feature_first_then_sorted_then_none() {
        let branches = vec![
            "chore/a".to_string(),
            "feature/b".to_string(),
            "bugfix/c".to_string(),
            "no-prefix".to_string(),
        ];
        assert_eq!(
            prefix_options(&branches),
            vec!["feature/", "bugfix/", "chore/", ""]
        );
    }

    #[test]
    fn test_prefix_options_without_feature_still_appends_none() {
        let branches = vec!["chore/a".to_string()];
        assert_eq!(prefix_options(&branches), vec!["chore/", ""]);
    }

    #[test]
    fn test_prefix_options_deduplicates() {
        let branches = vec!["fix/a".to_string(), "fix/b".to_string()];
        assert_eq!(prefix_options(&branches), vec!["fix/", ""]);
    }

    #[test]
    fn test_row_label_sibling_of_main_clone_uses_dir_name() {
        let label = row_label(
            Path::new("/home/u/work/spectra-ter"),
            Path::new("/home/u/work/spectra"),
        );
        assert_eq!(label, "spectra-ter");
    }

    #[test]
    fn test_row_label_main_clone_itself_uses_dir_name() {
        let label = row_label(
            Path::new("/home/u/work/spectra"),
            Path::new("/home/u/work/spectra"),
        );
        assert_eq!(label, "spectra");
    }

    #[test]
    fn test_row_label_nested_worktree_uses_relative_path() {
        let label = row_label(
            Path::new("/home/u/work/spectra/.claude/worktrees/agent-a"),
            Path::new("/home/u/work/spectra"),
        );
        assert_eq!(label, ".claude/worktrees/agent-a");
    }

    #[test]
    fn test_discover_outside_a_repo_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = Repo::discover(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_branch_names_strips_origin_prefix() {
        assert_eq!(extract_branch_names("origin/develop"), vec!["develop"]);
    }

    #[test]
    fn test_extract_branch_names_leaves_local_branch_unchanged() {
        assert_eq!(extract_branch_names("feature/x"), vec!["feature/x"]);
    }

    #[test]
    fn test_extract_branch_names_drops_blank_lines() {
        assert_eq!(
            extract_branch_names("develop\n\nfeature/x\n"),
            vec!["develop", "feature/x"]
        );
    }

    #[test]
    fn test_extract_branch_names_trims_surrounding_whitespace() {
        assert_eq!(extract_branch_names("  develop  \n"), vec!["develop"]);
    }
}
