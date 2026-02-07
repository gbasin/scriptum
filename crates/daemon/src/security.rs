use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{rand_core::RngCore, Aead, OsRng},
    KeyInit, XChaCha20Poly1305, XNonce,
};

const KEYRING_SERVICE: &str = "com.scriptum.daemon";
const MASTER_KEY_ACCOUNT: &str = "crdt_master_key_v1";

const MASTER_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
const ENVELOPE_MAGIC: [u8; 4] = *b"SEC1";

static MASTER_KEY_CACHE: OnceLock<std::result::Result<[u8; MASTER_KEY_BYTES], String>> =
    OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSlot {
    ApiKey,
    GitCredentials,
    RelayToken,
}

impl SecretSlot {
    fn account(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::GitCredentials => "git_credentials",
            Self::RelayToken => "relay_token",
        }
    }
}

pub fn set_secret(slot: SecretSlot, value: &str) -> Result<()> {
    set_secret_with_store(&KeyringSecretStore, slot, value)
}

pub fn get_secret(slot: SecretSlot) -> Result<Option<String>> {
    get_secret_with_store(&KeyringSecretStore, slot)
}

pub fn delete_secret(slot: SecretSlot) -> Result<()> {
    delete_secret_with_store(&KeyringSecretStore, slot)
}

pub fn encrypt_at_rest(plaintext: &[u8]) -> Result<Vec<u8>> {
    let key = master_key()?;
    encrypt_with_key(plaintext, &key)
}

pub fn decrypt_at_rest(payload: &[u8]) -> Result<Vec<u8>> {
    if !payload.starts_with(&ENVELOPE_MAGIC) {
        // Backward compatibility for plaintext records written before
        // transport/storage hardening landed.
        return Ok(payload.to_vec());
    }

    let key = master_key()?;
    decrypt_with_key(payload, &key)
}

pub fn ensure_owner_only_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if !path.exists() {
            return Ok(());
        }

        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to read metadata for `{}`", path.display()))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .with_context(|| format!("failed to set owner-only mode on `{}`", path.display()))?;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

pub fn ensure_owner_only_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if !path.exists() {
            return Ok(());
        }

        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to read metadata for `{}`", path.display()))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("failed to set owner-only mode on `{}`", path.display()))?;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

pub fn open_private_append(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        OpenOptions::new().create(true).append(true).mode(0o600).open(path)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().create(true).append(true).open(path)
    }
}

pub fn open_private_truncate(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(path)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().create(true).write(true).truncate(true).open(path)
    }
}

fn master_key() -> Result<[u8; MASTER_KEY_BYTES]> {
    MASTER_KEY_CACHE
        .get_or_init(|| load_or_create_master_key().map_err(|error| error.to_string()))
        .clone()
        .map_err(|error| anyhow!(error))
}

fn load_or_create_master_key() -> Result<[u8; MASTER_KEY_BYTES]> {
    if let Ok(value) = std::env::var("SCRIPTUM_DAEMON_MASTER_KEY_BASE64") {
        return decode_key(&value).context(
            "SCRIPTUM_DAEMON_MASTER_KEY_BASE64 must be a base64url-no-pad 32-byte key",
        );
    }

    let store = KeyringSecretStore;
    if let Some(stored) = store
        .get_secret(KEYRING_SERVICE, MASTER_KEY_ACCOUNT)
        .context("failed to read daemon master key from keychain")?
    {
        return decode_key(&stored).context("stored daemon master key is invalid");
    }

    let mut key = [0u8; MASTER_KEY_BYTES];
    OsRng.fill_bytes(&mut key);
    let encoded = STANDARD_NO_PAD.encode(key);

    store
        .set_secret(KEYRING_SERVICE, MASTER_KEY_ACCOUNT, &encoded)
        .context("failed to persist daemon master key to keychain")?;

    Ok(key)
}

fn encode_key(key: &[u8; MASTER_KEY_BYTES]) -> String {
    STANDARD_NO_PAD.encode(key)
}

fn decode_key(encoded: &str) -> Result<[u8; MASTER_KEY_BYTES]> {
    let bytes = STANDARD_NO_PAD
        .decode(encoded.trim())
        .context("master key is not valid base64url-no-pad")?;
    if bytes.len() != MASTER_KEY_BYTES {
        bail!(
            "master key must be {} bytes, got {} bytes",
            MASTER_KEY_BYTES,
            bytes.len()
        );
    }

    let mut key = [0u8; MASTER_KEY_BYTES];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn encrypt_with_key(plaintext: &[u8], key: &[u8; MASTER_KEY_BYTES]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).context("invalid master key length")?;
    let mut nonce = [0u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);

    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .context("failed to encrypt at-rest payload")?;

    let mut envelope = Vec::with_capacity(ENVELOPE_MAGIC.len() + NONCE_BYTES + ciphertext.len());
    envelope.extend_from_slice(&ENVELOPE_MAGIC);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

fn decrypt_with_key(payload: &[u8], key: &[u8; MASTER_KEY_BYTES]) -> Result<Vec<u8>> {
    if payload.len() < ENVELOPE_MAGIC.len() + NONCE_BYTES {
        bail!("encrypted payload is truncated");
    }
    if !payload.starts_with(&ENVELOPE_MAGIC) {
        bail!("encrypted payload is missing envelope magic");
    }

    let cipher = XChaCha20Poly1305::new_from_slice(key).context("invalid master key length")?;
    let nonce_start = ENVELOPE_MAGIC.len();
    let nonce_end = nonce_start + NONCE_BYTES;
    let nonce = XNonce::from_slice(&payload[nonce_start..nonce_end]);
    let ciphertext = &payload[nonce_end..];
    cipher.decrypt(nonce, ciphertext).context("failed to decrypt at-rest payload")
}

trait SecretStore: Send + Sync {
    fn set_secret(&self, service: &str, account: &str, value: &str) -> Result<()>;
    fn get_secret(&self, service: &str, account: &str) -> Result<Option<String>>;
    fn delete_secret(&self, service: &str, account: &str) -> Result<()>;
}

struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn set_secret(&self, service: &str, account: &str, value: &str) -> Result<()> {
        let entry = keyring::Entry::new(service, account)
            .context("failed to initialize keychain entry")?;
        entry
            .set_password(value)
            .context("failed to write keychain entry")?;
        Ok(())
    }

    fn get_secret(&self, service: &str, account: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(service, account)
            .context("failed to initialize keychain entry")?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error).context("failed to read keychain entry"),
        }
    }

    fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
        let entry = keyring::Entry::new(service, account)
            .context("failed to initialize keychain entry")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error).context("failed to delete keychain entry"),
        }
    }
}

fn set_secret_with_store(store: &dyn SecretStore, slot: SecretSlot, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("secret value must not be empty");
    }
    store
        .set_secret(KEYRING_SERVICE, slot.account(), value)
        .with_context(|| format!("failed to persist `{}` in keychain", slot.account()))
}

fn get_secret_with_store(store: &dyn SecretStore, slot: SecretSlot) -> Result<Option<String>> {
    store
        .get_secret(KEYRING_SERVICE, slot.account())
        .with_context(|| format!("failed to read `{}` from keychain", slot.account()))
}

fn delete_secret_with_store(store: &dyn SecretStore, slot: SecretSlot) -> Result<()> {
    store
        .delete_secret(KEYRING_SERVICE, slot.account())
        .with_context(|| format!("failed to clear `{}` from keychain", slot.account()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MemorySecretStore {
        values: Mutex<HashMap<(String, String), String>>,
    }

    impl SecretStore for MemorySecretStore {
        fn set_secret(&self, service: &str, account: &str, value: &str) -> Result<()> {
            self.values
                .lock()
                .expect("memory secret store lock should not be poisoned")
                .insert((service.to_string(), account.to_string()), value.to_string());
            Ok(())
        }

        fn get_secret(&self, service: &str, account: &str) -> Result<Option<String>> {
            Ok(self
                .values
                .lock()
                .expect("memory secret store lock should not be poisoned")
                .get(&(service.to_string(), account.to_string()))
                .cloned())
        }

        fn delete_secret(&self, service: &str, account: &str) -> Result<()> {
            self.values
                .lock()
                .expect("memory secret store lock should not be poisoned")
                .remove(&(service.to_string(), account.to_string()));
            Ok(())
        }
    }

    #[test]
    fn secret_store_round_trip_by_slot() {
        let store = MemorySecretStore::default();
        set_secret_with_store(&store, SecretSlot::ApiKey, "sk-test").expect("write should succeed");
        assert_eq!(
            get_secret_with_store(&store, SecretSlot::ApiKey).expect("read should succeed"),
            Some("sk-test".to_string())
        );
        delete_secret_with_store(&store, SecretSlot::ApiKey).expect("delete should succeed");
        assert_eq!(
            get_secret_with_store(&store, SecretSlot::ApiKey).expect("read should succeed"),
            None
        );
    }

    #[test]
    fn at_rest_encryption_round_trip() {
        let key = decode_key("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY")
            .expect("fixed key should decode");
        let plaintext = b"hello scriptum";
        let encrypted = encrypt_with_key(plaintext, &key).expect("encrypt should succeed");

        assert!(encrypted.starts_with(&ENVELOPE_MAGIC));
        assert_ne!(encrypted, plaintext);

        let decrypted = decrypt_with_key(&encrypted, &key).expect("decrypt should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_at_rest_passes_through_legacy_plaintext() {
        let plaintext = b"legacy";
        let decrypted = decrypt_at_rest(plaintext).expect("plaintext passthrough should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn key_encode_decode_round_trip() {
        let mut key = [0u8; MASTER_KEY_BYTES];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::try_from(index).expect("index should fit u8");
        }
        let encoded = encode_key(&key);
        let decoded = decode_key(&encoded).expect("decode should succeed");
        assert_eq!(decoded, key);
    }

    #[cfg(unix)]
    #[test]
    fn owner_only_helpers_apply_expected_modes() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempdir().expect("tempdir should be created");
        let dir_path = tmp.path().join("private-dir");
        let file_path = dir_path.join("private.bin");

        fs::create_dir_all(&dir_path).expect("directory should be created");
        fs::write(&file_path, b"secret").expect("file should be created");

        fs::set_permissions(&dir_path, fs::Permissions::from_mode(0o755))
            .expect("directory permissions should be set");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o644))
            .expect("file permissions should be set");

        ensure_owner_only_dir(&dir_path).expect("directory mode should be tightened");
        ensure_owner_only_file(&file_path).expect("file mode should be tightened");

        let dir_mode =
            fs::metadata(&dir_path).expect("directory metadata should load").permissions().mode()
                & 0o777;
        let file_mode =
            fs::metadata(&file_path).expect("file metadata should load").permissions().mode()
                & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
