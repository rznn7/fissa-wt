use anyhow::{Result, anyhow};

// `init` joins the flags in the passthrough arm: without it the wrapper would
// capture this very function text and try to cd into it.
pub const SHELL_FUNCTION: &str = r#"fissa() { case "$1" in -*|init) command fissa "$@"; return $?;; esac; local d; d=$(command fissa "$@") || return $?; [ -n "$d" ] || return 0; cd "$d"; }
export FISSA_SHELL_INIT=1
"#;

/// The wrapper is plain POSIX sh with no prompt hook, so bash and zsh take the
/// same text; the argument exists to name the shell and to leave room for ones
/// that would need their own.
pub fn init_script(shell: Option<&str>) -> Result<&'static str> {
    match shell {
        Some("bash") | Some("zsh") => Ok(SHELL_FUNCTION),
        Some(other) => Err(anyhow!(
            "unsupported shell '{other}' — fissa init supports bash and zsh"
        )),
        None => Err(anyhow!("usage: fissa init <bash|zsh>")),
    }
}

pub fn wrapper_active() -> bool {
    std::env::var_os("FISSA_SHELL_INIT").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_function_defines_fissa_and_exports_marker() {
        assert!(SHELL_FUNCTION.contains("fissa()"));
        assert!(SHELL_FUNCTION.contains("-*|init)"));
        assert!(SHELL_FUNCTION.contains("command fissa"));
        assert!(SHELL_FUNCTION.contains("FISSA_SHELL_INIT=1"));
    }

    #[test]
    fn test_shell_function_guards_against_empty_output() {
        assert!(SHELL_FUNCTION.contains("[ -n \"$d\" ]"));
    }

    #[test]
    fn test_init_script_supports_bash_and_zsh() {
        assert_eq!(init_script(Some("bash")).unwrap(), SHELL_FUNCTION);
        assert_eq!(init_script(Some("zsh")).unwrap(), SHELL_FUNCTION);
    }

    #[test]
    fn test_init_script_rejects_a_shell_it_cannot_emit_for() {
        let error = init_script(Some("fish")).unwrap_err().to_string();
        assert!(error.contains("fish"), "{error}");
        assert!(error.contains("bash"), "{error}");
        assert!(error.contains("zsh"), "{error}");
    }

    #[test]
    fn test_init_script_without_a_shell_reports_the_usage() {
        let error = init_script(None).unwrap_err().to_string();
        assert!(error.contains("fissa init"), "{error}");
        assert!(error.contains("bash"), "{error}");
        assert!(error.contains("zsh"), "{error}");
    }

    struct WrapperRun {
        status: i32,
        pwd: String,
        stdout: String,
    }

    fn run_wrapper(args: &str, stub_stdout: &str, stub_exit: i32) -> WrapperRun {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let chosen = tmp.path().join("chosen");
        fs::create_dir_all(&chosen).unwrap();

        let echo_line = if stub_stdout.is_empty() {
            String::new()
        } else {
            format!(
                "echo '{}'\n",
                stub_stdout.replace("{CHOSEN}", &chosen.display().to_string())
            )
        };
        let stub = bin.join("fissa");
        fs::write(&stub, format!("#!/bin/sh\n{echo_line}exit {stub_exit}\n")).unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

        let init = tmp.path().join("init.sh");
        fs::write(&init, SHELL_FUNCTION).unwrap();

        let output = Command::new("bash")
            .arg("-c")
            .arg(format!(
                ". '{}'; fissa {args}; printf 'status=%s\npwd=%s\n' \"$?\" \"$(pwd)\"",
                init.display()
            ))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .current_dir(tmp.path())
            .output()
            .unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let field = |key: &str| {
            stdout
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .unwrap_or_default()
                .to_string()
        };
        WrapperRun {
            status: field("status=").parse().unwrap(),
            pwd: field("pwd="),
            stdout,
        }
    }

    #[test]
    fn test_wrapper_cds_to_the_printed_directory() {
        let run = run_wrapper("", "{CHOSEN}", 0);
        assert_eq!(run.status, 0);
        assert!(run.pwd.ends_with("/chosen"), "pwd was {}", run.pwd);
    }

    #[test]
    fn test_wrapper_returns_zero_when_nothing_was_selected() {
        let run = run_wrapper("", "", 0);
        assert_eq!(run.status, 0);
        assert!(!run.pwd.ends_with("/chosen"));
    }

    #[test]
    fn test_wrapper_propagates_a_failing_exit_status() {
        let run = run_wrapper("", "", 3);
        assert_eq!(run.status, 3);
    }

    #[test]
    fn test_wrapper_passes_init_output_through_without_cd() {
        let run = run_wrapper("init zsh", "FUNCTION-TEXT", 0);
        assert_eq!(run.status, 0);
        assert!(run.stdout.contains("FUNCTION-TEXT"), "got {}", run.stdout);
        assert!(!run.pwd.ends_with("/chosen"));
    }

    #[test]
    fn test_wrapper_passes_flag_output_through_without_cd() {
        let run = run_wrapper("--help", "USAGE-TEXT", 0);
        assert_eq!(run.status, 0);
        assert!(run.stdout.contains("USAGE-TEXT"), "got {}", run.stdout);
        assert!(!run.pwd.ends_with("/chosen"));
    }
}
