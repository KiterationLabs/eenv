use std::fs::File;
use std::io::{BufRead, BufReader};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum ExampleAction {
    Created,
    Overwritten,
    SourceIsExample,
}

pub fn extract_env_skeletons(files: &[PathBuf]) -> io::Result<HashMap<PathBuf, Vec<String>>> {
    let mut out = HashMap::new();
    for path in files {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                lines.push(String::new());
            } else if trimmed.starts_with('#') {
                lines.push(line);
            } else if let Some((key, _value)) = line.split_once('=') {
                lines.push(format!("{}=", key.trim()));
            } else {
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

pub fn ensure_env_examples_from_skeletons(
    skeletons: &HashMap<PathBuf, Vec<String>>,
) -> io::Result<Vec<(PathBuf, PathBuf, ExampleAction)>> {
    let mut results = Vec::new();
    for (real_path, lines) in skeletons {
        let target = example_path_for(real_path);
        if real_path == &target {
            results.push((real_path.clone(), target, ExampleAction::SourceIsExample));
            continue;
        }
        let existed = target.exists();
        super::util::write_lines_atomic(&target, lines)?;
        let action = if existed {
            ExampleAction::Overwritten
        } else {
            ExampleAction::Created
        };
        results.push((real_path.clone(), target, action));
    }
    Ok(results)
}
