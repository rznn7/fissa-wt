use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::git;

pub struct Report {
    pub path: PathBuf,
    pub dirty: bool,
}

/// One `git status` per worktree is the whole cost of the list, so it runs off
/// the main thread and the receiver closes once every worktree has reported.
pub fn spawn(paths: Vec<PathBuf>) -> Receiver<Report> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || scan(&paths, &sender));
    receiver
}

fn scan(paths: &[PathBuf], sender: &Sender<Report>) {
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .min(paths.len());
    if workers == 0 {
        return;
    }

    std::thread::scope(|scope| {
        for slice in paths.chunks(paths.len().div_ceil(workers)) {
            let sender = sender.clone();
            scope.spawn(move || {
                for path in slice {
                    let report = Report {
                        path: path.clone(),
                        dirty: git::is_dirty(path),
                    };
                    // A quit drops the receiver; stop rather than finish the scan.
                    if sender.send(report).is_err() {
                        return;
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cwd: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn committed_repo(root: &std::path::Path) {
        std::fs::create_dir_all(root).unwrap();
        run(root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "hi").unwrap();
        run(root, &["add", "README.md"]);
        run(
            root,
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
    }

    #[test]
    fn test_scan_reports_every_worktree_it_was_given() {
        let tmp = tempfile::tempdir().unwrap();
        let clean = tmp.path().join("clean");
        let messy = tmp.path().join("messy");
        committed_repo(&clean);
        committed_repo(&messy);
        std::fs::write(messy.join("scratch.txt"), "untracked").unwrap();

        let reports: Vec<Report> = spawn(vec![clean.clone(), messy.clone()]).iter().collect();

        assert_eq!(reports.len(), 2);
        let dirty_of = |path: &std::path::Path| {
            reports
                .iter()
                .find(|report| report.path == path)
                .map(|report| report.dirty)
        };
        assert_eq!(dirty_of(&clean), Some(false));
        assert_eq!(dirty_of(&messy), Some(true));
    }

    #[test]
    fn test_scan_of_nothing_closes_immediately() {
        let reports: Vec<Report> = spawn(vec![]).iter().collect();
        assert!(reports.is_empty());
    }
}
