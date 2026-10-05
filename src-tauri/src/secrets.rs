//! API keys. The rules (known provider, trimming, an empty key deletes) live in `get_key`/`set_key`;
//! the storage itself is behind `SecretStore` (OS keyring in the app, in-memory in tests).
const SERVICE: &str = "scribe";

/// Raw credential storage, one entry per catalog provider id.
pub trait SecretStore: Send + Sync {
    /// The stored value, or None when there is none or it cannot be read.
    fn get(&self, provider: &str) -> Option<String>;
    fn set(&self, provider: &str, key: &str) -> Result<(), String>;
    /// Deleting a missing entry succeeds.
    fn delete(&self, provider: &str) -> Result<(), String>;
}

/// A usable key: blank stored values count as missing.
pub fn get_key(store: &dyn SecretStore, provider: &str) -> Option<String> {
    store.get(provider).filter(|k| !k.trim().is_empty())
}

/// An empty key deletes the stored credential.
pub fn set_key(store: &dyn SecretStore, provider: &str, key: &str) -> Result<(), String> {
    if scribe_providers::catalog::provider(provider).is_none() {
        return Err(format!("fournisseur inconnu : {provider}"));
    }
    if key.trim().is_empty() {
        store.delete(provider)
    } else {
        store.set(provider, key.trim())
    }
}

/// The OS credential store (Windows Credential Manager, macOS Keychain). `entry` is injectable so the
/// adapter is tested against keyring's mock credentials.
pub struct KeyringStore {
    entry: fn(&str) -> keyring::Result<keyring::Entry>,
}

fn system_entry(provider: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, provider)
}

impl KeyringStore {
    pub fn system() -> Self {
        Self { entry: system_entry }
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, provider: &str) -> Option<String> {
        (self.entry)(provider).ok()?.get_password().ok()
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), String> {
        (self.entry)(provider).and_then(|e| e.set_password(key)).map_err(|e| e.to_string())
    }

    fn delete(&self, provider: &str) -> Result<(), String> {
        match (self.entry)(provider).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
pub mod memory {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::SecretStore;

    /// In-memory `SecretStore`; `fail` makes every write fail with that message, `reads` logs every
    /// `get` (provider id) so tests can check which secrets were looked up.
    #[derive(Default)]
    pub struct MemorySecretStore {
        pub keys: Mutex<HashMap<String, String>>,
        pub fail: Mutex<Option<String>>,
        pub reads: Mutex<Vec<String>>,
    }

    impl MemorySecretStore {
        pub fn with(keys: &[(&str, &str)]) -> Self {
            let store = Self::default();
            store.keys.lock().unwrap().extend(keys.iter().map(|(p, k)| (p.to_string(), k.to_string())));
            store
        }

        fn check(&self) -> Result<(), String> {
            self.fail.lock().unwrap().clone().map_or(Ok(()), Err)
        }
    }

    impl SecretStore for MemorySecretStore {
        fn get(&self, provider: &str) -> Option<String> {
            self.reads.lock().unwrap().push(provider.to_string());
            self.keys.lock().unwrap().get(provider).cloned()
        }
        fn set(&self, provider: &str, key: &str) -> Result<(), String> {
            self.check()?;
            self.keys.lock().unwrap().insert(provider.into(), key.into());
            Ok(())
        }
        fn delete(&self, provider: &str) -> Result<(), String> {
            self.check()?;
            self.keys.lock().unwrap().remove(provider);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::memory::MemorySecretStore;
    use super::*;
    use keyring::mock::MockCredential;

    #[test]
    fn blank_stored_keys_count_as_missing() {
        let store = MemorySecretStore::with(&[("openai", "sk-1"), ("groq", "   ")]);
        assert_eq!(get_key(&store, "openai").as_deref(), Some("sk-1"));
        assert_eq!(get_key(&store, "groq"), None);
        assert_eq!(get_key(&store, "mistral"), None);
    }

    #[test]
    fn set_key_trims_and_rejects_unknown_providers() {
        let store = MemorySecretStore::default();
        set_key(&store, "openai", "  sk-abc \n").unwrap();
        assert_eq!(store.keys.lock().unwrap().get("openai").map(String::as_str), Some("sk-abc"));
        let err = set_key(&store, "nope", "sk").unwrap_err();
        assert_eq!(err, "fournisseur inconnu : nope");
        assert!(!store.keys.lock().unwrap().contains_key("nope"));
    }

    #[test]
    fn empty_key_deletes_the_credential() {
        let store = MemorySecretStore::with(&[("openai", "sk-1")]);
        set_key(&store, "openai", "  ").unwrap();
        assert!(store.keys.lock().unwrap().is_empty());
        // Deleting a key that is not there is not an error.
        set_key(&store, "openai", "").unwrap();
    }

    #[test]
    fn store_errors_are_reported() {
        let store = MemorySecretStore::default();
        *store.fail.lock().unwrap() = Some("verrouillé".into());
        assert_eq!(set_key(&store, "openai", "sk").unwrap_err(), "verrouillé");
        assert_eq!(set_key(&store, "openai", "").unwrap_err(), "verrouillé");
    }

    fn mock_entry(provider: &str) -> keyring::Result<keyring::Entry> {
        if provider == "bad" {
            return Err(keyring::Error::Invalid("user".into(), "vide".into()));
        }
        let entry = keyring::Entry::new_with_credential(Box::new(MockCredential::default()));
        match provider {
            "stored" => entry.set_password("sk-stored")?,
            "locked" => {
                let mock: &MockCredential = entry.get_credential().downcast_ref().unwrap();
                mock.set_error(keyring::Error::NoStorageAccess("verrouillé".to_string().into()));
            }
            _ => {}
        }
        Ok(entry)
    }

    #[test]
    fn keyring_store_maps_keyring_results() {
        let store = KeyringStore { entry: mock_entry };
        assert_eq!(store.get("stored").as_deref(), Some("sk-stored"));
        assert_eq!(store.get("missing"), None, "NoEntry");
        assert_eq!(store.get("locked"), None, "unreadable store");
        assert_eq!(store.get("bad"), None, "invalid entry");

        store.set("missing", "sk").unwrap();
        assert!(store.set("locked", "sk").is_err());
        assert!(store.set("bad", "sk").unwrap_err().contains("vide"));

        store.delete("stored").unwrap();
        store.delete("missing").expect("a missing entry is already deleted");
        assert!(store.delete("locked").is_err());
    }

    #[test]
    fn system_store_targets_the_scribe_service() {
        // Building an entry does not touch the OS store; only get/set/delete do.
        let store = KeyringStore::system();
        assert!((store.entry)("openai").is_ok());
    }
}
