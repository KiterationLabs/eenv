use std::process::Command as Proc;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const HOOK_MARKER: &str = "# managed-by-eenv";

pub fn git_hooks_dir(repo_root: &Path) -> io::Result<PathBuf> {
    let out = Proc::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--git-path")
        .arg("hooks")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "git rev-parse failed"));
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(p))
}

#[allow(dead_code)]
fn backup_path(p: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    p.with_extension(format!("bak.{ts}"))
}

pub fn install_git_hook(repo_root: &Path, force: bool) -> io::Result<()> {
    // ensure it's a repo
    let status = Proc::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--git-dir")
        .status()?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "not a git repo"));
    }

    let hooks_dir = git_hooks_dir(repo_root)?;
    fs::create_dir_all(&hooks_dir)?;
    let sh_path = hooks_dir.join("pre-commit");
    let ps1_path = hooks_dir.join("pre-commit.ps1");

    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy();

    let sh_content = format!(
        r#"#!/usr/bin/env bash
{marker}
set -euo pipefail
exec "{exe}" pre-commit --write
"#,
        marker = HOOK_MARKER,
        exe = exe_str
    );

    let ps1_content = format!(
        r#"{marker}
$ErrorActionPreference = "Stop"
& "{exe}" pre-commit --write
exit $LASTEXITCODE
"#,
        marker = HOOK_MARKER,
        exe = exe_str
    );

    fn write_if_needed(path: &Path, desired: &str, force: bool) -> io::Result<bool> {
        match fs::read_to_string(path) {
            Ok(existing) => {
                let ours = existing.contains(HOOK_MARKER);
                if !ours && !force {
                    return Ok(false);
                }
                if existing != desired {
                    if !ours && force {
                        let bak = super::util::backup_path_with_ts(path);
                        fs::copy(path, &bak).ok();
                    }
                    super::util::write_string_atomic(path, desired)?;
                    return Ok(true);
                }
                Ok(false)
            }
            Err(_) => {
                super::util::write_string_atomic(path, desired)?;
                Ok(true)
            }
        }
    }

    let _ = write_if_needed(&sh_path, &sh_content, force)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if sh_path.exists() {
            let mut perm = fs::metadata(&sh_path)?.permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&sh_path, perm)?;
        }
    }
    let _ = write_if_needed(&ps1_path, &ps1_content, force)?;

    let _ = self::ensure_gitignore_ignores_hooks(repo_root);
    Ok(())
}

pub fn uninstall_git_hook(repo_root: &Path, force: bool) -> io::Result<()> {
    let hooks_dir = git_hooks_dir(repo_root)?;
    for name in ["pre-commit", "pre-commit.ps1"] {
        let p = hooks_dir.join(name);
        if !p.exists() {
            continue;
        }
        if force {
            let _ = fs::remove_file(&p);
            continue;
        }
        if let Ok(existing) = fs::read_to_string(&p) {
            if existing.contains(HOOK_MARKER) {
                let _ = fs::remove_file(&p);
            }
        }
    }
    Ok(())
}

pub fn ensure_gitignore_ignores_hooks(repo_root: &Path) -> io::Result<()> {
    // Where git currently stores hooks (respects core.hooksPath)
    let hooks_dir = git_hooks_dir(repo_root)?;

    // If hooks dir isn’t in the repo worktree (e.g., a global path), nothing to ignore.
    let rel = match hooks_dir.strip_prefix(repo_root) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    // If it’s under .git/, it’s already untracked.
    if rel.components().next().map(|c| c.as_os_str()) == Some(std::ffi::OsStr::new(".git")) {
        return Ok(());
    }

    // Specific hook files we manage
    let pre_commit = rel.join("pre-commit");
    let pre_commit_ps1 = rel.join("pre-commit.ps1");

    let gi_path = repo_root.join(".gitignore");
    let original = if gi_path.exists() {
        fs::read_to_string(&gi_path)?
    } else {
        String::new()
    };
    let mut lines: Vec<String> = if original.is_empty() {
        Vec::new()
    } else {
        original.lines().map(|s| s.to_string()).collect()
    };

    // Helper: strip comments and whitespace
    fn core(s: &str) -> &str {
        let mut c = s;
        if let Some(i) = s.find('#') {
            c = &s[..i];
        }
        c.trim()
    }
    let existing: std::collections::HashSet<String> =
        lines.iter().map(|l| core(l).to_string()).collect();

    let mut to_add: Vec<String> = Vec::new();
    for p in [&pre_commit, &pre_commit_ps1] {
        let pat = p.to_string_lossy().replace('\\', "/");
        if !existing.contains(&pat) {
            to_add.push(pat);
        }
    }
    if to_add.is_empty() {
        return Ok(());
    }

    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
        lines.push(String::new());
    }
    lines.push("# added by eenv (ignore generated git hooks)".to_string());
    lines.extend(to_add);

    let mut s = lines.join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    super::util::write_string_atomic(&gi_path, &s)?;
    Ok(())
}
