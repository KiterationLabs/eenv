use std::io;
use std::path::Path;

pub fn run(repo_root: &Path) -> io::Result<()> {
    let state = crate::envscan::compute_eenv_state(repo_root)?;
    println!("[state]");
    println!("enc      = {}", state.enc);
    println!("example  = {}", state.example);
    println!("env      = {}", state.env);
    println!("eenvjson = {}", state.eenvjson);
    println!("-----------------");

    if state.enc {
        if state.eenvjson {
            if let Err(e) = crate::crypto::handle_enc_workflow(repo_root) {
                eprintln!("[enc] error: {e}");
            }
        } else {
            match crate::crypto::bootstrap_key_and_decrypt(repo_root) {
                Ok(()) => {
                    eprintln!("[enc] key accepted, config created, decrypted where possible.")
                }
                Err(e) => {
                    eprintln!("[enc] could not bootstrap from key: {e}");
                    return Err(e);
                }
            }
        }
    }

    if state.env {
        let (files, _t_find) = crate::util::time_result("find_env_files_recursive", || {
            crate::envscan::find_env_files_recursive(repo_root)
        })?;
        let ((real, examples, encs), _t_split) =
            crate::util::time_ok("split_env_files", move || {
                crate::envscan::split_env_files(files)
            });

        println!("--- real env files ---");
        for p in &real {
            println!("{}", p.display());
        }
        println!("--- example env files ---");
        for p in &examples {
            println!("{}", p.display());
        }
        println!("--- encrypted env files ---");
        for p in &encs {
            println!("{}", p.display());
        }

        if !state.example && !real.is_empty() {
            let skeletons = crate::examples::extract_env_skeletons(&real)?;
            if let Ok(actions) = crate::examples::ensure_env_examples_from_skeletons(&skeletons) {
                for (src, dst, action) in actions {
                    let label = match action {
                        crate::examples::ExampleAction::Created => "created",
                        crate::examples::ExampleAction::Overwritten => "overwritten",
                        crate::examples::ExampleAction::SourceIsExample => "skip",
                    };
                    println!(
                        "[env-example] {:<11} {}  ->  {}",
                        label,
                        src.display(),
                        dst.display()
                    );
                }
            }
        }

        match crate::gitignore::fix_gitignore_from_found(repo_root, &real) {
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
        for p in &produced {
            println!("[init] encrypted -> {}", p.display());
        }
    }

    // also make sure we ignore generated hooks if hooks path is inside the repo
    let _ = crate::hooks::ensure_gitignore_ignores_hooks(repo_root);
    Ok(())
}
