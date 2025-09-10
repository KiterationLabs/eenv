use clap::{Parser, Subcommand};
use std::time::{Duration, Instant};
use ignore::{WalkBuilder, DirEntry};
use std::{fs, io, path::{Path, PathBuf}};

/// Small demo
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
            //let files = find_env_files_in_cwd()?;
            let cwd = std::env::current_dir()?;
            //let files = find_env_files_recursive(&cwd)?;
            //let (real, examples) = split_env_files(files);

            // find files (fallible)
            let (files, _t_find) = time_result("find_env_files_recursive", || {
                find_env_files_recursive(&cwd)
            })?;

            // split files (non-fallible) — move `files` into the closure
            let ((real, examples), _t_split) =
                time_ok("split_env_files", move || split_env_files(files));

            println!("--- real env files ---");
            for path in real {
                println!("{}", path.display());
            }

            println!("--- example env files ---");
            for path in examples {
                println!("{}", path.display());
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

/// Recursively find absolute paths of files whose name starts with ".env",
/// honoring `.gitignore`, `.ignore`, and any `.eenvignore` files.
/// Also hard-skips `node_modules` for speed.
fn find_env_files_recursive(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)            // include .dot files (we want .env)
        .follow_links(false)
        .standard_filters(false)   // respect .gitignore/.ignore/etc
        .parents(false)            // also load ignore rules from parent dirs
        .add_custom_ignore_filename(".eenvignore")  // our custom file(s)
        // Hard skip big dirs early (fastest):
        .filter_entry(|d| {
            // Allow root itself:
            if d.depth() == 0 { return true; }
            true
        });

    let mut out = Vec::new();
    for result in builder.build() {
        let dent = match result {
            Ok(d) => d,
            Err(err) => {
                eprintln!("walk error: {err}");
                continue;
            }
        };
        if is_env_file(&dent) {
            let abs = dent
                .path()
                .canonicalize()
                .unwrap_or_else(|_| dent.path().to_path_buf());
            out.push(abs);
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// Split into (real_envs, example_envs)
fn split_env_files(mut files: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    // Sort and dedup first
    files.sort();
    files.dedup();

    let mut real = Vec::new();
    let mut examples = Vec::new();

    for path in files {
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.ends_with(".example") {
                examples.push(path);
            } else {
                real.push(path);
            }
        }
    }

    (real, examples)
}

fn is_env_file(d: &DirEntry) -> bool {
    if !d.file_type().map(|t| t.is_file()).unwrap_or(false) {
        return false;
    }
    match d.path().file_name().and_then(|s| s.to_str()) {
        Some(name) if name.starts_with(".env") => true,
        _ => false,
    }
}

fn time_result<F, T, E>(label: &str, f: F) -> Result<(T, Duration), E>
where
    F: FnOnce() -> Result<T, E>,
{
    let start = Instant::now();
    let out = f()?;
    let dt = start.elapsed();
    eprintln!("[time] {label}: {:.3} ms", dt.as_secs_f64() * 1000.0);
    Ok((out, dt))
}

fn time_ok<F, T>(label: &str, f: F) -> (T, Duration)
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let out = f();
    let dt = start.elapsed();
    eprintln!("[time] {label}: {:.3} ms", dt.as_secs_f64() * 1000.0);
    (out, dt)
}
