use clap::{Parser, Subcommand};
use std::{fs, io, path::PathBuf};

/// Small demo app
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Name of the person to greet (used by `greet`)
    #[arg(short, long, default_value = "world")]
    name: String,

    /// Number of times to greet (used by `greet`)
    #[arg(short, long, default_value_t = 1)]
    count: u8,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List .env* files in the current working directory
    FindEnv,
    /// Default greeting behavior (same as running without a subcommand)
    Greet,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Greet) {
        Command::Greet => {
            for _ in 0..cli.count {
                println!("Hello {}!", cli.name);
            }
        }
        Command::FindEnv => {
            let files = find_env_files_in_cwd()?;
            if files.is_empty() {
                // No matches; print nothing or a note—your choice
                // eprintln!("No .env* files found in current directory.");
            } else {
                for path in files {
                    println!("{}", path.display());
                }
            }
        }
    }

    Ok(())
}

/// Return absolute paths of files whose names start with ".env" in the CWD (non-recursive).
fn find_env_files_in_cwd() -> io::Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let mut out = Vec::new();

    for entry in fs::read_dir(&cwd)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.starts_with(".env") {
                    // canonicalize to get absolute paths; fall back to original on error
                    match path.canonicalize() {
                        Ok(abs) => out.push(abs),
                        Err(_) => out.push(path),
                    }
                }
            }
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}
