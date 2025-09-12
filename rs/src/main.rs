use clap::{Parser, Subcommand};
use ignore::{DirEntry, WalkBuilder};
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::time::{Duration, Instant};
use std::{
    collections::HashMap,
    fs,
    fs::File,
    io,
    path::{Path, PathBuf},
};

// Small demo
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
    #[allow(non_camel_case_types)]
    init,
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
        Command::init => {
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
            for path in &real {
                println!("{}", path.display());
            }

            println!("--- example env files ---");
            for path in examples {
                println!("{}", path.display());
            }

            let skeletons = extract_env_skeletons(&real)?;

            match ensure_env_examples_from_skeletons(&skeletons) {
                Ok(actions) => {
                    for (src, dst, action) in actions {
                        let label = match action {
                            ExampleAction::Created => "created",
                            ExampleAction::Overwritten => "overwritten",
                            ExampleAction::SourceIsExample => "skip",
                        };
                        println!(
                            "[env-example] {:<11} {}  ->  {}",
                            label,
                            src.display(),
                            dst.display()
                        );
                    }
                }
                Err(e) => eprintln!("error creating example files: {e}"),
            }

            // after ensure_env_examples_from_skeletons(...)
            match fix_gitignore_from_found(&cwd, &real) {
                Ok(report) => {
                    if report.changed {
                        println!(
                            "[gitignore] updated: {}\n  + added:   {:?}\n  - removed: {:?}",
                            report.path.display(),
                            report.added,
                            report.removed
                        );
                    } else {
                        println!("[gitignore] no changes needed ({})", report.path.display());
                    }
                }
                Err(e) => eprintln!("[gitignore] error: {e}"),
            }
        }
    }

    Ok(())
}

/// Recursively find absolute paths of files whose name starts with ".env",
/// using any `.eenvignore` files.
/// Also hard-skips `node_modules` for speed.
fn find_env_files_recursive(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true) // include .dot files (we want .env)
        .follow_links(false)
        .standard_filters(false) // respect .gitignore/.ignore/etc
        .parents(false) // also load ignore rules from parent dirs
        .add_custom_ignore_filename(".eenvignore") // our custom file(s)
        // Hard skip big dirs early (fastest):
        .filter_entry(|d| {
            // Allow root itself:
            if d.depth() == 0 {
                return true;
            }
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

fn extract_env_skeletons(files: &[PathBuf]) -> io::Result<HashMap<PathBuf, Vec<String>>> {
    let mut out: HashMap<PathBuf, Vec<String>> = HashMap::new();

    for path in files {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut lines = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();

            if trimmed.is_empty() {
                // keep blank line
                lines.push(String::new());
            } else if trimmed.starts_with('#') {
                // keep comments exactly
                lines.push(line);
            } else if let Some((key, _value)) = line.split_once('=') {
                // keep key but strip value
                lines.push(format!("{}=", key.trim()));
            } else {
                // line didn’t match KEY=VALUE, just preserve raw
                lines.push(line);
            }
        }

        out.insert(path.clone(), lines);
    }

    Ok(out)
}

fn example_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if file_name.ends_with(".example") {
        return path.to_path_buf();
    }
    let mut name = file_name.to_string();
    name.push_str(".example");
    path.with_file_name(name)
}

#[derive(Debug)]
enum ExampleAction {
    Created,         // file did not exist
    Overwritten,     // file existed; we replaced it
    SourceIsExample, // input was already *.example
}

fn ensure_env_examples_from_skeletons(
    skeletons: &std::collections::HashMap<PathBuf, Vec<String>>,
) -> io::Result<Vec<(PathBuf, PathBuf, ExampleAction)>> {
    let mut results = Vec::new();

    for (real_path, lines) in skeletons {
        let target = example_path_for(real_path);

        // If the source is already an .example, skip writing.
        if real_path == &target {
            results.push((real_path.clone(), target, ExampleAction::SourceIsExample));
            continue;
        }

        let existed = target.exists();
        write_lines_atomic(&target, lines)?; // see atomic helper below

        let action = if existed {
            ExampleAction::Overwritten
        } else {
            ExampleAction::Created
        };

        results.push((real_path.clone(), target, action));
    }

    Ok(results)
}

fn write_lines_atomic(path: &Path, lines: &[String]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut buf = lines.join("\n");
    if !buf.ends_with('\n') {
        buf.push('\n');
    }

    // write to temp file in same dir
    let tmp = path.with_extension("example.tmp~");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(buf.as_bytes())?;
        f.sync_all()?; // flush to disk
    }
    // atomic replace
    fs::rename(tmp, path)
}

/// Where to write the `.gitignore` (repo root). Walks up until it finds `.git`.
fn find_repo_root(start: &Path) -> io::Result<PathBuf> {
    let mut cur = start.canonicalize()?;
    loop {
        if cur.join(".git").exists() {
            return Ok(cur);
        }
        let Some(parent) = cur.parent() else {
            return Ok(start.to_path_buf());
        };
        cur = parent.to_path_buf();
    }
}

/// Patterns that should NOT be ignored and must be removed if present.
fn banned_env_ignores() -> &'static [&'static str] {
    &[
        // examples
        ".env.example",
        ".env*.example",
        ".env.*.example",
        "*.env.example",
        // encrypted
        ".env.enc",
        ".env*.enc",
        ".env.*.enc",
        "*.env.enc",
    ]
}

/// Strip trailing comments and trim.
fn pattern_core(line: &str) -> &str {
    let mut core = line;
    if let Some(hash) = line.find('#') {
        core = &line[..hash];
    }
    core.trim()
}

#[derive(Debug)]
pub struct GitignoreEdit {
    pub path: PathBuf,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: bool,
}

/// Convert an absolute path to a .gitignore pattern relative to repo root,
/// with forward slashes and spaces escaped.
fn to_gitignore_rel_pattern(abs: &std::path::Path, root: &std::path::Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    // Escape spaces (gitignore uses backslash escaping)
    let s = s.replace(' ', r"\ ");
    Some(if s.is_empty() { String::from("/") } else { s })
}

///  - removes banned rules (that ignore examples/encrypted)
///  - adds *exactly* the discovered real `.env*` files (relative to root)
pub fn fix_gitignore_from_found(
    project_root: &std::path::Path,
    real_env_files: &[std::path::PathBuf],
) -> std::io::Result<GitignoreEdit> {
    let root = find_repo_root(project_root)?;
    let path = root.join(".gitignore");

    let original = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = if original.is_empty() {
        Vec::new()
    } else {
        original.lines().map(|s| s.to_string()).collect()
    };

    use std::collections::{BTreeSet, HashSet};

    // 1) Remove banned patterns (exact core match against common forms)
    let banned: HashSet<&'static str> = banned_env_ignores().iter().copied().collect();
    let mut removed = Vec::new();
    lines.retain(|line| {
        let core = pattern_core(line);
        if !core.is_empty() && banned.contains(core) {
            removed.push(line.clone());
            false
        } else {
            true
        }
    });

    // 2) Build the required set from actually found files (relative patterns)
    let mut required: BTreeSet<String> = BTreeSet::new(); // sorted and dedup
    for abs in real_env_files {
        if let Some(pat) = to_gitignore_rel_pattern(abs, &root) {
            // For root-level ".env" this yields ".env", for nested "apps/api/.env"
            // this yields "apps/api/.env" — both correct for a root .gitignore.
            required.insert(pat);
        }
    }

    // Existing cores after removals
    let existing: HashSet<String> = lines.iter().map(|l| pattern_core(l).to_string()).collect();

    // 3) Append a block with missing required patterns (if any)
    let mut added = Vec::new();
    let missing: Vec<String> = required
        .into_iter()
        .filter(|r| !existing.contains(r))
        .collect();

    if !missing.is_empty() {
        if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
            lines.push(String::new());
        }
        lines.push("# === auto: env ignores (detected) ===".to_string());
        for m in &missing {
            lines.push(m.clone());
        }
        added.extend(missing);
    }

    // 4) Write back atomically if changed
    let new_text = {
        let mut s = lines.join("\n");
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s
    };

    let changed = new_text != original;
    if changed {
        let tmp = path.with_extension("tmp~");
        {
            let mut f = std::fs::File::create(&tmp)?;
            use std::io::Write;
            f.write_all(new_text.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(tmp, &path)?;
    }

    Ok(GitignoreEdit {
        path,
        added,
        removed,
        changed,
    })
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
