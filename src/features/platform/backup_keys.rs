//! H10 versioned backup keys and rotation.
//!
//! Backups use authenticated `DOMBAK2` envelopes for existing files and
//! chunked `DOMBAK3` envelopes for bounded-memory writes. Rotation creates a
//! new key, re-encrypts every existing `.enc` backup atomically, and only then
//! makes the new version active. Old keys are removed only after a successful
//! re-encryption unless the caller explicitly retains them.
use crate::state::AppState;
use chacha20poly1305::{
    aead::{stream::{DecryptorBE32, EncryptorBE32}, Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 7] = b"DOMBAK2";
const STREAM_MAGIC: &[u8; 7] = b"DOMBAK3";
const KEY_LEN: usize = 32;
const STREAM_CHUNK_BYTES: usize = 1024 * 1024;
const STREAM_HEADER_BYTES: usize = 7 + 4 + 7 + 8;

struct RemoveFileOnDrop(PathBuf);

impl Drop for RemoveFileOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct RemoveFilesOnDrop(Vec<PathBuf>);

impl Drop for RemoveFilesOnDrop {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Clone)]
pub struct KeyRing {
    active: u32,
    keys: BTreeMap<u32, [u8; KEY_LEN]>,
    directory: PathBuf,
}

fn key_file(directory: &Path, version: u32) -> PathBuf {
    directory.join(format!("key-{version}.key"))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn read_key(path: &Path) -> Option<[u8; KEY_LEN]> {
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::with_capacity(KEY_LEN + 1);
    file.take((KEY_LEN + 1) as u64).read_to_end(&mut bytes).ok()?;
    if bytes.len() != KEY_LEN {
        return None;
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Some(key)
}

impl KeyRing {
    pub fn load_or_create(data_dir: &Path) -> Result<Self, String> {
        let directory = data_dir.join("backup-keys");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let mut keys = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&directory) {
            for entry in entries.filter_map(|entry| entry.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(version) = name
                    .strip_prefix("key-")
                    .and_then(|rest| rest.strip_suffix(".key"))
                    .and_then(|value| value.parse::<u32>().ok())
                else {
                    continue;
                };
                if let Some(key) = read_key(&entry.path()) {
                    keys.insert(version, key);
                }
            }
        }
        if keys.is_empty() {
            let mut key = [0u8; KEY_LEN];
            rand::thread_rng().fill_bytes(&mut key);
            write_private(&key_file(&directory, 1), &key)?;
            keys.insert(1, key);
        }
        let active = crate::storage::bounded_file::read_utf8(directory.join("active"), 4096)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|version| keys.contains_key(version))
            .unwrap_or_else(|| *keys.keys().next_back().unwrap_or(&1));
        let _ = std::fs::write(directory.join("active"), active.to_string());
        Ok(Self {
            active,
            keys,
            directory,
        })
    }

    pub fn active_version(&self) -> u32 {
        self.active
    }

    pub fn versions(&self) -> Vec<u32> {
        self.keys.keys().cloned().collect()
    }

    fn cipher(key: &[u8; KEY_LEN]) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(key.into())
    }

    fn encrypt_with(&self, version: u32, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let key = self
            .keys
            .get(&version)
            .ok_or_else(|| format!("backup key version {version} is missing"))?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = Self::cipher(key)
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
            .map_err(|_| "backup encryption failed".to_string())?;
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        self.encrypt_with(self.active, plaintext)
    }

    /// Writes a version 3 backup envelope with authenticated 1 MiB chunks.
    /// Peak working memory is bounded independently of the snapshot size.
    pub fn encrypt_reader(
        &self,
        reader: &mut impl Read,
        plaintext_len: u64,
        writer: &mut impl Write,
    ) -> Result<u64, String> {
        let key = self.keys.get(&self.active)
            .ok_or_else(|| format!("backup key version {} is missing", self.active))?;
        let mut nonce_bytes = [0u8; 7];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = chacha20poly1305::aead::generic_array::GenericArray::clone_from_slice(&nonce_bytes);
        let mut header = STREAM_MAGIC.to_vec();
        header.extend_from_slice(&self.active.to_le_bytes());
        header.extend_from_slice(&nonce_bytes);
        header.extend_from_slice(&plaintext_len.to_le_bytes());
        writer.write_all(&header).map_err(|error| error.to_string())?;

        let mut encryptor = EncryptorBE32::from_aead(Self::cipher(key), &nonce);
        let chunk_count = if plaintext_len == 0 {
            1
        } else {
            (plaintext_len - 1) / STREAM_CHUNK_BYTES as u64 + 1
        };
        if chunk_count > u32::MAX as u64 {
            return Err("backup exceeds the streaming encryption limit".into());
        }
        let mut buffer = vec![0u8; STREAM_CHUNK_BYTES];
        let mut remaining = plaintext_len;
        let mut written = header.len() as u64;
        while remaining > STREAM_CHUNK_BYTES as u64 {
            let length = STREAM_CHUNK_BYTES;
            reader.read_exact(&mut buffer[..length]).map_err(|error| error.to_string())?;
            let payload = Payload { msg: &buffer[..length], aad: &header };
            let ciphertext = encryptor.encrypt_next(payload)
                .map_err(|_| "backup encryption failed".to_string())?;
            writer.write_all(&ciphertext).map_err(|error| error.to_string())?;
            written = written.checked_add(ciphertext.len() as u64)
                .ok_or_else(|| "backup size overflow".to_string())?;
            remaining -= length as u64;
        }
        let length = remaining as usize;
        reader.read_exact(&mut buffer[..length]).map_err(|error| error.to_string())?;
        let ciphertext = encryptor.encrypt_last(Payload { msg: &buffer[..length], aad: &header })
            .map_err(|_| "backup encryption failed".to_string())?;
        writer.write_all(&ciphertext).map_err(|error| error.to_string())?;
        written = written.checked_add(ciphertext.len() as u64)
            .ok_or_else(|| "backup size overflow".to_string())?;
        let mut extra = [0u8; 1];
        if reader.read(&mut extra).map_err(|error| error.to_string())? != 0 {
            return Err("backup source grew while it was being encrypted".into());
        }
        writer.flush().map_err(|error| error.to_string())?;
        Ok(written)
    }

    pub fn is_versioned(data: &[u8]) -> bool {
        data.starts_with(MAGIC) || data.starts_with(STREAM_MAGIC)
    }

    pub fn is_versioned_file(path: &Path) -> std::io::Result<bool> {
        let mut file = std::fs::File::open(path)?;
        let mut prefix = [0u8; MAGIC.len()];
        let mut read = 0;
        while read < prefix.len() {
            let count = file.read(&mut prefix[read..])?;
            if count == 0 {
                break;
            }
            read += count;
        }
        Ok(read == prefix.len() && (&prefix == MAGIC || &prefix == STREAM_MAGIC))
    }

    pub fn is_stream_file(path: &Path) -> std::io::Result<bool> {
        let mut file = std::fs::File::open(path)?;
        let mut prefix = [0u8; STREAM_MAGIC.len()];
        let mut read = 0;
        while read < prefix.len() {
            let count = file.read(&mut prefix[read..])?;
            if count == 0 {
                break;
            }
            read += count;
        }
        Ok(read == prefix.len() && &prefix == STREAM_MAGIC)
    }

    pub fn decrypt_stream_file_to(&self, source: &Path, target: &Path) -> Result<u64, String> {
        let mut reader = std::fs::File::open(source).map_err(|error| error.to_string())?;
        let metadata = reader.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("backup source is not a regular file".into());
        }
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|error| error.to_string())?;
        let plaintext_len = match self.decrypt_stream_reader(&mut reader, metadata.len(), &mut writer) {
            Ok(length) => length,
            Err(error) => {
                drop(writer);
                let _ = std::fs::remove_file(target);
                return Err(error);
            }
        };
        if let Err(error) = writer.sync_all() {
            drop(writer);
            let _ = std::fs::remove_file(target);
            return Err(error.to_string());
        }
        Ok(plaintext_len)
    }

    pub fn decrypt(&self, data: &[u8]) -> Option<Vec<u8>> {
        if data.starts_with(STREAM_MAGIC) {
            return self.decrypt_stream(data);
        }
        if data.len() < MAGIC.len() + 4 + 12 + 16 || !Self::is_versioned(data) {
            return None;
        }
        let version = u32::from_le_bytes(data[7..11].try_into().ok()?);
        let key = self.keys.get(&version)?;
        let nonce = Nonce::from_slice(&data[11..23]);
        Self::cipher(key).decrypt(nonce, &data[23..]).ok()
    }

    fn decrypt_stream(&self, data: &[u8]) -> Option<Vec<u8>> {
        let mut reader = std::io::Cursor::new(data);
        let mut plaintext = Vec::new();
        self.decrypt_stream_reader(&mut reader, data.len() as u64, &mut plaintext).ok()?;
        Some(plaintext)
    }

    fn decrypt_stream_reader(
        &self,
        reader: &mut impl Read,
        sealed_len: u64,
        writer: &mut impl Write,
    ) -> Result<u64, String> {
        let mut header = [0u8; STREAM_HEADER_BYTES];
        reader.read_exact(&mut header).map_err(|error| error.to_string())?;
        if &header[..STREAM_MAGIC.len()] != STREAM_MAGIC {
            return Err("invalid streaming backup header".into());
        }
        let version = u32::from_le_bytes(header[7..11].try_into().map_err(|_| "invalid key version")?);
        let key = self.keys.get(&version).ok_or("backup key version is missing")?;
        let nonce = chacha20poly1305::aead::generic_array::GenericArray::clone_from_slice(&header[11..18]);
        let plaintext_len = u64::from_le_bytes(header[18..26].try_into().map_err(|_| "invalid backup length")?);
        let chunk_count = if plaintext_len == 0 {
            1
        } else {
            (plaintext_len - 1) / STREAM_CHUNK_BYTES as u64 + 1
        };
        if chunk_count > u32::MAX as u64 {
            return Err("backup exceeds the streaming decryption limit".into());
        }
        let tag_bytes = chunk_count.checked_mul(16).ok_or("backup size overflow")?;
        let expected_len = (STREAM_HEADER_BYTES as u64)
            .checked_add(plaintext_len).and_then(|len| len.checked_add(tag_bytes))
            .ok_or("backup size overflow")?;
        if sealed_len != expected_len {
            return Err("streaming backup length does not match its header".into());
        }
        let mut decryptor = DecryptorBE32::from_aead(Self::cipher(key), &nonce);
        let mut remaining = plaintext_len;
        while remaining > STREAM_CHUNK_BYTES as u64 {
            let length = STREAM_CHUNK_BYTES;
            let encrypted_len = length.checked_add(16).ok_or("backup size overflow")?;
            let mut encrypted = vec![0u8; encrypted_len];
            reader.read_exact(&mut encrypted).map_err(|error| error.to_string())?;
            let chunk = decryptor.decrypt_next(Payload { msg: &encrypted, aad: &header })
                .map_err(|_| "backup authentication failed".to_string())?;
            if chunk.len() != length {
                return Err("invalid decrypted backup chunk length".into());
            }
            writer.write_all(&chunk).map_err(|error| error.to_string())?;
            remaining -= length as u64;
        }
        let length = remaining as usize;
        let encrypted_len = length.checked_add(16).ok_or("backup size overflow")?;
        let mut encrypted = vec![0u8; encrypted_len];
        reader.read_exact(&mut encrypted).map_err(|error| error.to_string())?;
        let chunk = decryptor.decrypt_last(Payload { msg: &encrypted, aad: &header })
            .map_err(|_| "backup authentication failed".to_string())?;
        if chunk.len() != length {
            return Err("invalid decrypted backup chunk length".into());
        }
        writer.write_all(&chunk).map_err(|error| error.to_string())?;
        let mut extra = [0u8; 1];
        if reader.read(&mut extra).map_err(|error| error.to_string())? != 0 {
            return Err("streaming backup has trailing data".into());
        }
        writer.flush().map_err(|error| error.to_string())?;
        Ok(plaintext_len)
    }
}

pub struct RotationReport {
    pub active_version: u32,
    pub previous_version: u32,
    pub reencrypted: usize,
    pub retained_versions: Vec<u32>,
}

fn backup_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("enc"))
        .collect()
}

/// Re-encrypts every backup under a newly generated key version.
pub fn rotate(state: &AppState, keep_old: bool) -> Result<RotationReport, String> {
    let mut ring = KeyRing::load_or_create(&state.config.data_dir)?;
    let previous_version = ring.active_version();
    let directory = state.config.data_dir.join("backups");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let next = ring.versions().into_iter().max().unwrap_or(0) + 1;
    let mut new_key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut new_key);
    ring.keys.insert(next, new_key);
    ring.active = next;

    // Stage each backup before changing the active key. Version 3 backups are
    // decrypted and re-encrypted in bounded chunks; older single-message
    // envelopes still use their historical in-memory decoder one file at a time.
    let mut staged = Vec::new();
    let mut staged_cleanup = RemoveFilesOnDrop(Vec::new());
    for path in backup_files(&directory) {
        let temp = directory.join(format!(".rotate-{}.enc.tmp", uuid::Uuid::new_v4()));
        staged_cleanup.0.push(temp.clone());
        let mut output = std::fs::File::create(&temp).map_err(|error| error.to_string())?;
        if KeyRing::is_stream_file(&path).map_err(|error| error.to_string())? {
            let plain_path = directory.join(format!(".rotate-{}.plain.tmp", uuid::Uuid::new_v4()));
            let _plain_cleanup = RemoveFileOnDrop(plain_path.clone());
            let plain_len = ring.decrypt_stream_file_to(&path, &plain_path)?;
            let mut plaintext = std::fs::File::open(&plain_path).map_err(|error| error.to_string())?;
            ring.encrypt_reader(&mut plaintext, plain_len, &mut output)?;
        } else {
            let sealed = std::fs::read(&path).map_err(|error| error.to_string())?;
            let plain = if KeyRing::is_versioned(&sealed) {
                ring.decrypt(&sealed)
            } else {
                // Legacy snapshots were sealed with the vault key.
                state.vault.open_bytes(&sealed)
            }.ok_or_else(|| format!("cannot decrypt {}", path.display()))?;
            ring.encrypt_reader(&mut std::io::Cursor::new(&plain), plain.len() as u64, &mut output)?;
        }
        output.sync_all().map_err(|error| error.to_string())?;
        staged.push((temp, path));
    }
    write_private(&key_file(&ring.directory, next), &new_key)?;
    std::fs::write(ring.directory.join("active"), next.to_string())
        .map_err(|error| error.to_string())?;
    for (temp, path) in &staged {
        std::fs::rename(temp, path).map_err(|error| error.to_string())?;
    }

    if !keep_old {
        for version in ring.versions() {
            if version != next {
                let _ = std::fs::remove_file(key_file(&ring.directory, version));
            }
        }
    }
    let report = RotationReport {
        active_version: next,
        previous_version,
        reencrypted: staged.len(),
        retained_versions: KeyRing::load_or_create(&state.config.data_dir)?.versions(),
    };
    state.store.audit(
        "backup.keys_rotated",
        json!({
            "activeVersion": report.active_version,
            "previousVersion": report.previous_version,
            "reencrypted": report.reencrypted,
            "retained": report.retained_versions,
            "keepOld": keep_old,
        }),
    );
    Ok(report)
}

pub fn decrypt_backup(state: &AppState, sealed: &[u8]) -> Option<Vec<u8>> {
    let ring = KeyRing::load_or_create(&state.config.data_dir).ok()?;
    if KeyRing::is_versioned(sealed) {
        return ring.decrypt(sealed);
    }
    state.vault.open_bytes(sealed)
}

pub fn decrypt_backup_file_to(
    state: &AppState,
    source: &Path,
    target: &Path,
) -> Result<u64, String> {
    let ring = KeyRing::load_or_create(&state.config.data_dir)?;
    if KeyRing::is_stream_file(source).map_err(|error| error.to_string())? {
        return ring.decrypt_stream_file_to(source, target);
    }
    let sealed = std::fs::read(source).map_err(|error| error.to_string())?;
    let plaintext = if KeyRing::is_versioned(&sealed) {
        ring.decrypt(&sealed)
    } else {
        state.vault.open_bytes(&sealed)
    }.ok_or("backup authentication failed")?;
    std::fs::write(target, &plaintext).map_err(|error| error.to_string())?;
    Ok(plaintext.len() as u64)
}

pub fn status(state: &AppState) -> Value {
    match KeyRing::load_or_create(&state.config.data_dir) {
        Ok(ring) => json!({
            "activeVersion": ring.active_version(),
            "versions": ring.versions(),
        }),
        Err(error) => json!({"error": error}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-keyring-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn rotation_reencrypts_backups_and_drops_old_keys() {
        let (state, dir) = state();
        state.store.put(
            "provider",
            &json!({"id":"p1","name":"rotation","baseUrl":"http://x/v1"}),
        );
        let backup = crate::engine::maintenance::create_snapshot(&state).unwrap();
        let sealed = std::fs::read(&backup).unwrap();
        assert!(KeyRing::is_versioned(&sealed));
        assert_eq!(KeyRing::load_or_create(&dir).unwrap().active_version(), 1);

        let report = rotate(&state, false).unwrap();
        assert_eq!(report.active_version, 2);
        assert_eq!(report.previous_version, 1);
        assert_eq!(report.retained_versions, vec![2]);
        let ring = KeyRing::load_or_create(&dir).unwrap();
        let resealed = std::fs::read(&backup).unwrap();
        let plain = ring.decrypt(&resealed).expect("new key decrypts");
        assert!(plain.starts_with(b"DOMVAULT1"));
        let database = &plain[41..];
        assert!(database.starts_with(b"SQLite format 3"));
        let restored_path = dir.join("rotated.sqlite");
        std::fs::write(&restored_path, database).unwrap();
        assert_eq!(
            Store::open(&restored_path).unwrap().get("provider", "p1").unwrap()["name"],
            "rotation"
        );
        assert!(!key_file(&dir.join("backup-keys"), 1).exists());
    }

    #[test]
    fn keep_old_retains_the_previous_key_version() {
        let (state, dir) = state();
        let _ = crate::engine::maintenance::create_snapshot(&state).unwrap();
        let report = rotate(&state, true).unwrap();
        assert_eq!(report.retained_versions, vec![1, 2]);
        assert!(key_file(&dir.join("backup-keys"), 1).exists());
    }

    #[test]
    fn a_missing_key_version_cannot_decrypt() {
        let (state, dir) = state();
        let _ = crate::engine::maintenance::create_snapshot(&state).unwrap();
        let report = rotate(&state, false).unwrap();
        // Forge an envelope claiming the removed version.
        let mut forged = MAGIC.to_vec();
        forged.extend_from_slice(&report.previous_version.to_le_bytes());
        forged.extend_from_slice(&[0u8; 12]);
        forged.extend_from_slice(&[0u8; 16]);
        let ring = KeyRing::load_or_create(&dir).unwrap();
        assert!(ring.decrypt(&forged).is_none());
    }
}
