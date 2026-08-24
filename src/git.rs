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

pub fn parse_remote_head(out: &str, remote: &str) -> Option<String> {
    out.trim()
        .strip_prefix(&format!("refs/remotes/{remote}/"))
        .filter(|branch| !branch.is_empty())
        .map(str::to_string)
}

/// `origin` when it is there, otherwise the first remote by name — a repo
/// cloned from a single GitLab or Codeberg host is not an edge case.
pub fn pick_default_remote(out: &str) -> Option<String> {
    let mut remotes: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if remotes.contains(&"origin") {
        return Some(String::from("origin"));
    }
    remotes.sort();
    remotes.first().map(|remote| String::from(*remote))
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

pub fn extract_branch_names(out: &str, remote: &str) -> Vec<String> {
    let prefix = format!("{remote}/");
    out.lines()
        .map(|line| line.trim())
        .map(|line| line.strip_prefix(&prefix).unwrap_or(line))
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

pub fn is_dirty(worktree: &Path) -> bool {
    run_git(worktree, &["status", "--porcelain"])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false)
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

    /// The remote everything defaults to when a branch has no upstream of its own.
    pub fn default_remote(&self) -> String {
        run_git(&self.source, &["remote"])
            .ok()
            .and_then(|out| pick_default_remote(&out))
            .unwrap_or_else(|| String::from("origin"))
    }

    pub fn default_base(&self) -> String {
        let remote = self.default_remote();
        run_git(
            &self.source,
            &[
                "symbolic-ref",
                "--quiet",
                &format!("refs/remotes/{remote}/HEAD"),
            ],
        )
        .ok()
        .and_then(|out| parse_remote_head(&out, &remote))
        .unwrap_or_else(|| String::from("main"))
    }

    /// Deleting a branch on the wrong remote is the one mistake here that is
    /// visible to everyone else, so this follows the branch's own upstream.
    pub fn upstream_remote(&self, branch: &str) -> String {
        run_git(
            &self.source,
            &["config", &format!("branch.{branch}.remote")],
        )
        .map(|out| out.trim().to_string())
        .ok()
        .filter(|remote| !remote.is_empty())
        .unwrap_or_else(|| self.default_remote())
    }

    pub fn remote_branch_exists(&self, remote: &str, branch: &str) -> bool {
        run_git(
            &self.source,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/remotes/{remote}/{branch}"),
            ],
        )
        .is_ok()
    }

    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<()> {
        run_git(&self.main_clone, &["push", remote, "--delete", branch]).map(|_| ())
    }

    pub fn prefixes(&self) -> Vec<String> {
        let remote = self.default_remote();
        let out = run_git(
            &self.source,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads",
                &format!("refs/remotes/{remote}"),
            ],
        )
        .unwrap_or_default();

        prefix_options(&extract_branch_names(&out, &remote))
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

    pub fn add_worktree(&self, dir: &Path, branch: &str, base: &str) -> Result<()> {
        let dir = dir.to_string_lossy().to_string();
        run_git(&self.source, &["worktree", "add", "-b", branch, &dir, base]).map(|_| ())
    }

    /// Runs from the main clone: the worktree being removed may be the one
    /// fissa was launched from, and git needs a surviving directory to work in.
    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<()> {
        let path = path.to_string_lossy().to_string();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path);
        run_git(&self.main_clone, &args).map(|_| ())
    }

    pub fn delete_branch(&self, branch: &str, force: bool) -> Result<()> {
        let flag = if force { "-D" } else { "-d" };
        run_git(&self.main_clone, &["branch", flag, branch]).map(|_| ())
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
    fn test_pick_default_remote_prefers_origin() {
        assert_eq!(
            pick_default_remote("upstream\norigin\nfork\n").as_deref(),
            Some("origin")
        );
    }

    #[test]
    fn test_pick_default_remote_takes_the_only_remote_there_is() {
        assert_eq!(pick_default_remote("gitlab\n").as_deref(), Some("gitlab"));
    }

    #[test]
    fn test_pick_default_remote_falls_back_to_the_first_by_name() {
        assert_eq!(
            pick_default_remote("upstream\nfork\n").as_deref(),
            Some("fork")
        );
    }

    #[test]
    fn test_pick_default_remote_of_a_repo_with_no_remote_is_none() {
        assert_eq!(pick_default_remote("\n"), None);
    }

    #[test]
    fn test_parse_remote_head_extracts_the_branch_of_a_named_remote() {
        assert_eq!(
            parse_remote_head("refs/remotes/gitlab/develop\n", "gitlab").as_deref(),
            Some("develop")
        );
    }

    #[test]
    fn test_parse_remote_head_ignores_a_head_of_another_remote() {
        assert_eq!(
            parse_remote_head("refs/remotes/origin/develop\n", "fork"),
            None
        );
    }

    #[test]
    fn test_extract_branch_names_strips_a_named_remote_prefix() {
        assert_eq!(
            extract_branch_names("gitlab/develop", "gitlab"),
            vec!["develop"]
        );
    }

    #[test]
    fn test_parse_origin_head_extracts_branch() {
        assert_eq!(
            parse_remote_head("refs/remotes/origin/develop\n", "origin").as_deref(),
            Some("develop")
        );
    }

    #[test]
    fn test_parse_origin_head_unset_returns_none() {
        assert_eq!(parse_remote_head("", "origin"), None);
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
        assert_eq!(
            extract_branch_names("origin/develop", "origin"),
            vec!["develop"]
        );
    }

    #[test]
    fn test_extract_branch_names_leaves_local_branch_unchanged() {
        assert_eq!(
            extract_branch_names("feature/x", "origin"),
            vec!["feature/x"]
        );
    }

    #[test]
    fn test_extract_branch_names_drops_blank_lines() {
        assert_eq!(
            extract_branch_names("develop\n\nfeature/x\n", "origin"),
            vec!["develop", "feature/x"]
        );
    }

    #[test]
    fn test_extract_branch_names_trims_surrounding_whitespace() {
        assert_eq!(
            extract_branch_names("  develop  \n", "origin"),
            vec!["develop"]
        );
    }
}

#[cfg(test)]
mod removal_tests {
    use super::*;

    fn run(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn commit(cwd: &Path, message: &str) {
        run(
            cwd,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                message,
            ],
        );
    }

    /// A main clone on `main` with a linked `side` worktree next to it.
    fn repo_with_a_side_worktree(root: &Path) -> Repo {
        std::fs::create_dir_all(root).unwrap();
        run(root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "hi").unwrap();
        run(root, &["add", "README.md"]);
        commit(root, "init");
        run(root, &["worktree", "add", "-q", "-b", "side", "../side"]);
        Repo::discover(root).unwrap()
    }

    fn side_of(repo: &Repo) -> PathBuf {
        repo.main_clone.parent().unwrap().join("side")
    }

    #[test]
    fn test_remove_worktree_deletes_a_clean_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_side_worktree(&tmp.path().join("main"));
        let side = side_of(&repo);

        repo.remove_worktree(&side, false).unwrap();

        assert!(!side.exists());
        assert_eq!(repo.worktrees().unwrap().len(), 1);
    }

    #[test]
    fn test_remove_worktree_refuses_a_worktree_with_uncommitted_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_side_worktree(&tmp.path().join("main"));
        let side = side_of(&repo);
        std::fs::write(side.join("scratch.txt"), "untracked").unwrap();

        assert!(repo.remove_worktree(&side, false).is_err());
        assert!(side.exists(), "nothing uncommitted may be destroyed");
    }

    #[test]
    fn test_remove_worktree_forces_past_uncommitted_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_side_worktree(&tmp.path().join("main"));
        let side = side_of(&repo);
        std::fs::write(side.join("scratch.txt"), "untracked").unwrap();

        repo.remove_worktree(&side, true).unwrap();

        assert!(!side.exists());
    }

    #[test]
    fn test_remove_worktree_works_from_the_worktree_being_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        repo_with_a_side_worktree(&root);
        let side = root.parent().unwrap().join("side");
        // fissa is often launched from inside the worktree the user deletes.
        let repo = Repo::discover(&side).unwrap();

        repo.remove_worktree(&side, false).unwrap();

        assert!(!side.exists());
    }

    #[test]
    fn test_delete_branch_deletes_a_merged_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_side_worktree(&tmp.path().join("main"));
        repo.remove_worktree(&side_of(&repo), false).unwrap();

        repo.delete_branch("side", false).unwrap();

        assert!(!repo.branch_exists("side"));
    }

    #[test]
    fn test_delete_branch_refuses_an_unmerged_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_side_worktree(&tmp.path().join("main"));
        let side = side_of(&repo);
        std::fs::write(side.join("work.txt"), "work").unwrap();
        run(&side, &["add", "work.txt"]);
        commit(&side, "unmerged work");
        repo.remove_worktree(&side, false).unwrap();

        assert!(repo.delete_branch("side", false).is_err());
        assert!(repo.branch_exists("side"), "unmerged work may not be lost");
    }

    #[test]
    fn test_delete_branch_forces_past_an_unmerged_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_side_worktree(&tmp.path().join("main"));
        let side = side_of(&repo);
        std::fs::write(side.join("work.txt"), "work").unwrap();
        run(&side, &["add", "work.txt"]);
        commit(&side, "unmerged work");
        repo.remove_worktree(&side, false).unwrap();

        repo.delete_branch("side", true).unwrap();

        assert!(!repo.branch_exists("side"));
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;

    fn run(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// A clone with `gitlab` as its only remote, plus a pushed `side` branch.
    fn repo_with_a_named_remote(tmp: &Path) -> Repo {
        let remote = tmp.join("remote.git");
        run(tmp, &["init", "-q", "--bare", remote.to_str().unwrap()]);

        let root = tmp.join("main");
        std::fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "hi").unwrap();
        run(&root, &["add", "README.md"]);
        run(
            &root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        );
        run(
            &root,
            &["remote", "add", "gitlab", remote.to_str().unwrap()],
        );
        run(&root, &["push", "-q", "-u", "gitlab", "main"]);
        run(&root, &["branch", "side"]);
        run(&root, &["push", "-q", "-u", "gitlab", "side"]);
        Repo::discover(&root).unwrap()
    }

    #[test]
    fn test_default_remote_is_the_only_remote_a_repo_has() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_named_remote(tmp.path());
        assert_eq!(repo.default_remote(), "gitlab");
    }

    #[test]
    fn test_prefixes_sees_the_branches_of_a_remote_not_called_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = repo_with_a_named_remote(tmp.path());
        run(&root, &["branch", "feature/x"]);
        run(&root, &["push", "-q", "gitlab", "feature/x"]);

        assert!(
            repo.prefixes().contains(&String::from("feature/")),
            "{:?}",
            repo.prefixes()
        );
    }

    #[test]
    fn test_default_base_reads_the_head_of_a_remote_not_called_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = repo_with_a_named_remote(tmp.path());
        run(&root, &["remote", "set-head", "gitlab", "main"]);

        assert_eq!(repo.default_base(), "main");
    }

    #[test]
    fn test_upstream_remote_of_a_tracking_branch_is_the_remote_it_tracks() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_named_remote(tmp.path());
        assert_eq!(repo.upstream_remote("side"), "gitlab");
    }

    #[test]
    fn test_upstream_remote_of_a_local_only_branch_falls_back_to_the_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = repo_with_a_named_remote(tmp.path());
        run(&root, &["branch", "local-only"]);
        assert_eq!(repo.upstream_remote("local-only"), "gitlab");
    }

    #[test]
    fn test_remote_branch_exists_for_a_pushed_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_named_remote(tmp.path());
        assert!(repo.remote_branch_exists("gitlab", "side"));
    }

    #[test]
    fn test_remote_branch_does_not_exist_for_a_branch_never_pushed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = repo_with_a_named_remote(tmp.path());
        run(&root, &["branch", "local-only"]);
        assert!(!repo.remote_branch_exists("gitlab", "local-only"));
    }

    #[test]
    fn test_delete_remote_branch_removes_it_from_the_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_named_remote(tmp.path());

        repo.delete_remote_branch("gitlab", "side").unwrap();

        let out = run_git(&repo.main_clone, &["ls-remote", "--heads", "gitlab"]).unwrap();
        assert!(!out.contains("refs/heads/side"), "{out}");
        assert!(out.contains("refs/heads/main"), "{out}");
    }

    #[test]
    fn test_delete_remote_branch_reports_a_branch_that_is_not_there() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_a_named_remote(tmp.path());
        assert!(repo.delete_remote_branch("gitlab", "never-pushed").is_err());
    }
}
