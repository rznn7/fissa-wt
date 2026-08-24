mod app;
mod components;
mod create;
mod dirty;
mod git;
mod naming;
mod node;
mod shell;

const HELP: &str = "\
fissa — git worktree TUI

USAGE:
    fissa                  open the TUI in the current repository
    fissa init <shell>     print the shell function that enables `cd` on exit
                           (shell is bash or zsh)
    fissa --version        print the version
    fissa --help           print this help

SETUP:
    Add to ~/.zshrc or ~/.bashrc:

        eval \"$(fissa init zsh)\"

    Without it everything works except landing in the selected worktree.
";

fn unknown_flag(args: &[String]) -> Option<&str> {
    args.iter()
        .map(String::as_str)
        .find(|arg| arg.starts_with('-'))
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().is_some_and(|a| a == "init") {
        print!("{}", shell::init_script(args.get(1).map(String::as_str))?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("fissa {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }

    // Falling through to the TUI would render on stderr from inside a
    // command substitution and hang the shell that ran it.
    if let Some(flag) = unknown_flag(&args) {
        anyhow::bail!("unknown flag '{flag}' — see fissa --help");
    }

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = app::restore_terminal();
        previous_hook(info);
    }));

    let repo = git::Repo::discover(&std::env::current_dir()?)?;
    let mut terminal = app::init_terminal()?;
    let result = app::App::new(repo).and_then(|app| app.run(&mut terminal));
    app::restore_terminal()?;

    if let Some(path) = result? {
        println!("{}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn test_unknown_flag_is_reported_rather_than_opening_the_tui() {
        assert_eq!(unknown_flag(&args(&["--shell-init"])), Some("--shell-init"));
    }

    #[test]
    fn test_no_arguments_is_not_an_unknown_flag() {
        assert_eq!(unknown_flag(&args(&[])), None);
    }

    #[test]
    fn test_a_bare_subcommand_is_not_an_unknown_flag() {
        assert_eq!(unknown_flag(&args(&["init", "zsh"])), None);
    }
}
