# M1 — Make v1 Trustworthy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist settings (API key in the OS keychain) across restarts, build the retrieval index on startup, and make digest tolerate fenced/prose LLM JSON — so okf-llm-wiki is a reliable daily driver.

**Architecture:** A new framework-agnostic `core/config.rs` owns persistence: non-secret settings as JSON in the OS app-config dir, the API key behind a `SecretStore` trait (real `keyring` impl + in-memory test impl). The Tauri layer (`lib.rs` setup) resolves the config dir, loads settings, and builds the index at startup; `set_settings` persists and rebuilds.

**Tech Stack:** Rust + Tauri 2, `keyring` (OS keychain), `time` (RFC-3339), `serde_json`. Frontend: Svelte 5 + vitest.

Spec: `docs/superpowers/specs/2026-06-16-m1-trustworthy-design.md`. Branch: `feat/m1-trustworthy`.

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src-tauri/Cargo.toml` | Modify | Add `keyring`, `time` deps |
| `src-tauri/src/core/config.rs` | Create | `SecretStore` trait, `KeyringSecretStore`, `ConfigStore` (save/load) |
| `src-tauri/src/core/mod.rs` | Modify | `pub mod config;` |
| `src-tauri/src/state.rs` | Modify | `AppState.config` field; `initial_index()` helper |
| `src-tauri/src/lib.rs` | Modify | Build `AppState` in `setup`: load settings + index at startup |
| `src-tauri/src/commands.rs` | Modify | `set_settings` → persists + rebuilds index, returns `Result` |
| `src-tauri/src/core/digest.rs` | Modify | `extract_json` hardening; real ISO-8601 timestamp |
| `src/lib/components/Settings.svelte` | Modify | Surface save errors |
| `src/lib/api.test.ts` | Modify | Cover `setSettings` success + rejection |

**Note on running cargo:** if `cargo` is not on PATH, use `$HOME/.cargo/bin/cargo` (run `cargo` commands from inside `src-tauri/`).

---

### Task 1: ConfigStore + SecretStore (persistence core)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/core/config.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Add dependencies**

In `src-tauri/Cargo.toml`, under `[dependencies]`, after the `anyhow = "1"` line, add:

```toml
keyring = { version = "3", features = ["apple-native"] }
time = { version = "0.3", features = ["formatting"] }
```

(The app targets macOS; `apple-native` selects the macOS Keychain backend. Add `windows-native` / `sync-secret-service` features later if other platforms are supported.)

- [ ] **Step 2: Register the module**

In `src-tauri/src/core/mod.rs`, add this line at the end:

```rust
pub mod config;
```

- [ ] **Step 3: Write `core/config.rs` with the trait, real impl, and failing tests**

Create `src-tauri/src/core/config.rs`:

```rust
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
        ConfigStore { dir: dir.into(), secrets }
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
        let tmp = dest.with_extension("json.tmp");
        std::fs::write(&tmp, &json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &dest)
            .with_context(|| format!("renaming to {}", dest.display()))?;

        if s.api_key.is_empty() {
            self.secrets.delete(KEYCHAIN_ACCOUNT).context("clearing api key")?;
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
        let mut settings = std::fs::read_to_string(&dest)
            .ok()
            .and_then(|raw| serde_json::from_str::<PersistedSettings>(&raw).ok())
            .map(|p| Settings {
                provider: p.provider,
                model: p.model,
                api_key: String::new(),
                wiki_path: p.wiki_path,
            })
            .unwrap_or_default();
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
            self.inner.lock().unwrap().insert(account.into(), secret.into());
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
        assert!(!on_disk.contains("sk-secret-123"), "api key must not be written to settings.json");
        assert!(on_disk.contains("/Users/me/wiki"), "non-secret fields should be persisted");
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
```

- [ ] **Step 4: Run the tests — expect them to pass**

Run: `cd src-tauri && cargo test config::`
Expected: 4 new tests pass (`save_then_load_roundtrips_all_fields`, `settings_file_never_contains_the_api_key`, `load_missing_config_returns_defaults`, `empty_api_key_clears_the_secret`).

- [ ] **Step 5: Lint & format**

Run: `cd src-tauri && cargo clippy --all-targets && cargo fmt`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/core/config.rs src-tauri/src/core/mod.rs
git commit -m "feat(core): ConfigStore with keychain-backed settings persistence

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Wire persistence into startup (AppState + lib.rs)

**Files:**
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing test for the startup-index helper**

Add this test module to the END of `src-tauri/src/state.rs` (the helper it tests is written in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::page::{Frontmatter, Page};
    use crate::core::store::OkfStore;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-state-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_wiki_path_yields_empty_index() {
        assert!(initial_index("").is_empty());
    }

    #[test]
    fn populated_wiki_path_builds_index() {
        let dir = tmp();
        let store = OkfStore::new(dir.clone());
        store
            .write_page(&Page {
                path: "concepts/x.md".into(),
                frontmatter: Frontmatter {
                    type_: "Concept".into(),
                    title: Some("X".into()),
                    description: None,
                    tags: vec![],
                    resource: None,
                    timestamp: None,
                    note: None,
                    extra: BTreeMap::new(),
                },
                body: "body".into(),
            })
            .unwrap();
        assert_eq!(initial_index(dir.to_str().unwrap()).len(), 1);
    }
}
```

- [ ] **Step 2: Run the test — verify it fails to compile**

Run: `cd src-tauri && cargo test state::tests`
Expected: FAIL — `cannot find function 'initial_index'`.

- [ ] **Step 3: Replace `state.rs` with the new AppState + helper**

Replace the ENTIRE non-test content at the top of `src-tauri/src/state.rs` (lines 1-9) with:

```rust
use crate::core::config::ConfigStore;
use crate::core::retrieval::{build_index, IndexEntry};
use crate::core::settings::Settings;
use crate::core::store::OkfStore;
use std::sync::Mutex;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub index: Mutex<Vec<IndexEntry>>,
    pub config: ConfigStore,
}

/// Build the retrieval index from a wiki path, returning empty for an unset path
/// or any read failure (the app should still launch).
pub fn initial_index(wiki_path: &str) -> Vec<IndexEntry> {
    if wiki_path.is_empty() {
        return Vec::new();
    }
    build_index(&OkfStore::new(wiki_path)).unwrap_or_default()
}
```

(The `#[cfg(test)] mod tests` block from Step 1 stays below this.)

- [ ] **Step 4: Run the helper tests — expect pass**

Run: `cd src-tauri && cargo test state::tests`
Expected: `empty_wiki_path_yields_empty_index` and `populated_wiki_path_builds_index` pass.

- [ ] **Step 5: Build AppState in the Tauri setup hook**

Replace the entire body of `src-tauri/src/lib.rs` with:

```rust
pub mod core;
mod commands;
mod state;

use crate::core::config::{ConfigStore, KeyringSecretStore};
use crate::state::{initial_index, AppState};
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_config_dir()
                .expect("resolving app config dir");
            let config = ConfigStore::new(dir, Box::new(KeyringSecretStore::new()));
            let settings = config.load();
            let index = initial_index(&settings.wiki_path);
            app.manage(AppState {
                settings: Mutex::new(settings),
                index: Mutex::new(index),
                config,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_pages,
            commands::submit_source,
            commands::ask_question
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 6: Build the whole crate**

Run: `cd src-tauri && cargo build`
Expected: compiles. (`set_settings` still has its old signature — it compiles because `AppState.config` is simply unused there until Task 3.)

- [ ] **Step 7: Lint, format, full test run**

Run: `cd src-tauri && cargo clippy --all-targets && cargo fmt && cargo test`
Expected: no warnings; all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/state.rs src-tauri/src/lib.rs
git commit -m "feat: load settings + build index on startup

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Persist + rebuild on `set_settings`

**Files:**
- Modify: `src-tauri/src/commands.rs:16-19`

- [ ] **Step 1: Replace `set_settings`**

In `src-tauri/src/commands.rs`, replace the current `set_settings` (lines 16-19):

```rust
#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: Settings) {
    *state.settings.lock().unwrap() = settings;
}
```

with:

```rust
#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    state.config.save(&settings).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = crate::state::initial_index(&settings.wiki_path);
    *state.settings.lock().unwrap() = settings;
    Ok(())
}
```

(Order: persist first so a keychain failure aborts before we mutate in-memory state. No `MutexGuard` is held across the `config.save` call — the locks are taken only at the assignment statements.)

- [ ] **Step 2: Build**

Run: `cd src-tauri && cargo build`
Expected: compiles.

- [ ] **Step 3: Run the existing integration test**

Run: `cd src-tauri && cargo test`
Expected: all tests pass (the commands integration test still compiles — `set_settings` returning `Result` is fine over IPC).

- [ ] **Step 4: Lint & format**

Run: `cd src-tauri && cargo clippy --all-targets && cargo fmt`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat: persist settings and rebuild index on set_settings

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Harden digest JSON parsing

**Files:**
- Modify: `src-tauri/src/core/digest.rs:24-26` (parse call) and add helpers + tests

- [ ] **Step 1: Add failing tests for fenced & prose-wrapped JSON**

In `src-tauri/src/core/digest.rs`, inside the existing `#[cfg(test)] mod tests` block (after `errors_on_malformed_json`), add:

```rust
    #[tokio::test]
    async fn parses_json_wrapped_in_code_fence() {
        let reply = "Here you go:\n```json\n{\"title\":\"T\",\"description\":\"d\",\"tags\":[],\"body\":\"**TL;DR.** x\"}\n```";
        let p = FakeProvider { reply: reply.into() };
        let r = digest(&p, "src", None, None).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("T".into()));
    }

    #[tokio::test]
    async fn parses_json_with_surrounding_prose() {
        let reply = "Sure! {\"title\":\"P\",\"description\":\"d\",\"tags\":[\"a\"],\"body\":\"b\"} Hope that helps.";
        let p = FakeProvider { reply: reply.into() };
        let r = digest(&p, "src", None, None).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("P".into()));
    }

    #[test]
    fn extract_json_handles_braces_inside_strings() {
        let raw = "noise {\"body\":\"a } b\",\"x\":1} trailing";
        assert_eq!(extract_json(raw), "{\"body\":\"a } b\",\"x\":1}");
    }
```

- [ ] **Step 2: Run — verify failure**

Run: `cd src-tauri && cargo test digest::`
Expected: FAIL — `parses_json_wrapped_in_code_fence` and the others fail (current parser chokes on fences/prose; `extract_json` undefined).

- [ ] **Step 3: Add the `extract_json` + `first_json_object` helpers**

In `src-tauri/src/core/digest.rs`, add these free functions just above the `now_iso` function:

```rust
/// Pull a JSON object out of an LLM reply that may wrap it in ```fences``` or prose.
fn extract_json(raw: &str) -> &str {
    let s = raw.trim();
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        let after = after.trim_start_matches(['\n', '\r', ' ']);
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    first_json_object(s).unwrap_or(s)
}

/// Return the first balanced `{...}` span, ignoring braces inside JSON strings.
fn first_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if in_str {
            match c {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[start..=i]);
                    }
                }
                _ => {}
            }
        }
    }
    None
}
```

- [ ] **Step 4: Use the helper in `digest`**

In `src-tauri/src/core/digest.rs`, replace the parse line (currently line 25):

```rust
    let parsed: DigestJson = serde_json::from_str(raw.trim())
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
```

with:

```rust
    let parsed: DigestJson = serde_json::from_str(extract_json(&raw))
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
```

- [ ] **Step 5: Run — verify pass**

Run: `cd src-tauri && cargo test digest::`
Expected: all digest tests pass, including the 3 new ones and the unchanged `errors_on_malformed_json`.

- [ ] **Step 6: Lint & format**

Run: `cd src-tauri && cargo clippy --all-targets && cargo fmt`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/core/digest.rs
git commit -m "fix(core): tolerate fenced/prose-wrapped JSON in digest

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Real ISO-8601 timestamps

**Files:**
- Modify: `src-tauri/src/core/digest.rs:45-50`

- [ ] **Step 1: Add a failing test for the timestamp format**

In `src-tauri/src/core/digest.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn now_iso_is_rfc3339() {
        let ts = now_iso();
        // RFC-3339 looks like 2026-06-16T12:34:56...Z — starts with a 4-digit year and 'T' at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(ts.as_bytes()[10], b'T', "expected date/time 'T' separator, got {ts}");
        assert!(!ts.starts_with("unixtime"), "should not be the old placeholder, got {ts}");
    }
```

- [ ] **Step 2: Run — verify failure**

Run: `cd src-tauri && cargo test digest::tests::now_iso_is_rfc3339`
Expected: FAIL — current `now_iso` returns `unixtime:NNN`.

- [ ] **Step 3: Replace `now_iso`**

In `src-tauri/src/core/digest.rs`, replace the current `now_iso` (lines 45-50):

```rust
fn now_iso() -> String {
    // Minimal RFC3339-ish stamp without extra deps.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("unixtime:{secs}")
}
```

with:

```rust
fn now_iso() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}
```

- [ ] **Step 4: Run — verify pass**

Run: `cd src-tauri && cargo test digest::`
Expected: `now_iso_is_rfc3339` passes; all other digest tests still pass.

- [ ] **Step 5: Lint & format**

Run: `cd src-tauri && cargo clippy --all-targets && cargo fmt`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/digest.rs
git commit -m "fix(core): emit real RFC-3339 timestamps in digest

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Frontend — surface save errors

**Files:**
- Modify: `src/lib/components/Settings.svelte:5-7,16`
- Modify: `src/lib/api.test.ts`

- [ ] **Step 1: Add a failing test for `setSettings`**

Replace the body of `src/lib/api.test.ts` with:

```ts
import { describe, it, expect, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (_cmd, args) => ({ echoed: args })) }));
import { listPages, submitSource, setSettings } from "./api";
import { invoke } from "@tauri-apps/api/core";

describe("api", () => {
  it("submitSource passes input and note", async () => {
    const r: any = await submitSource("https://x", "my note");
    expect(r.echoed).toEqual({ input: "https://x", note: "my note" });
  });
  it("listPages calls through", async () => {
    await expect(listPages()).resolves.toBeDefined();
  });
  it("setSettings rejects when the backend errors", async () => {
    (invoke as any).mockRejectedValueOnce("keychain failure");
    await expect(
      setSettings({ provider: "claude", model: "m", api_key: "k", wiki_path: "/w" })
    ).rejects.toBe("keychain failure");
  });
});
```

- [ ] **Step 2: Run — verify the new test passes (api.ts already throws via invoke) and nothing else breaks**

Run: `npm run test`
Expected: 3 tests pass. (No `api.ts` change is needed — `invoke` already rejects on a backend `Err`; this test locks in that contract.)

- [ ] **Step 3: Surface the error in `Settings.svelte`**

In `src/lib/components/Settings.svelte`, replace the `<script>` body (lines 4-7):

```ts
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"" };
  let saved = false;
  onMount(async () => { s = await getSettings(); });
  async function save(){ await setSettings(s); saved = true; setTimeout(()=>saved=false, 1500); }
```

with:

```ts
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"" };
  let saved = false;
  let error = "";
  onMount(async () => { s = await getSettings(); });
  async function save(){
    error = "";
    try {
      await setSettings(s);
      saved = true;
      setTimeout(()=>saved=false, 1500);
    } catch (e) {
      error = String(e);
    }
  }
```

Then, in the same file, replace the save button line (currently line 16):

```svelte
    <button class="nb-btn accent" on:click={save}>{saved?"Saved ✓":"Save"}</button>
```

with:

```svelte
    <button class="nb-btn accent" on:click={save}>{saved?"Saved ✓":"Save"}</button>
    {#if error}<p style="color:var(--pink);font-weight:700">⚠ {error}</p>{/if}
```

- [ ] **Step 4: Type-check & test**

Run: `npm run check && npm run test`
Expected: no svelte-check errors; 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Settings.svelte src/lib/api.test.ts
git commit -m "feat(ui): surface settings save errors; cover setSettings

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Security review + final verification

**Files:** none (review + gate)

- [ ] **Step 1: Run the security reviewer**

Dispatch the `security-reviewer` agent over the diff (`git diff main...feat/m1-trustworthy`), focusing on `core/config.rs`, `state.rs`, `commands.rs`. Confirm:
- The API key is never written to `settings.json` (covered by `settings_file_never_contains_the_api_key`).
- The key never appears in logs, error strings (`anyhow` contexts use "saving api key to keychain" — no value), or panic messages.
- Keychain errors propagate as messages without echoing the secret.
- New deps (`keyring`, `time`) are reputable and pinned.

Resolve any Critical/High finding before proceeding.

- [ ] **Step 2: Full green across both stacks**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets && cargo fmt --check`
Run: `npm run check && npm run test && npm run build`
Expected: all tests pass; clippy clean; fmt clean; frontend builds.

- [ ] **Step 3: Manual smoke note (user-run, not automated)**

Record in the PR description that the reviewer must manually verify with a real key: launch `npm run tauri dev`, set Settings (wiki folder + key), quit, relaunch → settings persist and Ask works immediately without re-adding a page.

- [ ] **Step 4: Commit any review fixes**

```bash
git add -A
git commit -m "chore: address M1 security review

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

(Skip if the reviewer found nothing to fix.)

---

## Self-Review

**Spec coverage:**
- Settings persistence (keychain + JSON) → Task 1 ✓
- Startup load + index build → Task 2 ✓
- `set_settings` persists + rebuilds, `Result` signature → Task 3 ✓
- Digest JSON hardening → Task 4 ✓
- Real ISO-8601 → Task 5 ✓
- Frontend `setSettings` contract + error UI → Task 6 ✓
- Security review gate → Task 7 ✓
- `SecretStore` trait + `MemSecretStore` test double → Task 1 ✓
- Config in app-config dir, JSON, key omitted from file → Task 1 (`PersistedSettings`) ✓
- Error handling (keychain fail aborts; missing key → empty; malformed JSON → defaults) → Task 1 `load`/`save`, Task 3 order ✓

**Type consistency:** `ConfigStore::new(dir, Box<dyn SecretStore>)`, `save(&Settings) -> Result<()>`, `load() -> Settings`, `initial_index(&str) -> Vec<IndexEntry>`, `set_settings -> Result<(), String>`, `extract_json(&str) -> &str` — all consistent across tasks.

**Placeholder scan:** No TBD/TODO; every code step shows full code; every run step shows the command + expected result.

**Out of scope (deferred):** `Browse.svelte` reactive cleanup (cosmetic, per spec); M2–M4.
