use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::components::progress::Progress;
use crate::git::Repo;

/// A worktree the user marked for deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub label: String,
    /// `None` for a detached worktree, which has no branch to delete.
    pub branch: Option<String>,
    /// The remote holding a copy of this branch, if one was ever pushed.
    pub remote: Option<String>,
    pub path: PathBuf,
}

/// How far past the worktree itself a deletion reaches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scope {
    pub branch: bool,
    pub remote: bool,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    RemoveWorktree { path: PathBuf, force: bool },
    DeleteBranch { branch: String, force: bool },
    DeleteRemoteBranch { remote: String, branch: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub label: String,
    pub action: Action,
}

/// One worktree's removal and, optionally, the deletion of its branch. The
/// steps inside a chain depend on each other; chains do not depend on anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub steps: Vec<Step>,
}

pub fn plan_chains(targets: &[Target], scope: Scope) -> Vec<Chain> {
    targets
        .iter()
        .map(|target| {
            let remove = if scope.force {
                "git worktree remove --force"
            } else {
                "git worktree remove"
            };
            let mut steps = vec![Step {
                label: format!("{remove}  {}", target.label),
                action: Action::RemoveWorktree {
                    path: target.path.clone(),
                    force: scope.force,
                },
            }];

            if let Some(branch) = target.branch.as_ref().filter(|_| scope.branch) {
                let flag = if scope.force { "-D" } else { "-d" };
                steps.push(Step {
                    label: format!("git branch {flag}  {branch}"),
                    action: Action::DeleteBranch {
                        branch: branch.clone(),
                        force: scope.force,
                    },
                });
            }

            if let (true, Some(branch), Some(remote)) =
                (scope.remote, target.branch.as_ref(), target.remote.as_ref())
            {
                steps.push(Step {
                    label: format!("git push {remote} --delete  {branch}"),
                    action: Action::DeleteRemoteBranch {
                        remote: remote.clone(),
                        branch: branch.clone(),
                    },
                });
            }

            Chain { steps }
        })
        .collect()
}

/// The progress screen numbers steps flatly, so chains are laid out in order.
pub fn labels(chains: &[Chain]) -> Vec<String> {
    chains
        .iter()
        .flat_map(|chain| chain.steps.iter().map(|step| step.label.clone()))
        .collect()
}

/// Removing a worktree is mostly `rm -rf`, so the chains overlap; one that
/// refuses to go never holds up or cancels its neighbours.
pub fn spawn(main_clone: PathBuf, chains: Vec<Chain>, tx: Sender<Progress>) {
    std::thread::spawn(move || {
        run_chains(&chains, &tx, |step| run_step(&main_clone, step));
        let _ = tx.send(Progress::Finished);
    });
}

fn run_chains<F>(chains: &[Chain], tx: &Sender<Progress>, run: F)
where
    F: Fn(&Step) -> anyhow::Result<String> + Send + Sync,
{
    let mut bases = Vec::with_capacity(chains.len());
    let mut next = 0;
    for chain in chains {
        bases.push(next);
        next += chain.steps.len();
    }

    std::thread::scope(|scope| {
        for (chain, base) in chains.iter().zip(bases) {
            let tx = tx.clone();
            let run = &run;
            scope.spawn(move || run_chain(chain, base, &tx, run));
        }
    });
}

fn run_chain<F>(chain: &Chain, base: usize, tx: &Sender<Progress>, run: &F)
where
    F: Fn(&Step) -> anyhow::Result<String>,
{
    for (offset, step) in chain.steps.iter().enumerate() {
        let index = base + offset;
        let _ = tx.send(Progress::Running(index));
        match run(step) {
            Ok(detail) => {
                let _ = tx.send(Progress::Ok(index, detail));
            }
            Err(error) => {
                let _ = tx.send(Progress::Failed(index, error.to_string()));
                // The rest of the chain only made sense once this step landed.
                for skipped in index + 1..base + chain.steps.len() {
                    let _ = tx.send(Progress::Ok(
                        skipped,
                        String::from("skipped (an earlier step failed)"),
                    ));
                }
                return;
            }
        }
    }
}

fn run_step(main_clone: &std::path::Path, step: &Step) -> anyhow::Result<String> {
    let repo = Repo::discover(main_clone)?;
    match &step.action {
        Action::RemoveWorktree { path, force } => {
            repo.remove_worktree(path, *force)?;
            Ok(String::from("removed"))
        }
        Action::DeleteBranch { branch, force } => {
            repo.delete_branch(branch, *force)?;
            Ok(String::from("deleted"))
        }
        Action::DeleteRemoteBranch { remote, branch } => {
            repo.delete_remote_branch(remote, branch)?;
            Ok(String::from("deleted"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(label: &str, branch: Option<&str>) -> Target {
        Target {
            label: String::from(label),
            branch: branch.map(String::from),
            remote: None,
            path: PathBuf::from(format!("/w/{label}")),
        }
    }

    fn tracked(label: &str, branch: &str, remote: &str) -> Target {
        Target {
            remote: Some(String::from(remote)),
            ..target(label, Some(branch))
        }
    }

    const NOTHING_ELSE: Scope = Scope {
        branch: false,
        remote: false,
        force: false,
    };
    const AND_BRANCH: Scope = Scope {
        branch: true,
        remote: false,
        force: false,
    };

    #[test]
    fn test_plan_chains_gives_each_worktree_its_own_chain() {
        let chains = plan_chains(
            &[target("side", Some("side")), target("other", None)],
            NOTHING_ELSE,
        );
        assert_eq!(chains.len(), 2);
        assert_eq!(
            labels(&chains),
            vec!["git worktree remove  side", "git worktree remove  other"]
        );
    }

    #[test]
    fn test_plan_chains_leaves_branches_alone_by_default() {
        let chains = plan_chains(&[target("side", Some("feature/side"))], NOTHING_ELSE);
        assert_eq!(labels(&chains).len(), 1);
    }

    #[test]
    fn test_a_chain_deletes_the_branch_after_its_own_worktree() {
        let chains = plan_chains(&[target("side", Some("feature/side"))], AND_BRANCH);
        assert_eq!(
            chains.len(),
            1,
            "the branch belongs to its worktree's chain"
        );
        assert_eq!(
            labels(&chains),
            vec!["git worktree remove  side", "git branch -d  feature/side"]
        );
    }

    #[test]
    fn test_plan_chains_has_no_branch_to_delete_for_a_detached_worktree() {
        let chains = plan_chains(&[target("side", None)], AND_BRANCH);
        assert_eq!(labels(&chains).len(), 1);
    }

    #[test]
    fn test_labels_flatten_the_chains_in_order() {
        let chains = plan_chains(
            &[target("one", Some("one")), target("two", Some("two"))],
            AND_BRANCH,
        );
        assert_eq!(
            labels(&chains),
            vec![
                "git worktree remove  one",
                "git branch -d  one",
                "git worktree remove  two",
                "git branch -d  two",
            ]
        );
    }

    #[test]
    fn test_plan_chains_says_so_when_it_forces() {
        let chains = plan_chains(
            &[target("side", Some("side"))],
            Scope {
                branch: true,
                remote: false,
                force: true,
            },
        );
        assert_eq!(
            labels(&chains),
            vec!["git worktree remove --force  side", "git branch -D  side"]
        );
    }

    #[test]
    fn test_plan_chains_carries_the_force_flag_into_the_actions() {
        let chains = plan_chains(
            &[target("side", Some("side"))],
            Scope {
                branch: true,
                remote: false,
                force: true,
            },
        );
        assert_eq!(
            chains[0].steps[0].action,
            Action::RemoveWorktree {
                path: PathBuf::from("/w/side"),
                force: true,
            }
        );
        assert_eq!(
            chains[0].steps[1].action,
            Action::DeleteBranch {
                branch: String::from("side"),
                force: true,
            }
        );
    }

    #[test]
    fn test_plan_chains_leaves_the_remote_alone_by_default() {
        let chains = plan_chains(&[tracked("side", "side", "gitlab")], AND_BRANCH);
        assert_eq!(labels(&chains).len(), 2);
    }

    #[test]
    fn test_a_chain_deletes_the_remote_branch_last() {
        let chains = plan_chains(
            &[tracked("side", "side", "gitlab")],
            Scope {
                branch: true,
                remote: true,
                force: false,
            },
        );
        assert_eq!(
            labels(&chains),
            vec![
                "git worktree remove  side",
                "git branch -d  side",
                "git push gitlab --delete  side",
            ]
        );
    }

    #[test]
    fn test_a_chain_can_delete_the_remote_branch_while_keeping_the_local_one() {
        let chains = plan_chains(
            &[tracked("side", "side", "gitlab")],
            Scope {
                branch: false,
                remote: true,
                force: false,
            },
        );
        assert_eq!(
            labels(&chains),
            vec![
                "git worktree remove  side",
                "git push gitlab --delete  side"
            ]
        );
    }

    #[test]
    fn test_plan_chains_has_no_remote_branch_to_delete_when_none_was_pushed() {
        let chains = plan_chains(
            &[target("side", Some("side"))],
            Scope {
                branch: false,
                remote: true,
                force: false,
            },
        );
        assert_eq!(labels(&chains), vec!["git worktree remove  side"]);
    }

    #[test]
    fn test_plan_chains_carries_the_remote_into_the_action() {
        let chains = plan_chains(
            &[tracked("side", "feature/side", "gitlab")],
            Scope {
                branch: false,
                remote: true,
                force: false,
            },
        );
        assert_eq!(
            chains[0].steps[1].action,
            Action::DeleteRemoteBranch {
                remote: String::from("gitlab"),
                branch: String::from("feature/side"),
            }
        );
    }

    #[test]
    fn test_a_failed_branch_delete_skips_the_remote_delete_behind_it() {
        use std::sync::Mutex;

        let chains = plan_chains(
            &[tracked("side", "side", "gitlab")],
            Scope {
                branch: true,
                remote: true,
                force: false,
            },
        );
        let attempted = Mutex::new(Vec::new());
        let (tx, rx) = std::sync::mpsc::channel();

        run_chains(&chains, &tx, |step| {
            attempted.lock().unwrap().push(step.label.clone());
            match step.action {
                Action::DeleteBranch { .. } => Err(anyhow::anyhow!("not fully merged")),
                _ => Ok(String::from("removed")),
            }
        });
        drop(tx);

        assert_eq!(
            attempted.into_inner().unwrap().len(),
            2,
            "an unmerged branch must not have its remote copy pushed away"
        );
        let reports: Vec<Progress> = rx.iter().collect();
        assert!(reports.iter().any(|report| matches!(
            report,
            Progress::Ok(2, detail) if detail.contains("skipped")
        )));
    }

    #[test]
    fn test_chains_run_at_the_same_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let chains = plan_chains(&[target("one", None), target("two", None)], NOTHING_ELSE);
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let (tx, _rx) = std::sync::mpsc::channel();

        run_chains(&chains, &tx, |_step| {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(50));
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(String::from("removed"))
        });

        assert_eq!(
            peak.load(Ordering::SeqCst),
            2,
            "one slow removal must not hold up the others"
        );
    }

    #[test]
    fn test_a_chain_runs_its_own_steps_in_order() {
        use std::sync::Mutex;

        let chains = plan_chains(&[target("side", Some("side"))], AND_BRANCH);
        let order = Mutex::new(Vec::new());
        let (tx, _rx) = std::sync::mpsc::channel();

        run_chains(&chains, &tx, |step| {
            order.lock().unwrap().push(step.label.clone());
            Ok(String::from("removed"))
        });

        assert_eq!(
            order.into_inner().unwrap(),
            vec!["git worktree remove  side", "git branch -d  side"]
        );
    }

    #[test]
    fn test_a_failed_removal_skips_the_branch_delete_behind_it() {
        use std::sync::Mutex;

        let chains = plan_chains(&[target("side", Some("side"))], AND_BRANCH);
        let attempted = Mutex::new(Vec::new());
        let (tx, rx) = std::sync::mpsc::channel();

        run_chains(&chains, &tx, |step| {
            attempted.lock().unwrap().push(step.label.clone());
            Err(anyhow::anyhow!("contains modified files"))
        });
        drop(tx);

        assert_eq!(
            attempted.into_inner().unwrap().len(),
            1,
            "the branch of a surviving worktree must not be touched"
        );
        let reports: Vec<Progress> = rx.iter().collect();
        assert_eq!(failures(&reports).len(), 1);
        assert!(
            reports.iter().any(|report| matches!(
                report,
                Progress::Ok(1, detail) if detail.contains("skipped")
            )),
            "the skipped branch step needs a reason on screen"
        );
    }

    #[test]
    fn test_a_failed_chain_does_not_stop_the_others() {
        use std::sync::Mutex;

        let chains = plan_chains(&[target("one", None), target("two", None)], NOTHING_ELSE);
        let attempted = Mutex::new(Vec::new());
        let (tx, _rx) = std::sync::mpsc::channel();

        run_chains(&chains, &tx, |step| {
            attempted.lock().unwrap().push(step.label.clone());
            Err(anyhow::anyhow!("nope"))
        });

        assert_eq!(attempted.into_inner().unwrap().len(), 2);
    }

    fn run(cwd: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn repo_with_two_side_worktrees(root: &std::path::Path) -> crate::git::Repo {
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
        run(root, &["worktree", "add", "-q", "-b", "one", "../one"]);
        run(root, &["worktree", "add", "-q", "-b", "two", "../two"]);
        crate::git::Repo::discover(root).unwrap()
    }

    fn drain(receiver: std::sync::mpsc::Receiver<Progress>) -> Vec<Progress> {
        receiver.iter().collect()
    }

    fn failures(reports: &[Progress]) -> Vec<(usize, String)> {
        reports
            .iter()
            .filter_map(|report| match report {
                Progress::Failed(index, message) => Some((*index, message.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_spawn_removes_every_worktree_it_was_given() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = repo_with_two_side_worktrees(&root);
        let parent = root.parent().unwrap().to_path_buf();
        let chains = plan_chains(
            &[
                Target {
                    label: String::from("one"),
                    branch: Some(String::from("one")),
                    remote: None,
                    path: parent.join("one"),
                },
                Target {
                    label: String::from("two"),
                    branch: Some(String::from("two")),
                    remote: None,
                    path: parent.join("two"),
                },
            ],
            AND_BRANCH,
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        spawn(repo.main_clone.clone(), chains, sender);
        let reports = drain(receiver);

        assert!(failures(&reports).is_empty(), "{:?}", failures(&reports));
        assert!(matches!(reports.last(), Some(Progress::Finished)));
        assert!(!parent.join("one").exists());
        assert!(!parent.join("two").exists());
        assert!(!repo.branch_exists("one"));
    }

    #[test]
    fn test_spawn_keeps_going_after_a_step_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let repo = repo_with_two_side_worktrees(&root);
        let parent = root.parent().unwrap().to_path_buf();
        std::fs::write(parent.join("one").join("scratch.txt"), "untracked").unwrap();
        let chains = plan_chains(
            &[
                Target {
                    label: String::from("one"),
                    branch: Some(String::from("one")),
                    remote: None,
                    path: parent.join("one"),
                },
                Target {
                    label: String::from("two"),
                    branch: Some(String::from("two")),
                    remote: None,
                    path: parent.join("two"),
                },
            ],
            NOTHING_ELSE,
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        spawn(repo.main_clone.clone(), chains, sender);
        let reports = drain(receiver);

        assert_eq!(failures(&reports).len(), 1);
        assert_eq!(failures(&reports)[0].0, 0);
        assert!(parent.join("one").exists(), "the dirty one survives");
        assert!(!parent.join("two").exists(), "the clean one still goes");
        assert!(matches!(reports.last(), Some(Progress::Finished)));
    }
}
