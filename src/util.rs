use rand::{Rng, distr::Alphanumeric};
use std::fs::File;
use std::io::Write;
use std::{
    fs, io,
    path::Path,
    path::PathBuf,
    time::{Duration, Instant},
};

pub fn find_repo_root(start: &Path) -> io::Result<PathBuf> {
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

pub fn write_string_atomic(path: &Path, contents: &str) -> io::Result<()> {
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

pub fn write_lines_atomic(path: &Path, lines: &[String]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut buf = lines.join("\n");
    if !buf.ends_with('\n') {
        buf.push('\n');
    }
    let tmp = path.with_extension("example.tmp~");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(buf.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
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

pub fn backup_path_with_ts(p: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    p.with_extension(format!("bak.{ts}"))
}

pub fn generate_key() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(44)
        .map(char::from)
        .collect()
}

// timing helpers
pub fn time_result<F, T, E>(label: &str, f: F) -> Result<(T, Duration), E>
where
    F: FnOnce() -> Result<T, E>,
{
    let start = Instant::now();
    let out = f()?;
    let dt = start.elapsed();
    eprintln!("[time] {label}: {:.3} ms", dt.as_secs_f64() * 1000.0);
    Ok((out, dt))
}

pub fn time_ok<F, T>(label: &str, f: F) -> (T, Duration)
where
    F: FnOnce() -> T,
{
    let start = std::time::Instant::now();
    let out = f();
    let dt = start.elapsed();
    eprintln!("[time] {label}: {:.3} ms", dt.as_secs_f64() * 1000.0);
    (out, dt)
}
