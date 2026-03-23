use anyhow::{Context, Result};

const SERVICE: &str = "ur";

pub fn set_api_key(provider_id: &str, key: &str) -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE, provider_id).context("failed to create keyring entry")?;
    entry
        .set_password(key)
        .context("failed to store API key in keyring")
}

pub fn get_api_key(provider_id: &str) -> Result<Option<String>> {
    let entry =
        keyring::Entry::new(SERVICE, provider_id).context("failed to create keyring entry")?;
    match entry.get_password() {
        Ok(val) => Ok(Some(val)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!(e).context("failed to read API key from keyring")),
    }
}
