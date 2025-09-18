use clap::{Parser, Subcommand};
use std::io;

use crate::util::find_repo_root;
use crate::{hooks, precommit, types::HookAction};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(short, long, default_value = "world")]
    pub name: String,
    #[arg(short, long, default_value_t = 1)]
    pub count: u8,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[allow(non_camel_case_types)]
    init,
    PreCommit {
        #[arg(long)]
        write: bool,
    },
    Hook {
        #[arg(value_enum)]
        action: HookAction,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Greet,
}

pub fn dispatch(cli: Cli) -> io::Result<()> {
    match cli.command.unwrap_or(Command::Greet) {
        Command::init => {
            let cwd = std::env::current_dir()?;
            let repo_root = find_repo_root(&cwd)?;
            if let Err(e) = hooks::install_git_hook(&repo_root, false) {
                eprintln!("[hook] WARN: could not install pre-commit hook: {e}");
            }
            crate::init::run(&repo_root)?;
        }
        Command::PreCommit { write } => {
            let cwd = std::env::current_dir()?;
            let repo_root = find_repo_root(&cwd)?;
            if let Err(e) = hooks::install_git_hook(&repo_root, false) {
                eprintln!("[hook] WARN: could not ensure pre-commit hook: {e}");
            }
            if let Err(e) = precommit::pre_commit(&repo_root, write) {
                eprintln!("[pre-commit] {e}");
                std::process::exit(1);
            }
        }
        Command::Hook { action, force } => {
            let cwd = std::env::current_dir()?;
            let repo_root = find_repo_root(&cwd)?;
            match action {
                HookAction::Install => {
                    if let Err(e) = hooks::install_git_hook(&repo_root, force) {
                        eprintln!("[hook] ERROR: {e}");
                        std::process::exit(1);
                    }
                    println!("[hook] installed (force={force})");
                }
                HookAction::Uninstall => {
                    if let Err(e) = hooks::uninstall_git_hook(&repo_root, force) {
                        eprintln!("[hook] ERROR: {e}");
                        std::process::exit(1);
                    }
                    println!("[hook] uninstalled");
                }
            }
        }
        Command::Greet => {
            for _ in 0..cli.count {
                println!("Hello {}!", cli.name);
            }
        }
    }
    Ok(())
}
