use std::process::Command as Proc;
use std::{io, path::Path, path::PathBuf};

pub fn pre_commit(repo_root: &Path, write: bool) -> io::Result<()> {
    let staged = staged_files(repo_root)?;
    let mut offenders = Vec::new();
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

    let (files, _t_find) = crate::util::time_result("find_env_files_recursive", || {
        crate::envscan::find_env_files_recursive(repo_root)
    })?;
    let ((real, _examples, _encs), _t_split) =
        crate::util::time_ok("split_env_files", || crate::envscan::split_env_files(files));

    if write && !real.is_empty() {
        let skeletons = crate::examples::extract_env_skeletons(&real)?;
        if let Ok(actions) = crate::examples::ensure_env_examples_from_skeletons(&skeletons) {
            let mut to_add = Vec::new();
            for (_src, dst, action) in actions {
                match action {
                    crate::examples::ExampleAction::Created
                    | crate::examples::ExampleAction::Overwritten => to_add.push(dst),
                    crate::examples::ExampleAction::SourceIsExample => {}
                }
            }
            if !to_add.is_empty() {
                git_add(repo_root, &to_add)?;
            }
        }
    }

    if write && !real.is_empty() {
        match crate::gitignore::fix_gitignore_from_found(repo_root, &real) {
            Ok(report) => {
                if report.changed {
                    git_add(repo_root, &[report.path])?;
                }
            }
            Err(e) => eprintln!("[pre-commit] gitignore fix error: {e}"),
        }
    }

    if write && !real.is_empty() {
        match crate::config::ensure_eenv_config(repo_root) {
            Ok(crate::config::ConfigStatus::Created) => {
                eprintln!("[config] created eenv.config.json")
            }
            Ok(crate::config::ConfigStatus::FixedMissingKey) => {
                eprintln!("[config] injected key into eenv.config.json")
            }
            Ok(crate::config::ConfigStatus::RewrittenFromInvalid { backup }) => eprintln!(
                "[config] repaired eenv.config.json (backup: {})",
                backup.display()
            ),
            Ok(crate::config::ConfigStatus::Valid) => {}
            Err(e) => eprintln!("[config] error: {e}"),
        }

        let produced = crate::crypto::encrypt_envs_to_enc(repo_root, &real)?;
        if !produced.is_empty() {
            git_add(repo_root, &produced)?;
        }
    }

    Ok(())
}

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
