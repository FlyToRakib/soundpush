//! Authenticated encryption for data at rest, plus atomic file writes.
//!
//! File format: `MAGIC (5) | nonce (12) | ChaCha20-Poly1305 ciphertext`.
//! The 32-byte key comes from the platform keystore (DPAPI, Keychain,
//! Secret Service, Android Keystore) via the engine's platform hooks.

use std::fs;
use std::io::Write;
use std::path::Path;

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;

use crate::SecurityError;

const MAGIC: &[u8; 5] = b"SPSB1";
const NONCE_LEN: usize = 12;

pub fn seal(key: &[u8; 32], associated: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + plaintext.len() + 16);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    let mut body = plaintext.to_vec();
    // In-memory encryption with a valid key and nonce cannot fail.
    if cipher
        .encrypt_in_place(Nonce::from_slice(&nonce), associated, &mut body)
        .is_err()
    {
        body.clear();
    }
    out.extend_from_slice(&body);
    out
}

pub fn open(key: &[u8; 32], associated: &[u8], sealed: &[u8]) -> Result<Vec<u8>, SecurityError> {
    if sealed.len() < MAGIC.len() + NONCE_LEN || &sealed[..MAGIC.len()] != MAGIC {
        return Err(SecurityError::Corrupted);
    }
    let (nonce, ciphertext) = sealed[MAGIC.len()..].split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut body = ciphertext.to_vec();
    cipher
        .decrypt_in_place(Nonce::from_slice(nonce), associated, &mut body)
        .map_err(|_| SecurityError::Corrupted)?;
    Ok(body)
}

/// Write `data` to `path` atomically (temp file + fsync + rename).
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), SecurityError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    restrict_permissions(&tmp)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> std::io::Result<()> {
    // Windows: files under %LOCALAPPDATA% inherit a per-user ACL.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_and_tamper_detection() {
        let key = [3u8; 32];
        let sealed = seal(&key, b"trust", b"secret data");
        assert_eq!(open(&key, b"trust", &sealed).unwrap(), b"secret data");
        assert!(open(&[4u8; 32], b"trust", &sealed).is_err());
        assert!(open(&key, b"other", &sealed).is_err());
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open(&key, b"trust", &tampered).is_err());
    }
}
