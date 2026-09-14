//! Encrypted store of paired devices and the local identity.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::SecurityError;
use crate::identity::{DeviceId, DeviceIdentity, Fingerprint};
use crate::permissions::Permissions;
use crate::secretbox::{open, seal, write_atomic};

const TRUST_AD: &[u8] = b"soundpush-trust-v1";
const IDENTITY_AD: &[u8] = b"soundpush-identity-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub device_id: DeviceId,
    pub public_key: [u8; 32],
    /// Name reported by the device (untrusted label).
    pub name: String,
    /// Local rename chosen by the user.
    pub alias: Option<String>,
    pub platform: String,
    pub permissions: Permissions,
    pub auto_connect: bool,
    pub blocked: bool,
    pub paired_at_unix: u64,
    pub last_seen_unix: u64,
    /// Last addresses where the device was reachable (hints only).
    pub last_addresses: Vec<String>,
}

impl TrustedDevice {
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of_public_key(&self.public_key)
    }

    pub fn display_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustFile {
    version: u32,
    devices: BTreeMap<String, TrustedDevice>,
}

/// Paired devices, persisted encrypted at rest.
pub struct TrustStore {
    path: PathBuf,
    key: [u8; 32],
    devices: BTreeMap<String, TrustedDevice>,
    /// Hint fields changed in memory and not written yet (see [`TrustStore::update_hints`]).
    dirty: bool,
}

impl TrustStore {
    /// Load the store. A missing file yields an empty store; a corrupted file is
    /// moved aside to `*.corrupt` and an empty store is returned with `recovered = true`.
    pub fn load(path: impl Into<PathBuf>, key: [u8; 32]) -> Result<(Self, bool), SecurityError> {
        let path = path.into();
        let mut recovered = false;
        let devices = match fs::read(&path) {
            Ok(bytes) => match open(&key, TRUST_AD, &bytes)
                .and_then(|plain| Ok(serde_json::from_slice::<TrustFile>(&plain)?))
            {
                Ok(file) => file.devices,
                Err(_) => {
                    let _ = fs::rename(&path, path.with_extension("corrupt"));
                    recovered = true;
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok((
            Self {
                path,
                key,
                devices,
                dirty: false,
            },
            recovered,
        ))
    }

    pub fn get(&self, id: &DeviceId) -> Option<&TrustedDevice> {
        self.devices.get(&id.to_hex())
    }

    /// Returns the trusted, non-blocked device whose key matches `public_key`.
    pub fn authorize(&self, public_key: &[u8; 32]) -> Option<&TrustedDevice> {
        let id = Fingerprint::of_public_key(public_key).device_id();
        self.get(&id)
            .filter(|d| !d.blocked && &d.public_key == public_key)
    }

    pub fn list(&self) -> impl Iterator<Item = &TrustedDevice> {
        self.devices.values()
    }

    pub fn upsert(&mut self, device: TrustedDevice) -> Result<(), SecurityError> {
        self.devices.insert(device.device_id.to_hex(), device);
        self.save()
    }

    pub fn update<F: FnOnce(&mut TrustedDevice)>(
        &mut self,
        id: &DeviceId,
        f: F,
    ) -> Result<bool, SecurityError> {
        let Some(device) = self.devices.get_mut(&id.to_hex()) else {
            return Ok(false);
        };
        f(device);
        self.save()?;
        Ok(true)
    }

    pub fn remove(&mut self, id: &DeviceId) -> Result<bool, SecurityError> {
        let existed = self.devices.remove(&id.to_hex()).is_some();
        if existed {
            self.save()?;
        }
        Ok(existed)
    }

    /// Change fields that are only hints (last seen time, last addresses, reported name) without
    /// writing to disk; [`TrustStore::flush`] persists them later. Never use this for keys,
    /// permissions or block state: those must be durable immediately.
    pub fn update_hints<F: FnOnce(&mut TrustedDevice)>(&mut self, id: &DeviceId, f: F) -> bool {
        let Some(device) = self.devices.get_mut(&id.to_hex()) else {
            return false;
        };
        let key = device.public_key;
        let blocked = device.blocked;
        let permissions = device.permissions;
        f(device);
        // Guard the contract: security fields are restored if a caller touched them.
        device.public_key = key;
        device.blocked = blocked;
        device.permissions = permissions;
        self.dirty = true;
        true
    }

    /// Write pending hint changes, if any.
    pub fn flush(&mut self) -> Result<(), SecurityError> {
        if self.dirty { self.save() } else { Ok(()) }
    }

    fn save(&mut self) -> Result<(), SecurityError> {
        let file = TrustFile {
            version: 1,
            devices: self.devices.clone(),
        };
        let plain = serde_json::to_vec(&file)?;
        write_atomic(&self.path, &seal(&self.key, TRUST_AD, &plain))?;
        self.dirty = false;
        Ok(())
    }
}

/// Load the local identity, creating and persisting a new one on first run.
///
/// A file that cannot be decrypted (the storage key changed, the file was damaged) is moved
/// aside and a new identity is made, so the app still starts; the second value is then
/// `true`, and the user must pair their devices again.
pub fn load_or_create_identity(
    path: &Path,
    key: &[u8; 32],
) -> Result<(DeviceIdentity, bool), SecurityError> {
    match fs::read(path) {
        Ok(bytes) => {
            let secret = open(key, IDENTITY_AD, &bytes).and_then(|plain| {
                plain
                    .as_slice()
                    .try_into()
                    .map_err(|_| SecurityError::Corrupted)
            });
            match secret {
                Ok(secret) => Ok((DeviceIdentity::from_secret_bytes(&secret), false)),
                Err(_) => {
                    let _ = fs::rename(path, path.with_extension("bin.bad"));
                    Ok((create_identity(path, key)?, true))
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok((create_identity(path, key)?, false))
        }
        Err(e) => Err(e.into()),
    }
}

fn create_identity(path: &Path, key: &[u8; 32]) -> Result<DeviceIdentity, SecurityError> {
    let identity = DeviceIdentity::generate();
    write_atomic(
        path,
        &seal(key, IDENTITY_AD, identity.secret_bytes().as_ref()),
    )?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(identity: &DeviceIdentity) -> TrustedDevice {
        TrustedDevice {
            device_id: identity.device_id(),
            public_key: identity.public_key(),
            name: "Phone".into(),
            alias: None,
            platform: "android".into(),
            permissions: Permissions::default(),
            auto_connect: true,
            blocked: false,
            paired_at_unix: 1,
            last_seen_unix: 1,
            last_addresses: vec!["192.168.1.5:47650".into()],
        }
    }

    #[test]
    fn persist_reload_authorize_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust.bin");
        let key = [7u8; 32];
        let peer = DeviceIdentity::generate();

        let (mut store, recovered) = TrustStore::load(&path, key).unwrap();
        assert!(!recovered);
        store.upsert(device(&peer)).unwrap();

        let (mut store, _) = TrustStore::load(&path, key).unwrap();
        assert!(store.authorize(&peer.public_key()).is_some());
        assert!(
            store
                .authorize(&DeviceIdentity::generate().public_key())
                .is_none()
        );

        store
            .update(&peer.device_id(), |d| d.blocked = true)
            .unwrap();
        assert!(store.authorize(&peer.public_key()).is_none());

        assert!(store.remove(&peer.device_id()).unwrap());
        let (store, _) = TrustStore::load(&path, key).unwrap();
        assert_eq!(store.list().count(), 0);
    }

    #[test]
    fn hint_updates_are_deferred_until_flush_and_cannot_touch_security_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust.bin");
        let key = [9u8; 32];
        let peer = DeviceIdentity::generate();
        let (mut store, _) = TrustStore::load(&path, key).unwrap();
        store.upsert(device(&peer)).unwrap();

        assert!(store.update_hints(&peer.device_id(), |d| {
            d.last_seen_unix = 99;
            d.blocked = true;
            d.public_key = [0; 32];
        }));
        assert_eq!(
            store.get(&peer.device_id()).unwrap().public_key,
            peer.public_key()
        );
        assert!(!store.get(&peer.device_id()).unwrap().blocked);
        let (reloaded, _) = TrustStore::load(&path, key).unwrap();
        assert_eq!(
            reloaded.get(&peer.device_id()).unwrap().last_seen_unix,
            1,
            "not written yet"
        );

        store.flush().unwrap();
        let (reloaded, _) = TrustStore::load(&path, key).unwrap();
        assert_eq!(reloaded.get(&peer.device_id()).unwrap().last_seen_unix, 99);
        assert!(!store.update_hints(&DeviceIdentity::generate().device_id(), |_| {}));
    }

    #[test]
    fn corrupted_store_is_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust.bin");
        fs::write(&path, b"garbage").unwrap();
        let (store, recovered) = TrustStore::load(&path, [1u8; 32]).unwrap();
        assert!(recovered);
        assert_eq!(store.list().count(), 0);
        assert!(path.with_extension("corrupt").exists());
    }

    #[test]
    fn identity_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.bin");
        let key = [5u8; 32];
        let (a, reset_a) = load_or_create_identity(&path, &key).unwrap();
        let (b, reset_b) = load_or_create_identity(&path, &key).unwrap();
        assert_eq!(a.public_key(), b.public_key());
        assert!(!reset_a && !reset_b);
        // A file the key can't open is moved aside and a fresh identity takes its place.
        let (c, reset_c) = load_or_create_identity(&path, &[6u8; 32]).unwrap();
        assert!(reset_c);
        assert_ne!(c.public_key(), a.public_key());
        assert!(dir.path().join("identity.bin.bad").exists());
    }
}
