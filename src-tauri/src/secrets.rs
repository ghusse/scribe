const SERVICE: &str = "scribe";

pub fn get_key(provider: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, provider).ok()?.get_password().ok().filter(|k| !k.trim().is_empty())
}

/// An empty key deletes the stored credential.
pub fn set_key(provider: &str, key: &str) -> Result<(), String> {
    if scribe_providers::catalog::provider(provider).is_none() {
        return Err(format!("fournisseur inconnu : {provider}"));
    }
    let entry = keyring::Entry::new(SERVICE, provider).map_err(|e| e.to_string())?;
    if key.trim().is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        entry.set_password(key.trim()).map_err(|e| e.to_string())
    }
}
