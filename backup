 Command::init => {
            //let files = find_env_files_in_cwd()?;

            //let files = find_env_files_recursive(&cwd)?;
            //let (real, examples) = split_env_files(files);

            let cwd = std::env::current_dir()?;
            let repo_root = find_repo_root(&cwd)?;

            // Pure detection first
            let state = compute_eenv_state(&repo_root)?;
            println!(
                "[state] enc={}, example={}, env={}, eenvjson={}",
                state.enc, state.example, state.env, state.eenvjson
            );

            /*
            match ensure_eenv_config(&repo_root) {
                Ok(ConfigStatus::Created) => {
                    println!("[init] created eenv.config.json in {}", repo_root.display())
                }
                Ok(ConfigStatus::Valid) => {
                    println!("[init] using existing eenv.config.json (valid)")
                }
                Ok(ConfigStatus::FixedMissingKey) => {
                    println!("[init] repaired eenv.config.json: inserted missing key")
                }
                Ok(ConfigStatus::RewrittenFromInvalid { backup }) => println!(
                    "[init] eenv.config.json was invalid JSON -> replaced (backup: {})",
                    backup.display()
                ),
                Err(e) => {
                    eprintln!("[error] could not ensure eenv.config.json: {e}");
                    std::process::exit(1);
                }
            }

            // find files (fallible)
            // only continue if the config exists
            let (files, _t_find) = time_result("find_env_files_recursive", || {
                find_env_files_recursive(&repo_root)
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

            // fix .gitignore
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
            */
        }
    }
