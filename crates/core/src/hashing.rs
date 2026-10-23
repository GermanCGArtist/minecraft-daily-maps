//! BLAKE3 hashing. Much faster than SHA-256 and, for big files, it splits the work over all
//! CPU cores. Hashes are only used to compare files with each other, never for security.

use crate::error::Result;
use std::{fs::File, io::Read, path::Path};

/// Hash a small or medium file on the calling thread (many of these run in parallel,
/// one per region or tile file).
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Hash one huge file (the world zip) using all cores via a memory map.
/// Falls back to the plain single-thread read if mapping fails.
pub fn hash_file_parallel(path: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    match hasher.update_mmap_rayon(path) {
        Ok(_) => Ok(hasher.finalize().to_hex().to_string()),
        Err(_) => hash_file(path),
    }
}

pub fn hash_bytes(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}
