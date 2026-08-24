use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::node::{self, Strategy, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    AddWorktree { branch: String, base: String },
    Install { rel: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub label: String,
    pub action: Action,
}

pub enum Progress {
    Running(usize),
    Ok(usize, String),
    Failed(usize, String),
    Finished,
}

pub struct Request {
    pub dest: PathBuf,
    pub steps: Vec<Step>,
}

pub fn plan_steps(branch: &str, base: &str, strategy: Strategy, targets: &[Target]) -> Vec<Step> {
    let mut steps = vec![Step {
        label: format!("git worktree add  {branch}"),
        action: Action::AddWorktree {
            branch: branch.to_string(),
            base: base.to_string(),
        },
    }];

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

    steps
}

pub fn skip_reason(dest: &Path, step: &Step) -> Option<&'static str> {
    let rel = match &step.action {
        Action::AddWorktree { .. } => return None,
        Action::Install { rel } => rel,
    };
    if dest.join(rel).is_dir() {
        None
    } else {
        Some("skipped (not in this worktree)")
    }
}

pub fn spawn(repo_source: PathBuf, request: Request, tx: Sender<Progress>) {
    std::thread::spawn(move || {
        run_steps(&request.steps, &tx, |step| {
            run_step(&repo_source, &request, step)
        });
    });
}

/// The worktree must exist before anything installs into it, so the leading
/// steps run in order; the installs that follow are independent and overlap.
fn run_steps<F>(steps: &[Step], tx: &Sender<Progress>, run: F)
where
    F: Fn(&Step) -> anyhow::Result<String> + Send + Sync,
{
    let first_install = steps
        .iter()
        .position(|step| matches!(step.action, Action::Install { .. }))
        .unwrap_or(steps.len());

    for (index, step) in steps[..first_install].iter().enumerate() {
        if !report(tx, index, step, &run) {
            let _ = tx.send(Progress::Finished);
            return;
        }
    }

    std::thread::scope(|scope| {
        for (offset, step) in steps[first_install..].iter().enumerate() {
            let tx = tx.clone();
            let run = &run;
            scope.spawn(move || report(&tx, first_install + offset, step, run));
        }
    });

    let _ = tx.send(Progress::Finished);
}

fn report<F>(tx: &Sender<Progress>, index: usize, step: &Step, run: &F) -> bool
where
    F: Fn(&Step) -> anyhow::Result<String>,
{
    let _ = tx.send(Progress::Running(index));
    match run(step) {
        Ok(detail) => {
            let _ = tx.send(Progress::Ok(index, detail));
            true
        }
        Err(error) => {
            let _ = tx.send(Progress::Failed(index, error.to_string()));
            false
        }
    }
}

fn run_step(repo_source: &Path, request: &Request, step: &Step) -> anyhow::Result<String> {
    if let Some(reason) = skip_reason(&request.dest, step) {
        return Ok(String::from(reason));
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_plan_steps_install_leaves_out_a_marker_package_with_no_lockfile() {
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
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].label, "npm ci  app");
    }

    #[test]
    fn test_plan_steps_always_starts_with_the_worktree_add() {
        let steps = plan_steps("feature/x", "develop", Strategy::None, &targets());
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0].action, Action::AddWorktree { .. }));
        assert!(steps[0].label.contains("feature/x"));
    }

    #[test]
    fn test_plan_steps_install_covers_every_package() {
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets());
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[1].action, Action::Install { .. }));
        assert!(matches!(steps[2].action, Action::Install { .. }));
    }

    #[test]
    fn test_skip_reason_never_skips_the_worktree_add() {
        let tmp = tempfile::tempdir().unwrap();
        let step = Step {
            label: String::from("git worktree add  feature/x"),
            action: Action::AddWorktree {
                branch: String::from("feature/x"),
                base: String::from("develop"),
            },
        };
        assert_eq!(skip_reason(tmp.path(), &step), None);
    }

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

    #[test]
    fn test_skip_reason_skips_a_directory_absent_from_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let step = Step {
            label: String::from("build/dist/vendored/node_modules"),
            action: Action::Install {
                rel: PathBuf::from("build/dist/vendored"),
            },
        };
        assert_eq!(
            skip_reason(tmp.path(), &step),
            Some("skipped (not in this worktree)")
        );
    }

    #[test]
    fn test_plan_steps_labels_a_root_package_with_the_bare_command() {
        let targets = vec![Target {
            rel: PathBuf::new(),
            has_lockfile: true,
        }];
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets);
        assert_eq!(steps[1].label, "npm ci");
    }

    fn worktree_step() -> Step {
        Step {
            label: String::from("git worktree add  feature/x"),
            action: Action::AddWorktree {
                branch: String::from("feature/x"),
                base: String::from("develop"),
            },
        }
    }

    fn install_step(rel: &str) -> Step {
        Step {
            label: format!("npm ci  {rel}"),
            action: Action::Install {
                rel: PathBuf::from(rel),
            },
        }
    }

    fn collect<F>(steps: &[Step], run: F) -> Vec<String>
    where
        F: Fn(&Step) -> anyhow::Result<String> + Send + Sync,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        run_steps(steps, &tx, run);
        drop(tx);
        rx.iter()
            .map(|progress| match progress {
                Progress::Running(index) => format!("running {index}"),
                Progress::Ok(index, detail) => format!("ok {index} {detail}"),
                Progress::Failed(index, message) => format!("failed {index} {message}"),
                Progress::Finished => String::from("finished"),
            })
            .collect()
    }

    #[test]
    fn test_run_steps_reports_every_install_even_when_one_fails() {
        let steps = vec![worktree_step(), install_step("app"), install_step("tools")];
        let messages = collect(&steps, |step| match &step.action {
            Action::Install { rel } if rel == Path::new("app") => {
                Err(anyhow::anyhow!("npm ci: broken lockfile"))
            }
            _ => Ok(String::from("done")),
        });

        assert!(
            messages.contains(&String::from("failed 1 npm ci: broken lockfile")),
            "{messages:?}"
        );
        assert!(
            messages.contains(&String::from("ok 2 done")),
            "{messages:?}"
        );
        assert_eq!(messages.last(), Some(&String::from("finished")));
    }

    #[test]
    fn test_run_steps_skips_the_installs_when_the_worktree_add_fails() {
        let steps = vec![worktree_step(), install_step("app")];
        let messages = collect(&steps, |step| match &step.action {
            Action::AddWorktree { .. } => Err(anyhow::anyhow!("branch exists")),
            _ => Ok(String::from("done")),
        });

        assert_eq!(
            messages,
            vec![
                String::from("running 0"),
                String::from("failed 0 branch exists"),
                String::from("finished"),
            ]
        );
    }

    #[test]
    fn test_run_steps_runs_the_installs_concurrently() {
        let steps = vec![worktree_step(), install_step("app"), install_step("tools")];
        let installs = steps.len() - 1;
        let started = std::sync::atomic::AtomicUsize::new(0);

        let messages = collect(&steps, |step| {
            if !matches!(step.action, Action::Install { .. }) {
                return Ok(String::from("created"));
            }
            started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while started.load(std::sync::atomic::Ordering::SeqCst) < installs {
                if std::time::Instant::now() > deadline {
                    return Err(anyhow::anyhow!("installs did not overlap"));
                }
                std::thread::yield_now();
            }
            Ok(String::from("installed"))
        });

        assert!(
            messages.contains(&String::from("ok 1 installed")),
            "{messages:?}"
        );
        assert!(
            messages.contains(&String::from("ok 2 installed")),
            "{messages:?}"
        );
    }

    #[test]
    fn test_plan_steps_labels_a_nested_package_with_its_path() {
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets());
        assert_eq!(steps[1].label, "npm ci  app");
        assert_eq!(steps[2].label, "npm ci  tools");
    }
}
