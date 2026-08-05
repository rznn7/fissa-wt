use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::node::{self, Strategy, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    AddWorktree { branch: String, base: String },
    Hardlink { rel: PathBuf },
    Symlink { rel: PathBuf },
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
    pub source: PathBuf,
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
            String::from("node_modules")
        } else {
            format!("{}/node_modules", rel.to_string_lossy())
        };

        let action = match strategy {
            Strategy::Hardlink => Action::Hardlink { rel },
            Strategy::Symlink => Action::Symlink { rel },
            Strategy::Install => Action::Install { rel },
            Strategy::None => continue,
        };

        steps.push(Step {
            label: shown,
            action,
        });
    }

    steps
}

pub fn skip_reason(dest: &Path, step: &Step) -> Option<&'static str> {
    let rel = match &step.action {
        Action::AddWorktree { .. } => return None,
        Action::Hardlink { rel } | Action::Symlink { rel } | Action::Install { rel } => rel,
    };
    if dest.join(rel).is_dir() {
        None
    } else {
        Some("skipped (not in this worktree)")
    }
}

pub fn spawn(repo_source: PathBuf, request: Request, tx: Sender<Progress>) {
    std::thread::spawn(move || {
        for (index, step) in request.steps.iter().enumerate() {
            let _ = tx.send(Progress::Running(index));

            let outcome = run_step(&repo_source, &request, step);
            match outcome {
                Ok(detail) => {
                    let _ = tx.send(Progress::Ok(index, detail));
                }
                Err(error) => {
                    let _ = tx.send(Progress::Failed(index, error.to_string()));
                    return;
                }
            }
        }
        let _ = tx.send(Progress::Finished);
    });
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
        Action::Hardlink { rel } => {
            let src = request.source.join(rel).join("node_modules");
            let dst = request.dest.join(rel).join("node_modules");
            node::hardlink_modules(&src, &dst)?;
            Ok(format!("hardlinked ({} pkgs)", node::package_count(&dst)))
        }
        Action::Symlink { rel } => {
            let src = request.source.join(rel).join("node_modules");
            let dst = request.dest.join(rel).join("node_modules");
            node::symlink_modules(&src, &dst)?;
            Ok(String::from("symlinked"))
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
                has_source_modules: true,
                has_lockfile: true,
            },
            Target {
                rel: PathBuf::from("tools"),
                has_source_modules: false,
                has_lockfile: true,
            },
        ]
    }

    #[test]
    fn test_plan_steps_install_leaves_out_a_marker_package_with_no_lockfile() {
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
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].label, "app/node_modules");
    }

    #[test]
    fn test_plan_steps_always_starts_with_the_worktree_add() {
        let steps = plan_steps("feature/x", "develop", Strategy::None, &targets());
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0].action, Action::AddWorktree { .. }));
        assert!(steps[0].label.contains("feature/x"));
    }

    #[test]
    fn test_plan_steps_hardlink_covers_only_targets_with_source_modules() {
        let steps = plan_steps("feature/x", "develop", Strategy::Hardlink, &targets());
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[1].action, Action::Hardlink { .. }));
        assert!(steps[1].label.contains("app"));
    }

    #[test]
    fn test_plan_steps_install_covers_every_package() {
        let steps = plan_steps("feature/x", "develop", Strategy::Install, &targets());
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[1].action, Action::Install { .. }));
        assert!(matches!(steps[2].action, Action::Install { .. }));
    }

    #[test]
    fn test_plan_steps_symlink_produces_symlink_actions() {
        let steps = plan_steps("feature/x", "develop", Strategy::Symlink, &targets());
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[1].action, Action::Symlink { .. }));
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
            action: Action::Hardlink {
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
}
