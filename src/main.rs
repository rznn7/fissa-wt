mod app;
mod components;
mod create;
mod git;
mod naming;
mod node;
mod shell;

const HELP: &str = "\
fissa — git worktree TUI

USAGE:
    fissa                  open the TUI in the current repository
    fissa --shell-init     print the shell function that enables `cd` on exit
    fissa --version        print the version
    fissa --help           print this help

SETUP:
    Add to ~/.zshrc or ~/.bashrc:

        eval \"$(fissa --shell-init)\"

    Without it everything works except landing in the selected worktree.
";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--shell-init") {
        print!("{}", shell::SHELL_FUNCTION);
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
