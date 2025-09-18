use crate::config::{ensure_gitignore_has_config, read_eenv_key, write_eenv_config_with_key};
use crate::envscan::{find_env_files_recursive, split_env_files};
use crate::util::write_bytes_atomic;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::Rng;
use std::{fs, io, path::Path};

pub const MAGIC: &[u8; 5] = b"EENV1";

pub fn enc_output_path(input: &std::path::Path) -> std::path::PathBuf {
    let mut name = input
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    name.push_str(".enc");
    input.with_file_name(name)
}

pub fn dec_output_path(input_enc: &std::path::Path) -> std::path::PathBuf {
    let name = input_enc.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if let Some(stripped) = name.strip_suffix(".enc") {
        input_enc.with_file_name(stripped)
    } else {
        input_enc.with_file_name(name)
    }
}

pub fn encrypt_file_to_enc(aead: &XChaCha20Poly1305, src: &Path, dst: &Path) -> io::Result<()> {
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

pub fn decrypt_file_from_enc(
    aead: &XChaCha20Poly1305,
    src_enc: &Path,
    dst: &Path,
) -> io::Result<()> {
    let data = fs::read(src_enc)?;
    if data.len() < MAGIC.len() + 24 + 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "enc file too short",
        ));
    }
    if &data[..MAGIC.len()] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad magic/version",
        ));
    }
    let nonce_bytes = &data[MAGIC.len()..MAGIC.len() + 24];
    let nonce = XNonce::from_slice(nonce_bytes);
    let ciphertext = &data[MAGIC.len() + 24..];
    let plaintext = aead
        .decrypt(nonce, ciphertext)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decrypt failed (wrong key?)"))?;
    write_bytes_atomic(dst, &plaintext)
}

pub fn handle_enc_workflow(repo_root: &Path) -> io::Result<()> {
    let key = read_eenv_key(repo_root)?;
    let aead = XChaCha20Poly1305::new((&key).into());

    let files = find_env_files_recursive(repo_root)?;
    let (_real, _examples, encs) = split_env_files(files);

    for enc_path in encs {
        let dst = dec_output_path(&enc_path);
        if dst.exists() {
            eprintln!("[enc] skip decrypt (target exists): {}", dst.display());
            continue;
        }
        match decrypt_file_from_enc(&aead, &enc_path, &dst) {
            Ok(()) => println!(
                "[enc] decrypted {} -> {}",
                enc_path.display(),
                dst.display()
            ),
            Err(e) => eprintln!(
                "[enc] WARN: could not decrypt {} ({})",
                enc_path.display(),
                e
            ),
        }
    }
    Ok(())
}

pub fn encrypt_envs_to_enc(
    repo_root: &Path,
    real_envs: &[std::path::PathBuf],
) -> io::Result<Vec<std::path::PathBuf>> {
    let key = read_eenv_key(repo_root)?;
    let aead = XChaCha20Poly1305::new((&key).into());
    let mut produced = Vec::new();
    for src in real_envs {
        let Some(name) = src.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
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

pub fn aead_from_key_str(key_str: &str) -> io::Result<XChaCha20Poly1305> {
    if key_str.trim().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty key"));
    }
    let hash = blake3::hash(key_str.as_bytes());
    Ok(XChaCha20Poly1305::new(hash.as_bytes().into()))
}

// bootstrap flow
pub fn bootstrap_key_and_decrypt(repo_root: &Path) -> io::Result<()> {
    let key_str = crate::config::prompt_for_key()?;
    let aead = aead_from_key_str(&key_str)?;

    let files = find_env_files_recursive(repo_root)?;
    let (_real, _examples, encs) = split_env_files(files);
    if encs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no .env*.enc files found",
        ));
    }

    let mut validated = false;
    for enc_path in &encs {
        let dst = dec_output_path(enc_path);
        if dst.exists() {
            let tmp = dst.with_extension("validate.tmp~");
            match decrypt_file_from_enc(&aead, enc_path, &tmp) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&tmp);
                    validated = true;
                    break;
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&tmp);
                    continue;
                }
            }
        } else {
            if decrypt_file_from_enc(&aead, enc_path, &dst).is_ok() {
                validated = true;
                break;
            } else {
                let _ = std::fs::remove_file(&dst);
            }
        }
    }

    if !validated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provided key did not decrypt any .env*.enc",
        ));
    }

    write_eenv_config_with_key(repo_root, &key_str)?;
    ensure_gitignore_has_config(repo_root)?;
    handle_enc_workflow(repo_root)
}
