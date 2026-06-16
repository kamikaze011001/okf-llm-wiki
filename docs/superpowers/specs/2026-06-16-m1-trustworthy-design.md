# M1 — Make v1 Trustworthy — Design Spec

> Milestone 1 of the roadmap (`docs/roadmap.md`). Makes okf-llm-wiki survive restarts and
> handle real Claude output, so it's usable as a daily driver.

**Date:** 2026-06-16 · **Status:** Approved (design) · **Branch:** `feat/m1-trustworthy`

---

## Goal

After M1: the app remembers your settings (incl. API key) across restarts, Ask works
immediately on launch, and digesting real Claude output does not fail on fenced/prose JSON.

## Problem (grounded in current code)

| Gap | Location | Symptom |
|---|---|---|
| Settings in-memory only | `state.rs` (`#[derive(Default)]`), `commands.rs:16-19` | API key + wiki folder reset on every restart |
| Index never built on startup | `commands.rs:43` (only `submit_source` rebuilds it) | Ask returns nothing after a restart |
| Brittle digest JSON parse | `digest.rs:25` (`serde_json::from_str(raw.trim())`) | Fenced/prose-wrapped LLM output → digest fails |
| Fake timestamps | `digest.rs:45-50` (`unixtime:NNN`) | OKF frontmatter not portable/sortable |

## Decisions

- **API key → OS keychain** (`keyring` crate); other settings → plaintext JSON. Key never on disk in plaintext.
- **Config format: JSON** (matches existing `serde` on `Settings`).
- **Config location: OS app-config dir** (not the wiki folder — `wiki_path` is itself a setting, so it can't live there).
- **Bundle** the startup-index build and ISO-8601 fix into M1 (both are "trustworthy"-themed correctness gaps).
- **Secret access behind a trait** so persistence is unit-testable without the real keychain.

## Architecture

Core stays Tauri-free (ADR-0001). The Tauri layer resolves the app-config dir and hands the path to the core.

```
lib.rs (setup)  ──app_config_dir()──►  core::config::ConfigStore { dir, secrets }
                                          ├─ save(&Settings) -> Result<()>
                                          └─ load() -> Settings

trait SecretStore { get(account)->Option<String>; set(account,&str)->Result<()>; delete(account)->Result<()> }
  ├─ KeyringSecretStore   → real OS keychain (keyring crate)   [production]
  └─ MemSecretStore       → in-memory HashMap                  [tests]
```

### Components

**`core/config.rs` (new)**
- `SecretStore` trait + `KeyringSecretStore` (service `"okf-llm-wiki"`, account `"api_key"`) + `MemSecretStore`.
- `ConfigStore { dir: PathBuf, secrets: Box<dyn SecretStore> }`
  - `save(&Settings)`: serialize `{provider, model, wiki_path}` (api_key blanked) → `<dir>/settings.json` (atomic temp+rename, mirroring `store.rs`); `secrets.set("api_key", &s.api_key)`.
  - `load() -> Settings`: read+parse `settings.json` (missing/malformed → `Settings::default()`); set `api_key` from `secrets.get("api_key")` (missing → empty).

**`state.rs` (modify)**
- `AppState` gains `config: ConfigStore`. Drop `#[derive(Default)]`; construct explicitly in setup.

**`lib.rs` (modify)**
- In the Tauri `setup` hook: resolve `app.path().app_config_dir()`, build `ConfigStore` (with `KeyringSecretStore`), `load()` settings, build the retrieval index if `wiki_path` is set, then `app.manage(AppState { settings, index, config })`.

**`commands.rs` (modify)**
- `set_settings` → `Result<(), String>`: update in-memory, `config.save()`, rebuild index from the new `wiki_path`.
- `get_settings` unchanged.

**`digest.rs` (modify)**
- `extract_json(raw: &str) -> &str`: if a ```` ``` ```` fenced block is present, return its inner content; else return the substring from the first `{` to its matching balanced `}`; else the trimmed input. Parse the result.
- `now_iso()` → real RFC-3339 via `time::OffsetDateTime::now_utc().format(&Rfc3339)`.

**`src/lib/api.ts` (modify)**
- `setSettings` now resolves to `void` from a `Result` (returns on success, throws the error string on failure) — keep the existing throw-on-error pattern used by other commands.

## Data flow

```
Startup:  load() → seed AppState.settings → (wiki_path set?) build_index → manage state
Save:     set_settings → in-memory update → config.save() (JSON + keychain) → rebuild index
Digest:   provider.complete → extract_json → serde_json::from_str → Page
```

## Error handling

- **Keychain write fails** on save → `set_settings` returns `Err(String)`; UI shows it. No silent plaintext fallback.
- **Keychain read fails / key absent** on load → `api_key` empty; user re-enters. App still launches.
- **Malformed `settings.json`** → fall back to `Settings::default()` (log, don't crash).
- **`extract_json` finds no JSON** → digest returns the existing "did not return valid digest JSON" error.

## Testing

- `core/config.rs`: `save`→`load` round-trip with `MemSecretStore` + temp dir (process-wide `AtomicU64` counter, per CLAUDE.md); assert `settings.json` on disk does **not** contain the key; missing-file load → defaults.
- `digest.rs`: `extract_json` for fenced / prose-wrapped / bare input all parse; malformed still errors (keep existing `errors_on_malformed_json`).
- `digest.rs`: timestamp matches an RFC-3339 shape.
- Frontend: `api.test.ts` reflects `setSettings` resolving on success / throwing on error.
- Regression: existing 17 Rust + 2 FE tests stay green; `cargo clippy --all-targets` clean; `cargo fmt`.

## Security review (required gate)

Touches `settings.rs` + `state.rs` (sensitive areas). Before commit, run `security-reviewer`:
- API key never written to `settings.json` (assert in test).
- Key never in logs, error strings, or panic messages.
- Keychain errors don't echo the secret.
- New deps (`keyring`, `time`) reviewed.

## Dependencies added

- `keyring` — OS keychain access.
- `time` (features: `formatting`) — RFC-3339 timestamps.

## Out of scope

M2 (real embeddings, persisted index, chunking) · M3 (`[[links]]`, backlinks, graph) ·
M4 (edit/delete, capture polish, onboarding) · the `Browse.svelte` reactive cleanup
(cosmetic — may be folded in opportunistically but is not a requirement).
