use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct GitignoreEdit {
    pub path: PathBuf,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: bool,
}

pub fn pattern_core(line: &str) -> &str {
    let mut core = line;
    if let Some(hash) = line.find('#') {
        core = &line[..hash];
    }
    core.trim()
}

fn banned_env_ignores() -> &'static [&'static str] {
    &[
        ".env.example",
        ".env*.example",
        ".env.*.example",
        "*.env.example",
        ".env.enc",
        ".env*.enc",
        ".env.*.enc",
        "*.env.enc",
    ]
}

fn to_gitignore_rel_pattern(abs: &Path, root: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    Some(if s.is_empty() {
        String::from("/")
    } else {
        s.replace(' ', r"\ ")
    })
}

pub fn fix_gitignore_from_found(
    project_root: &Path,
    real_env_files: &[PathBuf],
) -> io::Result<GitignoreEdit> {
    let root = super::util::find_repo_root(project_root)?;
    let path = root.join(".gitignore");

    let original = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let mut lines: Vec<String> = if original.is_empty() {
        Vec::new()
    } else {
        original.lines().map(|s| s.to_string()).collect()
    };

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

    let mut required: BTreeSet<String> = BTreeSet::new();
    for abs in real_env_files {
        let Some(fname) = abs.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if fname.ends_with(".example") || fname.ends_with(".enc") {
            continue;
        }
        if let Some(pat) = to_gitignore_rel_pattern(abs, &root) {
            required.insert(pat);
        }
    }
    required.insert("eenv.config.json".to_string());

    let existing: HashSet<String> = lines.iter().map(|l| pattern_core(l).to_string()).collect();
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
        fs::rename(tmp, &path)?;
    }

    Ok(GitignoreEdit {
        path,
        added,
        removed,
        changed,
    })
}
