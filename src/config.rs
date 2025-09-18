use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn eenv_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join("eenv.config.json")
}

pub fn prompt_for_key() -> io::Result<String> {
    use std::io::Write;
    print!("eenv: existing eenv.config.json is invalid.\nEnter key to use: ");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    let key = s.trim().to_string();
    if key.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty key not allowed",
        ));
    }
    Ok(key)
}

pub fn validate_eenv_config(repo_root: &Path) -> io::Result<bool> {
    let path = eenv_config_path(repo_root);
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) if v.is_object() => {
            Ok(matches!(v.get("key"), Some(serde_json::Value::String(s)) if !s.is_empty()))
        }
        _ => Ok(false),
    }
}

#[derive(Debug)]
pub enum ConfigStatus {
    Created,
    Valid,
    FixedMissingKey,
    RewrittenFromInvalid { backup: PathBuf },
}

pub fn ensure_eenv_config(repo_root: &Path) -> io::Result<ConfigStatus> {
    let path = eenv_config_path(repo_root);

    if !path.exists() {
        let key = super::util::generate_key();
        let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key);
        super::util::write_string_atomic(&path, &pretty)?;
        return Ok(ConfigStatus::Created);
    }

    let text = fs::read_to_string(&path)?;
    match serde_json::from_str::<Value>(&text) {
        Ok(mut v) => {
            if !v.is_object() {
                let backup = super::util::backup_path_with_ts(&path);
                super::util::write_string_atomic(&backup, &text)?;
                let key = super::util::generate_key();
                let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key);
                super::util::write_string_atomic(&path, &pretty)?;
                return Ok(ConfigStatus::RewrittenFromInvalid { backup });
            }

            let needs_key = match v.get("key") {
                Some(Value::String(s)) => s.is_empty(),
                _ => true,
            };

            if needs_key {
                let key = super::util::generate_key();
                v.as_object_mut()
                    .unwrap()
                    .insert("key".into(), Value::String(key));
                let mut pretty = serde_json::to_string_pretty(&v)
                    .unwrap_or_else(|_| json!({ "key": super::util::generate_key() }).to_string());
                if !pretty.ends_with('\n') {
                    pretty.push('\n');
                }
                super::util::write_string_atomic(&path, &pretty)?;
                Ok(ConfigStatus::FixedMissingKey)
            } else {
                Ok(ConfigStatus::Valid)
            }
        }
        Err(_) => {
            let key = prompt_for_key()?;
            let backup = super::util::backup_path_with_ts(&path);
            super::util::write_string_atomic(&backup, &text)?;
            let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key);
            super::util::write_string_atomic(&path, &pretty)?;
            Ok(ConfigStatus::RewrittenFromInvalid { backup })
        }
    }
}

pub fn write_eenv_config_with_key(repo_root: &Path, key_str: &str) -> io::Result<()> {
    let path = eenv_config_path(repo_root);
    let pretty = format!("{{\n  \"key\": \"{}\"\n}}\n", key_str);
    super::util::write_string_atomic(&path, &pretty)
}

pub fn ensure_gitignore_has_config(repo_root: &Path) -> io::Result<()> {
    let root = super::util::find_repo_root(repo_root)?;
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

    let already = lines
        .iter()
        .any(|l| super::gitignore::pattern_core(l) == "eenv.config.json");
    if !already {
        if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
            lines.push(String::new());
        }
        lines.push("# added by eenv".to_string());
        lines.push("eenv.config.json".to_string());
        let mut s = lines.join("\n");
        if !s.ends_with('\n') {
            s.push('\n');
        }
        let tmp = path.with_extension("tmp~");
        {
            let mut f = std::fs::File::create(&tmp)?;
            use std::io::Write;
            f.write_all(s.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(tmp, &path)?;
    }
    Ok(())
}

pub fn read_eenv_key(repo_root: &Path) -> io::Result<[u8; 32]> {
    let cfg_path = eenv_config_path(repo_root);
    let text = fs::read_to_string(&cfg_path)?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad eenv.config.json: {e}"),
        )
    })?;
    let key_str = v
        .get("key")
        .and_then(|x| x.as_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "eenv.config.json missing non-empty \"key\"",
            )
        })?
        .trim()
        .to_string();
    if key_str.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty key"));
    }
    let hash = blake3::hash(key_str.as_bytes());
    Ok(*hash.as_bytes())
}
