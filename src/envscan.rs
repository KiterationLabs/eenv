use crate::config::validate_eenv_config;
use crate::types::EenvState;
use ignore::{DirEntry, WalkBuilder};
use std::{io, path::Path, path::PathBuf};

pub fn find_env_files_recursive(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .follow_links(false)
        .standard_filters(false)
        .parents(false)
        .add_custom_ignore_filename(".eenvignore")
        .filter_entry(|d| d.depth() == 0 || true);

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

pub fn split_env_files(mut files: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
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
    matches!(d.path().file_name().and_then(|s| s.to_str()), Some(name) if name.starts_with(".env"))
}

pub fn compute_eenv_state(repo_root: &Path) -> io::Result<EenvState> {
    let files = find_env_files_recursive(repo_root)?;
    let (real, examples, encs) = split_env_files(files);
    let enc = !encs.is_empty();
    let example = !examples.is_empty();
    let env = !real.is_empty();
    let eenvjson = validate_eenv_config(repo_root)?;
    Ok(EenvState {
        enc,
        example,
        env,
        eenvjson,
    })
}
