use crate::core::settings::Settings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const KEYCHAIN_SERVICE: &str = "okf-llm-wiki";
const KEYCHAIN_ACCOUNT: &str = "api_key";
const SETTINGS_FILE: &str = "settings.json";

/// Abstraction over secret storage so persistence is testable without a real keychain.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Option<String>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}

/// Production secret store backed by the OS keychain.
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    pub fn new() -> Self {
        KeyringSecretStore
    }
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, account: &str) -> Option<String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, account)
            .ok()?
            .get_password()
            .ok()
    }
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        keyring::Entry::new(KEYCHAIN_SERVICE, account)?.set_password(secret)?;
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Non-secret settings persisted as JSON. `api_key` is intentionally absent — it lives in the keychain.
#[derive(Serialize, Deserialize)]
struct PersistedSettings {
    provider: String,
    model: String,
    wiki_path: String,
}

/// Persists Settings: non-secret fields to `<dir>/settings.json`, the API key to a `SecretStore`.
pub struct ConfigStore {
    dir: PathBuf,
    secrets: Box<dyn SecretStore>,
}

impl ConfigStore {
    pub fn new(dir: impl Into<PathBuf>, secrets: Box<dyn SecretStore>) -> Self {
        ConfigStore {
            dir: dir.into(),
            secrets,
        }
    }

    /// Write non-secret settings to disk (atomic temp+rename) and the key to the secret store.
    pub fn save(&self, s: &Settings) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating config dir {}", self.dir.display()))?;
        let persisted = PersistedSettings {
            provider: s.provider.clone(),
            model: s.model.clone(),
            wiki_path: s.wiki_path.clone(),
        };
        let json = serde_json::to_string_pretty(&persisted).context("serializing settings")?;
        let dest = self.dir.join(SETTINGS_FILE);
        // Explicit `settings.json.tmp` in the same dir (not `with_extension`, which would
        // mangle a compound filename) so the temp+rename stays atomic on one filesystem.
        let tmp = self.dir.join(format!("{SETTINGS_FILE}.tmp"));
        std::fs::write(&tmp, &json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &dest).with_context(|| format!("renaming to {}", dest.display()))?;

        if s.api_key.is_empty() {
            self.secrets
                .delete(KEYCHAIN_ACCOUNT)
                .context("clearing api key")?;
        } else {
            self.secrets
                .set(KEYCHAIN_ACCOUNT, &s.api_key)
                .context("saving api key to keychain")?;
        }
        Ok(())
    }

    /// Load settings; any missing/corrupt piece degrades to empty rather than failing.
    pub fn load(&self) -> Settings {
        let dest = self.dir.join(SETTINGS_FILE);
        let mut settings = match std::fs::read_to_string(&dest) {
            Ok(raw) => match serde_json::from_str::<PersistedSettings>(&raw) {
                Ok(p) => Settings {
                    provider: p.provider,
                    model: p.model,
                    api_key: String::new(),
                    wiki_path: p.wiki_path,
                },
                // Present but corrupt: surface it so the user can self-diagnose
                // (e.g. delete the file), then degrade to defaults. The file never
                // holds the API key, so logging the parse error leaks no secret.
                Err(e) => {
                    eprintln!("okf: ignoring corrupt {}: {e}", dest.display());
                    Settings::default()
                }
            },
            // Missing file is the normal first-run case — no warning.
            Err(_) => Settings::default(),
        };
        settings.api_key = self.secrets.get(KEYCHAIN_ACCOUNT).unwrap_or_default();
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory secret store for tests — never touches the real keychain.
    #[derive(Default)]
    struct MemSecretStore {
        inner: Mutex<HashMap<String, String>>,
    }
    impl SecretStore for MemSecretStore {
        fn get(&self, account: &str) -> Option<String> {
            self.inner.lock().unwrap().get(account).cloned()
        }
        fn set(&self, account: &str, secret: &str) -> Result<()> {
            self.inner
                .lock()
                .unwrap()
                .insert(account.into(), secret.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.inner.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-cfg-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn sample() -> Settings {
        Settings {
            provider: "claude".into(),
            model: "claude-opus-4-8".into(),
            api_key: "sk-secret-123".into(),
            wiki_path: "/Users/me/wiki".into(),
        }
    }

    #[test]
    fn save_then_load_roundtrips_all_fields() {
        let cfg = ConfigStore::new(tmp(), Box::new(MemSecretStore::default()));
        cfg.save(&sample()).unwrap();
        let loaded = cfg.load();
        assert_eq!(loaded, sample());
    }

    #[test]
    fn settings_file_never_contains_the_api_key() {
        let dir = tmp();
        let cfg = ConfigStore::new(dir.clone(), Box::new(MemSecretStore::default()));
        cfg.save(&sample()).unwrap();
        let on_disk = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(
            !on_disk.contains("sk-secret-123"),
            "api key must not be written to settings.json"
        );
        assert!(
            on_disk.contains("/Users/me/wiki"),
            "non-secret fields should be persisted"
        );
    }

    #[test]
    fn load_missing_config_returns_defaults() {
        let cfg = ConfigStore::new(tmp(), Box::new(MemSecretStore::default()));
        assert_eq!(cfg.load(), Settings::default());
    }

    #[test]
    fn empty_api_key_clears_the_secret() {
        let dir = tmp();
        let cfg = ConfigStore::new(dir, Box::new(MemSecretStore::default()));
        cfg.save(&sample()).unwrap();
        let mut cleared = sample();
        cleared.api_key = String::new();
        cfg.save(&cleared).unwrap();
        assert_eq!(cfg.load().api_key, "");
    }
}
