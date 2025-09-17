use clap::{Parser, Subcommand};
use ignore::{DirEntry, WalkBuilder};
use rand::{distr::Alphanumeric, Rng};
use serde_json::{json, Value};
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::process::Command as Proc;
use std::time::{Duration, Instant};
use std::{
    collections::HashMap,
    fs,
    fs::File,
    io,
    path::{Path, PathBuf},
};
use blake3;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};

const MAGIC: &[u8; 5] = b"EENV1"; // magic + version

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
    /// Run validations for git pre-commit
    PreCommit {
        /// Also write/refresh artifacts (*.example, *.enc) and git add them
        #[arg(long)]
        write: bool,
    },
    /// Default greeting behavior (same as running without a subcommand)
    Greet,
}

#[derive(Debug, Clone, Copy)]
pub struct EenvState {
    pub enc: bool,      // any .env*.enc files exist
    pub example: bool,  // any .env*.example files exist
    pub env: bool,      // any real .env* files exist
    pub eenvjson: bool, // eenv.config.json exists AND is valid (JSON object with non-empty "key")
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Greet) {
        Command::PreCommit { write } => {
            let cwd = std::env::current_dir()?;
            let repo_root = find_repo_root(&cwd)?;
            // Exit with non-zero on violation so Git aborts the commit
            if let Err(e) = pre_commit(&repo_root, write) {
                eprintln!("[pre-commit] {e}");
                std::process::exit(1);
            }
        }
        Command::Greet => {
            for _ in 0..cli.count {
                println!("Hello {}!", cli.name);
            }
        }
        Command::init => {
            let cwd = std::env::current_dir()?;
            let repo_root = find_repo_root(&cwd)?;

            init(&repo_root)?;
        }
    }

    Ok(())
}

fn init(repo_root: &Path) -> io::Result<()> {
    let state = compute_eenv_state(repo_root)?;
    println!("[state]");
    println!("enc      = {}", state.enc);
    println!("example  = {}", state.example);
    println!("env      = {}", state.env);
    println!("eenvjson = {}", state.eenvjson);
    println!("-----------------");

   // decrypt if .enc exist (requires valid config)
    if state.enc {
        if state.eenvjson {
            if let Err(e) = handle_enc_workflow(repo_root) {
                eprintln!("[enc] error: {e}");
            }
        } else {
            // bootstrap key flow
            match bootstrap_key_and_decrypt(repo_root) {
                Ok(()) => eprintln!("[enc] key accepted, config created, decrypted where possible."),
                Err(e) => {
                    eprintln!("[enc] could not bootstrap from key: {e}");
                    return Err(e);
                }
            }
        }
    }

    // examples, .gitignore, and encrypt (only for real plaintext files)
    if state.env {
        let (files, _t_find) = time_result("find_env_files_recursive", || find_env_files_recursive(repo_root))?;
        let ((real, examples, encs), _t_split) = time_ok("split_env_files", move || split_env_files(files));

        println!("--- real env files ---");     for p in &real { println!("{}", p.display()); }
        println!("--- example env files ---");  for p in &examples { println!("{}", p.display()); }
        println!("--- encrypted env files ---");for p in &encs { println!("{}", p.display()); }

        if !state.example && !real.is_empty() {
            let skeletons = extract_env_skeletons(&real)?;
            if let Ok(actions) = ensure_env_examples_from_skeletons(&skeletons) {
                for (src, dst, action) in actions {
                    let label = match action {
                        ExampleAction::Created => "created",
                        ExampleAction::Overwritten => "overwritten",
                        ExampleAction::SourceIsExample => "skip",
                    };
                    println!("[env-example] {:<11} {}  ->  {}", label, src.display(), dst.display());
                }
            }
        }

        match fix_gitignore_from_found(repo_root, &real) {
            Ok(report) => {
                if report.changed {
                    println!("[gitignore] updated: {}\n  + added:   {:?}\n  - removed: {:?}", report.path.display(), report.added, report.removed);
                } else {
                    println!("[gitignore] no changes needed ({})", report.path.display());
                }
            }
            Err(e) => eprintln!("[gitignore] error: {e}"),
        }

        match ensure_eenv_config(repo_root) {
            Ok(ConfigStatus::Created) => eprintln!("[config] created eenv.config.json"),
            Ok(ConfigStatus::FixedMissingKey) => eprintln!("[config] injected key into eenv.config.json"),
            Ok(ConfigStatus::RewrittenFromInvalid { backup }) => eprintln!("[config] repaired eenv.config.json (backup: {})", backup.display()),
            Ok(ConfigStatus::Valid) => {}
            Err(e) => eprintln!("[config] error: {e}"),
        }

        let produced = encrypt_envs_to_enc(repo_root, &real)?;
        for p in &produced {
            println!("[init] encrypted -> {}", p.display());
        }
    }

    Ok(())
}

fn handle_enc_workflow(repo_root: &Path) -> io::Result<()> {
    let key = read_eenv_key(repo_root)?;
    let aead = XChaCha20Poly1305::new((&key).into());

    let files = find_env_files_recursive(repo_root)?;
    let (_real, _examples, encs) = split_env_files(files);

    for enc_path in encs {
        let dst = dec_output_path(&enc_path);

        // Don’t clobber an existing plaintext by default.
        if dst.exists() {
            eprintln!("[enc] skip decrypt (target exists): {}", dst.display());
            continue;
        }

        match decrypt_file_from_enc(&aead, &enc_path, &dst) {
            Ok(()) => println!("[enc] decrypted {} -> {}", enc_path.display(), dst.display()),
            Err(e) => eprintln!("[enc] WARN: could not decrypt {} ({})", enc_path.display(), e),
        }
    }
    Ok(())
}

fn enc_output_path(input: &Path) -> PathBuf {
    // ".env" -> ".env.enc", ".env.local" -> ".env.local.enc"
    let mut name = input.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
    name.push_str(".enc");
    input.with_file_name(name)
}

fn dec_output_path(input_enc: &Path) -> PathBuf {
    // strip trailing ".enc"
    let name = input_enc.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if let Some(stripped) = name.strip_suffix(".enc") {
        input_enc.with_file_name(stripped)
    } else {
        input_enc.with_file_name(name) // fallback
    }
}

fn encrypt_file_to_enc(aead: &XChaCha20Poly1305, src: &Path, dst: &Path) -> io::Result<()> {
    let plaintext = fs::read(src)?;
    let nonce_bytes: [u8; 24] = rand::rng().random();
    let nonce = XNonce::from_slice(&nonce_bytes);
    let mut out = Vec::with_capacity(MAGIC.len() + nonce_bytes.len() + plaintext.len() + 32);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    let ciphertext = aead
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "encrypt failed"))?;
    out.extend_from_slice(&ciphertext);
    write_bytes_atomic(dst, &out)
}

fn decrypt_file_from_enc(aead: &XChaCha20Poly1305, src_enc: &Path, dst: &Path) -> io::Result<()> {
    let data = fs::read(src_enc)?;
    if data.len() < MAGIC.len() + 24 + 16 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "enc file too short"));
    }
    if &data[..MAGIC.len()] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic/version"));
    }
    let nonce_bytes = &data[MAGIC.len()..MAGIC.len() + 24];
    let nonce = XNonce::from_slice(nonce_bytes);
    let ciphertext = &data[MAGIC.len() + 24..];
    let plaintext = aead
        .decrypt(nonce, ciphertext)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decrypt failed (wrong key?)"))?;
    write_bytes_atomic(dst, &plaintext)
}

/// Parse + validate eenv.config.json without mutating it.
/// Valid = file exists, is JSON object, and has non-empty "key": string
fn validate_eenv_config(repo_root: &Path) -> io::Result<bool> {
    let path = eenv_config_path(repo_root);
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) if v.is_object() => {
            let ok = matches!(v.get("key"), Some(serde_json::Value::String(s)) if !s.is_empty());
            Ok(ok)
        }
        _ => Ok(false),
    }
}

/// Inspect the repository and return the four booleans without changing anything.
fn compute_eenv_state(repo_root: &Path) -> io::Result<EenvState> {
    let files = find_env_files_recursive(repo_root)?;
    let (real, examples, encs) = split_env_files(files);

    let enc = !encs.is_empty();
    let example = !examples.is_empty();
    let env = !real.is_empty();
    let eenvjson = validate_eenv_config(repo_root)?;

    Ok(EenvState { enc, example, env, eenvjson })
}

fn eenv_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join("eenv.config.json")
}

/// Generate a random key (44 chars like your example)
fn generate_key() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(44)
        .map(char::from)
        .collect()
}

/// Prompt user for a key (non-empty). Echoed input; swap to `rpassword` if you want hidden input.
fn prompt_for_key() -> io::Result<String> {
    print!("eenv: existing eenv.config.json is invalid.\nEnter key to use: ");
    io::stdout().flush()?;

    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    let key = s.trim().to_string();

    if key.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty key not allowed",
        ));
    }
    Ok(key)
}

/// Ensure eenv.config.json exists; create with random key if missing.
/// Returns true if we had to create it.
/// Write atomically: tmp -> rename
fn write_string_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp~");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

#[derive(Debug)]
enum ConfigStatus {
    Created,                                  // created new file
    Valid,                                    // already valid, unchanged
    FixedMissingKey,                          // added "key" to existing valid JSON
    RewrittenFromInvalid { backup: PathBuf }, // invalid JSON -> backed up and replaced
}

/// Ensure eenv.config.json exists and is valid JSON with a "key": "<string>".
/// - If file missing -> create with random key
/// - If invalid JSON -> back up original (*.bak) and write fresh JSON
/// - If valid but missing/invalid "key" -> inject a new key and rewrite (preserving other fields)
fn ensure_eenv_config(repo_root: &Path) -> io::Result<ConfigStatus> {
    let path = eenv_config_path(repo_root);

    if !path.exists() {
        let key = generate_key();
        let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key);
        write_string_atomic(&path, &pretty)?;
        return Ok(ConfigStatus::Created);
    }

    let text = fs::read_to_string(&path)?;
    match serde_json::from_str::<Value>(&text) {
        Ok(mut v) => {
            // Ensure it's an object
            if !v.is_object() {
                // treat as invalid structure: rewrite from scratch, back up
                let backup = backup_invalid_config(&path, &text)?;
                let key = generate_key();
                let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key);
                write_string_atomic(&path, &pretty)?;
                return Ok(ConfigStatus::RewrittenFromInvalid { backup });
            }

            // Ensure "key" exists and is a non-empty string
            let needs_key = match v.get("key") {
                Some(Value::String(s)) => s.is_empty(),
                _ => true,
            };

            if needs_key {
                // Preserve other fields; just set/replace "key"
                let key = generate_key();
                v.as_object_mut()
                    .expect("object checked")
                    .insert("key".into(), Value::String(key));
                let mut pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| {
                    // Fallback: minimal JSON with only key
                    let key = generate_key();
                    json!({ "key": key }).to_string()
                });
                if !pretty.ends_with('\n') {
                    pretty.push('\n');
                }
                write_string_atomic(&path, &pretty)?;
                Ok(ConfigStatus::FixedMissingKey)
            } else {
                Ok(ConfigStatus::Valid)
            }
        }
        Err(_) => {
            // Invalid JSON: prompt user for key, back up original, then write fresh JSON with that key
            let key = prompt_for_key()?; // <-- user-provided
            let backup = backup_invalid_config(&path, &text)?;
            let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key);
            write_string_atomic(&path, &pretty)?;
            Ok(ConfigStatus::RewrittenFromInvalid { backup })
        }
    }
}

/// Save a copy of the invalid config next to the original (timestamped .bak).
fn backup_invalid_config(path: &Path, contents: &str) -> io::Result<PathBuf> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let backup = path.with_extension(format!("bak.{ts}"));
    write_string_atomic(&backup, contents)?;
    Ok(backup)
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

/// Split into (real_plain, example_envs, encrypted_envs)
fn split_env_files(mut files: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    files.sort();
    files.dedup();

    let mut real = Vec::new();
    let mut examples = Vec::new();
    let mut encs = Vec::new();

    for path in files {
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.ends_with(".example") {
                examples.push(path);
            } else if name.ends_with(".enc") {
                encs.push(path);
            } else {
                real.push(path);
            }
        }
    }

    (real, examples, encs)
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
fn fix_gitignore_from_found(
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
        let Some(fname) = abs.file_name().and_then(|s| s.to_str()) else { continue; };
        if fname.ends_with(".example") || fname.ends_with(".enc") {
            continue; // <- don't ignore examples or encrypted artifacts
        }
        if let Some(pat) = to_gitignore_rel_pattern(abs, &root) {
            required.insert(pat);
        }
    }

    required.insert("eenv.config.json".to_string());

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
        lines.push("# added by eenv".to_string());
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

fn pre_commit(repo_root: &Path, write: bool) -> io::Result<()> {
    // A) block plain .env* files being committed (except .example / .enc)
    let staged = staged_files(repo_root)?;
    let mut offenders: Vec<PathBuf> = Vec::new();
    for p in &staged {
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if name.starts_with(".env") && !name.ends_with(".example") && !name.ends_with(".enc") {
                offenders.push(p.clone());
            }
        }
    }
    if !offenders.is_empty() {
        eprintln!("[pre-commit] ❌ refusing to commit raw .env files:");
        for p in offenders {
            eprintln!("  - {}", p.display());
        }
        eprintln!("Hint: encrypt them to .env*.enc or add them to .gitignore.");
        return Err(io::Error::new(io::ErrorKind::Other, "raw .env staged"));
    }

    // B) discover env files
    let (files, _t_find) = time_result("find_env_files_recursive", || {
        find_env_files_recursive(repo_root)
    })?;
    let ((real, examples, encs), _t_split) = time_ok("split_env_files", || split_env_files(files));

    // C) ensure .env.example exist (optional write mode)
    if write && !real.is_empty() {
        let skeletons = extract_env_skeletons(&real)?;
        match ensure_env_examples_from_skeletons(&skeletons) {
            Ok(actions) => {
                let mut to_add: Vec<PathBuf> = Vec::new();
                for (_src, dst, action) in actions {
                    match action {
                        ExampleAction::Created | ExampleAction::Overwritten => to_add.push(dst),
                        ExampleAction::SourceIsExample => {}
                    }
                }
                if !to_add.is_empty() {
                    git_add(repo_root, &to_add)?;
                }
            }
            Err(e) => eprintln!("[pre-commit] example gen error: {e}"),
        }
    }

    // D) align .gitignore with found real env files (optional write mode)
    if write && !real.is_empty() {
        match fix_gitignore_from_found(repo_root, &real) {
            Ok(report) => {
                if report.changed {
                    git_add(repo_root, &[report.path])?;
                }
            }
            Err(e) => eprintln!("[pre-commit] gitignore fix error: {e}"),
        }
    }

    // E) refresh encrypted files and stage them
    if write && !real.is_empty() {
        // create/fix the config if needed
        match ensure_eenv_config(repo_root) {
            Ok(ConfigStatus::Created) => eprintln!("[config] created eenv.config.json"),
            Ok(ConfigStatus::FixedMissingKey) => eprintln!("[config] injected key into eenv.config.json"),
            Ok(ConfigStatus::RewrittenFromInvalid { backup }) => eprintln!("[config] repaired eenv.config.json (backup: {})", backup.display()),
            Ok(ConfigStatus::Valid) => {}
            Err(e) => eprintln!("[config] error: {e}"),
        }

        // now encrypt
        let produced = encrypt_envs_to_enc(repo_root, &real)?;
        if !produced.is_empty() {
            git_add(repo_root, &produced)?;
        }
    }

    Ok(())
}

fn aead_from_key_str(key_str: &str) -> io::Result<XChaCha20Poly1305> {
    if key_str.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty key"));
    }
    let hash = blake3::hash(key_str.as_bytes());
    Ok(XChaCha20Poly1305::new(hash.as_bytes().into()))
}

fn write_eenv_config_with_key(repo_root: &Path, key_str: &str) -> io::Result<()> {
    let path = eenv_config_path(repo_root);
    let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key_str);
    write_string_atomic(&path, &pretty)
}

/// Ensure `.gitignore` contains `eenv.config.json` even if there are no real .env files yet.
fn ensure_gitignore_has_config(repo_root: &Path) -> io::Result<()> {
    let root = find_repo_root(repo_root)?;
    let path = root.join(".gitignore");

    let original = if path.exists() { fs::read_to_string(&path)? } else { String::new() };
    let mut lines: Vec<String> = if original.is_empty() { Vec::new() } else { original.lines().map(|s| s.to_string()).collect() };

    let already = lines.iter().any(|l| {
        let core = pattern_core(l);
        core == "eenv.config.json"
    });

    if !already {
        if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
            lines.push(String::new());
        }
        lines.push("# added by eenv".to_string());
        lines.push("eenv.config.json".to_string());

        let mut s = lines.join("\n");
        if !s.ends_with('\n') { s.push('\n'); }

        let tmp = path.with_extension("tmp~");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(s.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(tmp, &path)?;
    }
    Ok(())
}

fn bootstrap_key_and_decrypt(repo_root: &Path) -> io::Result<()> {
    // 1) Ask user for key
    let key_str = prompt_for_key()?;
    let aead = aead_from_key_str(&key_str)?;

    // 2) Find encrypted candidates
    let files = find_env_files_recursive(repo_root)?;
    let (_real, _examples, encs) = split_env_files(files);
    if encs.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no .env*.enc files found"));
    }

    // 3) Validate the key by attempting to decrypt at least one file
    //    We won’t clobber a plaintext that already exists; if it exists,
    //    we decrypt to a temp to validate, then discard the temp.
    let mut validated = false;
    for enc_path in &encs {
        let dst = dec_output_path(enc_path);
        if dst.exists() {
            // decrypt to temp just to validate
            let tmp = dst.with_extension("validate.tmp~");
            match decrypt_file_from_enc(&aead, enc_path, &tmp) {
                Ok(()) => {
                    let _ = fs::remove_file(&tmp);
                    validated = true;
                    break;
                }
                Err(_) => {
                    let _ = fs::remove_file(&tmp);
                    continue;
                }
            }
        } else {
            if decrypt_file_from_enc(&aead, enc_path, &dst).is_ok() {
                // we wrote one successfully; treat as validated.
                // (If you prefer not to write anything until config is saved, you could write to tmp and rename after persist.)
                validated = true;
                break;
            } else {
                // cleanup if partial file created
                let _ = fs::remove_file(&dst);
            }
        }
    }

    if !validated {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "provided key did not decrypt any .env*.enc"));
    }

    // 4) Persist config with this key & ensure .gitignore includes it
    write_eenv_config_with_key(repo_root, &key_str)?;
    ensure_gitignore_has_config(repo_root)?;

    // 5) With config persisted, decrypt all remaining (non-clobbering)
    //    Reuse your normal decrypt workflow; it already skips if target exists.
    handle_enc_workflow(repo_root)
}

/// Get staged file paths (relative to repo root)
fn staged_files(repo_root: &Path) -> io::Result<Vec<PathBuf>> {
    let out = Proc::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("diff")
        .arg("--name-only")
        .arg("--cached")
        .arg("-z")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "git diff failed"));
    }
    let mut files = Vec::new();
    for name in out.stdout.split(|b| *b == 0u8) {
        if name.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(name);
        files.push(repo_root.join(s.as_ref()));
    }
    Ok(files)
}

fn git_add(repo_root: &Path, paths: &[PathBuf]) -> io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cmd = Proc::new("git");
    cmd.arg("-C").arg(repo_root).arg("add").arg("--");
    for p in paths {
        cmd.arg(p);
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "git add failed"));
    }
    Ok(())
}

/// Stub: produce `.env*.enc` from `real` envs using decryption key.
/// Return the list of generated/updated paths so the hook can `git add` them.
fn encrypt_envs_to_enc(repo_root: &Path, real_envs: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let key = read_eenv_key(repo_root)?;
    let aead = XChaCha20Poly1305::new((&key).into());

    let mut produced = Vec::new();
    for src in real_envs {
        let Some(name) = src.file_name().and_then(|s| s.to_str()) else { continue; };
        // Only encrypt real envs (not examples, not already .enc)
        if name.ends_with(".example") || name.ends_with(".enc") {
            continue;
        }
        let dst = enc_output_path(src);
        encrypt_file_to_enc(&aead, src, &dst)?;
        println!("[enc] wrote {}", dst.display());
        produced.push(dst);
    }
    Ok(produced)
}

fn read_eenv_key(repo_root: &Path) -> io::Result<[u8; 32]> {
    let cfg_path = eenv_config_path(repo_root);
    let text = fs::read_to_string(&cfg_path)?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad eenv.config.json: {e}")))?;
    let key_str = v.get("key")
        .and_then(|x| x.as_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "eenv.config.json missing non-empty \"key\""))?
        .trim()
        .to_string();
    if key_str.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty key"));
    }
    // Derive a stable 32-byte key from the user string (works with your current 44-char key)
    let hash = blake3::hash(key_str.as_bytes());
    Ok(*hash.as_bytes()) // [u8;32]
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp~");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

// Timing helpers
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
