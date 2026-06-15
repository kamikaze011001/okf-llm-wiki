# OKF LLM Wiki v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the v1 thin slice — paste a URL or text → auto-digest into OKF Markdown pages → Browse + Ask over them — in a local-first Tauri desktop app.

**Architecture:** A Tauri 2 shell. A **Rust core** owns the filesystem, LLM/embedding calls (so API keys never reach the webview), a background digest job, and a brute-force vector search. A **Svelte 5 + Vite + TypeScript** frontend renders a neo-brutalist UI and talks to the core via Tauri commands. The user's wiki is a plain OKF folder (Markdown + YAML frontmatter); a derived search index lives in the app data dir.

**Tech Stack:** Tauri 2, Rust (tokio, reqwest, serde, serde_yaml, gray_matter, scraper, async-trait, anyhow), Svelte 5 + Vite + TypeScript, Vitest, `cargo test`.

---

## Conventions used by every task

- **Wiki folder layout:** `index.md`, `log.md`, `concepts/<slug>.md`, `sources/<slug>.md`.
- **Rust crate** lives in `src-tauri/`. Core logic is in library modules under `src-tauri/src/core/` so it is unit-testable without Tauri. Tests live in `#[cfg(test)]` modules or `src-tauri/tests/`.
- **Run Rust tests:** `cd src-tauri && cargo test`.
- **Run frontend tests:** `npm run test` (Vitest).
- **Commit** after every green step.

### Shared Rust types (defined in Task 2, referenced everywhere)

```rust
// src-tauri/src/core/page.rs
use std::collections::BTreeMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)] pub title: Option<String>,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub tags: Vec<String>,
    #[serde(default)] pub resource: Option<String>,
    #[serde(default)] pub timestamp: Option<String>,
    #[serde(default)] pub note: Option<String>,
    #[serde(flatten)] pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub path: String,   // relative to wiki root, e.g. "concepts/vitamin-d-sleep.md"
    pub frontmatter: Frontmatter,
    pub body: String,   // markdown after the frontmatter block
}
```

---

## Task 0: Scaffold Tauri + Svelte project

**Files:**
- Create: `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `src/main.ts`, `src/App.svelte`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Create: `vitest.config.ts`

- [ ] **Step 1: Scaffold with the Tauri CLI**

Run:
```bash
npm create tauri-app@latest . -- --template svelte-ts --manager npm --yes
npm install
```
Expected: a Tauri+Svelte+TS project; `src-tauri/` Rust crate present.

- [ ] **Step 2: Add Rust dependencies**

Edit `src-tauri/Cargo.toml`, add under `[dependencies]`:
```toml
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
gray_matter = "0.2"
scraper = "0.20"
async-trait = "0.1"
anyhow = "1"
```

- [ ] **Step 3: Add Vitest**

Run: `npm install -D vitest @testing-library/svelte jsdom`
Create `vitest.config.ts`:
```ts
import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
export default defineConfig({
  plugins: [svelte({ hot: false })],
  test: { environment: "jsdom", globals: true },
});
```
Add to `package.json` scripts: `"test": "vitest run"`.

- [ ] **Step 4: Verify both toolchains build**

Run: `cd src-tauri && cargo build` → Expected: compiles.
Run: `npm run test` → Expected: "no test files" (exit 0) or passes.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: scaffold Tauri + Svelte + TS project"
```

---

## Task 1: OKF slug + path helpers

**Files:**
- Create: `src-tauri/src/core/mod.rs`, `src-tauri/src/core/slug.rs`
- Test: in `slug.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

`src-tauri/src/core/slug.rs`:
```rust
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slugifies_title() {
        assert_eq!(slugify("Vitamin D & Sleep!"), "vitamin-d-sleep");
        assert_eq!(slugify("  Hello   World  "), "hello-world");
    }
}
```
Add to `src-tauri/src/core/mod.rs`: `pub mod slug;`
Wire `pub mod core;` into `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test slug`
Expected: PASS (implementation written alongside test — trivial pure function).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/core
git commit -m "feat(core): add slugify helper"
```

---

## Task 2: OKF Store — read/write pages with frontmatter

**Files:**
- Create: `src-tauri/src/core/page.rs` (types from "Shared Rust types" above)
- Create: `src-tauri/src/core/store.rs`
- Test: `src-tauri/src/core/store.rs` `#[cfg(test)]`

- [ ] **Step 1: Add the shared types**

Create `src-tauri/src/core/page.rs` with the exact `Frontmatter` and `Page` structs from the "Shared Rust types" section. Add `pub mod page;` to `core/mod.rs`.

- [ ] **Step 2: Write the failing test**

`src-tauri/src/core/store.rs`:
```rust
use crate::core::page::{Page, Frontmatter};
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};

pub struct OkfStore { root: PathBuf }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("okf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_then_read_roundtrips() {
        let store = OkfStore::new(tmp());
        let page = Page {
            path: "concepts/vitamin-d-sleep.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Vitamin D & Sleep".into()),
                description: None, tags: vec!["sleep".into()],
                resource: None, timestamp: None, note: Some("winter insomnia".into()),
                extra: BTreeMap::new(),
            },
            body: "**TL;DR.** Take it in the morning.".into(),
        };
        store.write_page(&page).unwrap();
        let read = store.read_page("concepts/vitamin-d-sleep.md").unwrap();
        assert_eq!(read.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert_eq!(read.frontmatter.tags, vec!["sleep".to_string()]);
        assert!(read.body.contains("morning"));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd src-tauri && cargo test store::tests::write_then_read`
Expected: FAIL — `OkfStore::new` / `write_page` / `read_page` not found.

- [ ] **Step 4: Implement OkfStore (atomic write, frontmatter parse)**

Add to `src-tauri/src/core/store.rs` (above the tests):
```rust
impl OkfStore {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }

    pub fn write_page(&self, page: &Page) -> Result<()> {
        let full = self.root.join(&page.path);
        if let Some(parent) = full.parent() { std::fs::create_dir_all(parent)?; }
        let yaml = serde_yaml::to_string(&page.frontmatter)?;
        let contents = format!("---\n{}---\n\n{}\n", yaml, page.body.trim_end());
        let tmp = full.with_extension("md.tmp");
        std::fs::write(&tmp, contents)?;            // atomic: write temp then rename
        std::fs::rename(&tmp, &full)?;
        Ok(())
    }

    pub fn read_page(&self, rel: &str) -> Result<Page> {
        let full = self.root.join(rel);
        let raw = std::fs::read_to_string(&full)
            .with_context(|| format!("reading {rel}"))?;
        let parsed = gray_matter::Matter::<gray_matter::engine::YAML>::new().parse(&raw);
        let fm: Frontmatter = parsed.data
            .ok_or_else(|| anyhow::anyhow!("missing frontmatter in {rel}"))?
            .deserialize()?;
        Ok(Page { path: rel.to_string(), frontmatter: fm, body: parsed.content })
    }

    pub fn list_pages(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for entry in walk(&self.root, &self.root)? {
            if entry.ends_with(".md") && entry != "index.md" && entry != "log.md" {
                out.push(entry);
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn append_log(&self, entry: &str) -> Result<()> {
        use std::io::Write;
        let path = self.root.join("log.md");
        std::fs::create_dir_all(&self.root)?;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(f, "- {entry}")?;
        Ok(())
    }
}

fn walk(dir: &Path, root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if !dir.exists() { return Ok(out); }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() { out.extend(walk(&p, root)?); }
        else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(out)
}
```
Add `pub mod store;` to `core/mod.rs`.

- [ ] **Step 5: Run test to verify it passes**

Run: `cd src-tauri && cargo test store::`
Expected: PASS.

- [ ] **Step 6: Add list_pages + append_log tests, verify, commit**

Add tests asserting `list_pages` excludes `index.md`/`log.md`, and `append_log` appends two lines. Run `cargo test store::`. Expected: PASS.
```bash
git add src-tauri/src/core
git commit -m "feat(core): OKF store with atomic write, frontmatter parse, log append"
```

---

## Task 3: LLMProvider trait + fake provider for tests

**Files:**
- Create: `src-tauri/src/core/provider/mod.rs`
- Create: `src-tauri/src/core/provider/fake.rs`
- Test: `fake.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

`src-tauri/src/core/provider/mod.rs`:
```rust
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

pub mod fake;
```
`src-tauri/src/core/provider/fake.rs`:
```rust
use super::LlmProvider;
use anyhow::Result;
use async_trait::async_trait;

/// Deterministic provider for tests. `complete` returns `reply`;
/// `embed` returns a tiny bag-of-chars vector so similar text scores higher.
pub struct FakeProvider { pub reply: String }

#[async_trait]
impl LlmProvider for FakeProvider {
    async fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        Ok(self.reply.clone())
    }
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0f32; 26];
        for c in text.to_ascii_lowercase().chars() {
            if c.is_ascii_lowercase() { v[(c as u8 - b'a') as usize] += 1.0; }
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fake_completes_and_embeds() {
        let p = FakeProvider { reply: "ok".into() };
        assert_eq!(p.complete("s", "u").await.unwrap(), "ok");
        assert_eq!(p.embed("abc").await.unwrap().len(), 26);
    }
}
```
Add `pub mod provider;` to `core/mod.rs`. Add `tokio` test feature is already on.

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test provider::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/core/provider
git commit -m "feat(core): LlmProvider trait + deterministic fake provider"
```

---

## Task 4: Claude provider adapter

**Files:**
- Create: `src-tauri/src/core/provider/claude.rs`
- Test: `claude.rs` `#[cfg(test)]` (request-building only; no network)

- [ ] **Step 1: Write the failing test (request body shape)**

`src-tauri/src/core/provider/claude.rs`:
```rust
use super::LlmProvider;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ClaudeProvider {
    pub api_key: String,
    pub model: String,       // e.g. "claude-opus-4-8"
    pub client: reqwest::Client,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model, client: reqwest::Client::new() }
    }
    pub(crate) fn messages_body(&self, system: &str, user: &str) -> Value {
        json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system,
            "messages": [{ "role": "user", "content": user }]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_messages_body() {
        let p = ClaudeProvider::new("k".into(), "claude-opus-4-8".into());
        let b = p.messages_body("be brief", "hi");
        assert_eq!(b["model"], "claude-opus-4-8");
        assert_eq!(b["system"], "be brief");
        assert_eq!(b["messages"][0]["content"], "hi");
    }
}
```

- [ ] **Step 2: Run test to verify it fails, then passes**

Run: `cd src-tauri && cargo test claude::`
Expected: PASS once the struct compiles. (If it fails to compile, add `pub mod claude;` to `provider/mod.rs`.)

- [ ] **Step 3: Implement the trait (network calls)**

Append to `claude.rs`:
```rust
#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let resp = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.messages_body(system, user))
            .send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!("Claude API error {}: {}", resp.status(), resp.text().await.unwrap_or_default()));
        }
        let v: Value = resp.json().await?;
        v["content"][0]["text"].as_str().map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected Claude response shape"))
    }
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        // Anthropic has no first-party embeddings; v1 uses a local hashing embedder.
        Ok(crate::core::retrieval::hash_embed(_text))
    }
}
```
(Note: `hash_embed` is defined in Task 6. Until then, this references it — implement Task 6 before building the full app, or temporarily return `Ok(vec![])`.)

- [ ] **Step 4: Verify build, commit**

Run: `cd src-tauri && cargo test claude::`
Expected: PASS (network method not exercised in tests).
```bash
git add src-tauri/src/core/provider/claude.rs
git commit -m "feat(core): Claude provider adapter (messages API)"
```

---

## Task 5: Source Fetcher — URL → clean text, text passthrough

**Files:**
- Create: `src-tauri/src/core/fetch.rs`
- Test: `fetch.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

`src-tauri/src/core/fetch.rs`:
```rust
use anyhow::Result;

pub enum Source { Url(String), Text(String) }

pub fn classify(input: &str) -> Source {
    let t = input.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        Source::Url(t.to_string())
    } else {
        Source::Text(input.to_string())
    }
}

/// Extract readable text from an HTML document (paragraphs + headings).
pub fn html_to_text(html: &str) -> String {
    use scraper::{Html, Selector};
    let doc = Html::parse_document(html);
    let sel = Selector::parse("h1,h2,h3,p,li").unwrap();
    let mut out = Vec::new();
    for el in doc.select(&sel) {
        let txt: String = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if !txt.is_empty() { out.push(txt); }
    }
    out.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_url_vs_text() {
        assert!(matches!(classify("https://a.com"), Source::Url(_)));
        assert!(matches!(classify("just a note"), Source::Text(_)));
    }
    #[test]
    fn strips_html_to_readable_text() {
        let html = "<html><body><nav>Home</nav><h1>Title</h1><p>Hello world.</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world."));
        assert!(!text.contains("<p>"));
    }
}
```
Add `pub mod fetch;` to `core/mod.rs`.

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test fetch::`
Expected: PASS.

- [ ] **Step 3: Add async fetch_clean (network) + commit**

Append:
```rust
pub async fn fetch_clean(input: &str) -> Result<String> {
    match classify(input) {
        Source::Text(t) => Ok(t),
        Source::Url(u) => {
            let html = reqwest::get(&u).await?.text().await?;
            Ok(html_to_text(&html))
        }
    }
}
```
Run: `cd src-tauri && cargo test fetch::` → PASS.
```bash
git add src-tauri/src/core/fetch.rs
git commit -m "feat(core): source fetcher (url->text, text passthrough)"
```

---

## Task 6: Retrieval — hashing embedder + cosine search

**Files:**
- Create: `src-tauri/src/core/retrieval.rs`
- Test: `retrieval.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

`src-tauri/src/core/retrieval.rs`:
```rust
const DIM: usize = 256;

/// Deterministic local embedding: hashes word tokens into a fixed vector.
/// Keeps v1 fully offline and provider-independent for search.
pub fn hash_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() { continue; }
        let mut h: u64 = 1469598103934665603;
        for b in word.bytes() { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        v[(h as usize) % DIM] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 { for x in &mut v { *x /= norm; } }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Clone)]
pub struct IndexEntry { pub path: String, pub vector: Vec<f32>, pub snippet: String }

pub fn search<'a>(query: &str, entries: &'a [IndexEntry], k: usize) -> Vec<&'a IndexEntry> {
    let q = hash_embed(query);
    let mut scored: Vec<(f32, &IndexEntry)> =
        entries.iter().map(|e| (cosine(&q, &e.vector), e)).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored.into_iter().take(k).map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_most_relevant_page() {
        let entries = vec![
            IndexEntry { path: "a.md".into(), vector: hash_embed("vitamin d sleep melatonin"), snippet: "".into() },
            IndexEntry { path: "b.md".into(), vector: hash_embed("rust tauri desktop app"), snippet: "".into() },
        ];
        let hits = search("how does vitamin d affect sleep", &entries, 1);
        assert_eq!(hits[0].path, "a.md");
    }
}
```
Add `pub mod retrieval;` to `core/mod.rs`.

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test retrieval::`
Expected: PASS. Now revisit Task 4 Step 3 — `hash_embed` resolves.

- [ ] **Step 3: Add index build from store + persistence**

Append:
```rust
use crate::core::store::OkfStore;
use anyhow::Result;

pub fn build_index(store: &OkfStore) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    for path in store.list_pages()? {
        let page = store.read_page(&path)?;
        let text = format!("{} {}", page.frontmatter.title.clone().unwrap_or_default(), page.body);
        let snippet: String = page.body.chars().take(160).collect();
        entries.push(IndexEntry { path, vector: hash_embed(&text), snippet });
    }
    Ok(entries)
}
```
Add a test building an index from a temp store with two pages and asserting two entries. Run `cargo test retrieval::` → PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/core/retrieval.rs
git commit -m "feat(core): retrieval (hashing embedder, cosine search, index build)"
```

---

## Task 7: Digest Service — turn source text into OKF pages

**Files:**
- Create: `src-tauri/src/core/digest.rs`
- Test: `digest.rs` `#[cfg(test)]` (uses FakeProvider)

- [ ] **Step 1: Write the failing test**

`src-tauri/src/core/digest.rs`:
```rust
use crate::core::page::{Page, Frontmatter};
use crate::core::provider::LlmProvider;
use crate::core::slug::slugify;
use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::BTreeMap;

/// JSON contract the LLM must return.
#[derive(Deserialize)]
struct DigestJson { title: String, description: String, tags: Vec<String>, body: String }

pub struct DigestResult { pub page: Page, pub log_entry: String }

pub async fn digest(
    provider: &dyn LlmProvider,
    source_text: &str,
    resource: Option<&str>,
    note: Option<&str>,
) -> Result<DigestResult> {
    let system = "You write one OKF wiki page from a source. \
        Respond ONLY with JSON: {\"title\":..,\"description\":..,\"tags\":[..],\"body\":..}. \
        The body is Markdown beginning with a bold TL;DR line, then '## Key points'.";
    let user = format!("SOURCE:\n{source_text}\n\nUSER NOTE: {}", note.unwrap_or(""));
    let raw = provider.complete(system, &user).await?;
    let parsed: DigestJson = serde_json::from_str(raw.trim())
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
    let slug = slugify(&parsed.title);
    let page = Page {
        path: format!("concepts/{slug}.md"),
        frontmatter: Frontmatter {
            type_: "Concept".into(),
            title: Some(parsed.title.clone()),
            description: Some(parsed.description),
            tags: parsed.tags,
            resource: resource.map(|s| s.to_string()),
            timestamp: Some(now_iso()),
            note: note.map(|s| s.to_string()),
            extra: BTreeMap::new(),
        },
        body: parsed.body,
    };
    Ok(DigestResult { log_entry: format!("Added page: {}", parsed.title), page })
}

fn now_iso() -> String {
    // Minimal RFC3339-ish stamp without extra deps.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("unixtime:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::fake::FakeProvider;
    #[tokio::test]
    async fn produces_concept_page_from_llm_json() {
        let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
        let p = FakeProvider { reply: reply.into() };
        let r = digest(&p, "some source", Some("https://x"), Some("winter")).await.unwrap();
        assert_eq!(r.page.path, "concepts/vitamin-d-sleep.md");
        assert_eq!(r.page.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert_eq!(r.page.frontmatter.note, Some("winter".into()));
        assert!(r.page.body.contains("TL;DR"));
    }
}
```
Add `pub mod digest;` to `core/mod.rs`.

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test digest::`
Expected: PASS.

- [ ] **Step 3: Add a malformed-JSON test, verify graceful error, commit**

Add a test where `FakeProvider.reply = "not json"` and assert `digest(...).await.is_err()`. Run `cargo test digest::` → PASS.
```bash
git add src-tauri/src/core/digest.rs
git commit -m "feat(core): digest service (source text -> OKF concept page via LLM JSON)"
```

---

## Task 8: Ask Service — retrieve context, answer with citations

**Files:**
- Create: `src-tauri/src/core/ask.rs`
- Test: `ask.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

`src-tauri/src/core/ask.rs`:
```rust
use crate::core::provider::LlmProvider;
use crate::core::retrieval::{IndexEntry, search};
use anyhow::Result;

pub struct Answer { pub text: String, pub citations: Vec<String> }

pub async fn ask(
    provider: &dyn LlmProvider,
    question: &str,
    index: &[IndexEntry],
) -> Result<Answer> {
    let hits = search(question, index, 4);
    let citations: Vec<String> = hits.iter().map(|h| h.path.clone()).collect();
    let context = hits.iter()
        .map(|h| format!("[{}]\n{}", h.path, h.snippet))
        .collect::<Vec<_>>().join("\n\n");
    let system = "Answer ONLY from the provided wiki context. Cite page paths in [brackets]. \
        If the context is insufficient, say so.";
    let user = format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}");
    let text = provider.complete(system, &user).await?;
    Ok(Answer { text, citations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::fake::FakeProvider;
    use crate::core::retrieval::hash_embed;
    #[tokio::test]
    async fn answers_and_returns_citations() {
        let index = vec![
            IndexEntry { path: "concepts/sleep.md".into(), vector: hash_embed("vitamin d sleep"), snippet: "morning dose".into() },
        ];
        let p = FakeProvider { reply: "Take it in the morning [concepts/sleep.md]".into() };
        let a = ask(&p, "vitamin d sleep timing", &index).await.unwrap();
        assert!(a.text.contains("morning"));
        assert_eq!(a.citations, vec!["concepts/sleep.md".to_string()]);
    }
}
```
Add `pub mod ask;` to `core/mod.rs`.

- [ ] **Step 2: Run test to verify it passes, commit**

Run: `cd src-tauri && cargo test ask::` → PASS.
```bash
git add src-tauri/src/core/ask.rs
git commit -m "feat(core): ask service (retrieve context + cite pages)"
```

---

## Task 9: App state, settings, and provider factory

**Files:**
- Create: `src-tauri/src/core/settings.rs`
- Create: `src-tauri/src/state.rs`
- Test: `settings.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing test for settings serialization**

`src-tauri/src/core/settings.rs`:
```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub provider: String,    // "claude" | "openai" | "ollama"
    pub model: String,
    pub api_key: String,
    pub wiki_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { provider: "claude".into(), model: "claude-opus-4-8".into(),
               api_key: String::new(), wiki_path: String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_json() {
        let s = Settings { provider: "claude".into(), model: "m".into(), api_key: "k".into(), wiki_path: "/w".into() };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&j).unwrap(), s);
    }
}
```
Add `pub mod settings;` to `core/mod.rs`.

- [ ] **Step 2: Run test → PASS**

Run: `cd src-tauri && cargo test settings::`

- [ ] **Step 3: Build the provider factory**

Append to `settings.rs`:
```rust
use crate::core::provider::{LlmProvider, claude::ClaudeProvider};
use std::sync::Arc;
use anyhow::{Result, anyhow};

pub fn make_provider(s: &Settings) -> Result<Arc<dyn LlmProvider>> {
    match s.provider.as_str() {
        "claude" => Ok(Arc::new(ClaudeProvider::new(s.api_key.clone(), s.model.clone()))),
        other => Err(anyhow!("provider '{other}' not yet supported in v1")),
    }
}
```

- [ ] **Step 4: Define shared app state**

`src-tauri/src/state.rs`:
```rust
use crate::core::settings::Settings;
use crate::core::retrieval::IndexEntry;
use std::sync::Mutex;

#[derive(Default)]
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub index: Mutex<Vec<IndexEntry>>,
}
```
Add `mod state;` to `lib.rs`.

- [ ] **Step 5: Verify build, commit**

Run: `cd src-tauri && cargo test settings::` → PASS; `cargo build` → compiles.
```bash
git add src-tauri/src
git commit -m "feat(core): settings, provider factory, shared app state"
```

---

## Task 10: Tauri commands (the frontend bridge)

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (register commands + manage state)
- Test: `src-tauri/tests/commands_integration.rs`

- [ ] **Step 1: Write the integration test (full loop, fake provider)**

`src-tauri/tests/commands_integration.rs`:
```rust
use okf_llm_wiki_lib::core::{store::OkfStore, digest::digest, retrieval::build_index, ask::ask};
use okf_llm_wiki_lib::core::provider::fake::FakeProvider;

#[tokio::test]
async fn full_loop_digest_then_ask() {
    let dir = std::env::temp_dir().join("okf-int-test");
    let _ = std::fs::remove_dir_all(&dir);
    let store = OkfStore::new(&dir);

    // Digest a source.
    let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
    let dp = FakeProvider { reply: reply.into() };
    let r = digest(&dp, "source about sleep", Some("https://x"), Some("note")).await.unwrap();
    store.write_page(&r.page).unwrap();
    store.append_log(&r.log_entry).unwrap();

    // Build index and ask.
    let index = build_index(&store).unwrap();
    let ap = FakeProvider { reply: "Morning dose [concepts/vitamin-d-sleep.md]".into() };
    let a = ask(&ap, "vitamin d sleep", &index).await.unwrap();
    assert!(a.text.contains("Morning"));
    assert_eq!(a.citations, vec!["concepts/vitamin-d-sleep.md".to_string()]);
}
```
(Set the lib name: ensure `src-tauri/Cargo.toml` has `[lib] name = "okf_llm_wiki_lib"`. Adjust the `use` path if your crate name differs.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test commands_integration`
Expected: FAIL until modules are `pub` and lib name matches.

- [ ] **Step 3: Make core modules public + fix lib name**

In `src-tauri/src/lib.rs` ensure `pub mod core;`. In `Cargo.toml` confirm `[lib] name = "okf_llm_wiki_lib"` and that `core/mod.rs` re-exports each submodule with `pub`.

- [ ] **Step 4: Run integration test → PASS**

Run: `cd src-tauri && cargo test --test commands_integration`
Expected: PASS.

- [ ] **Step 5: Implement Tauri commands**

`src-tauri/src/commands.rs`:
```rust
use crate::state::AppState;
use crate::core::{store::OkfStore, settings::{Settings, make_provider}, retrieval::build_index, digest::digest, ask::ask, fetch::fetch_clean};
use tauri::State;
use serde::Serialize;

#[derive(Serialize)]
pub struct PageDto { pub path: String, pub title: String, pub body: String, pub tags: Vec<String>, pub note: Option<String>, pub resource: Option<String> }

fn store(state: &State<AppState>) -> OkfStore {
    OkfStore::new(state.settings.lock().unwrap().wiki_path.clone())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings { state.settings.lock().unwrap().clone() }

#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: Settings) {
    *state.settings.lock().unwrap() = settings;
}

#[tauri::command]
pub fn list_pages(state: State<AppState>) -> Result<Vec<PageDto>, String> {
    let s = store(&state);
    let mut out = Vec::new();
    for path in s.list_pages().map_err(|e| e.to_string())? {
        let p = s.read_page(&path).map_err(|e| e.to_string())?;
        out.push(PageDto { path: p.path, title: p.frontmatter.title.unwrap_or_default(),
            body: p.body, tags: p.frontmatter.tags, note: p.frontmatter.note, resource: p.frontmatter.resource });
    }
    Ok(out)
}

#[tauri::command]
pub async fn submit_source(state: State<'_, AppState>, input: String, note: Option<String>) -> Result<PageDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let provider = make_provider(&settings).map_err(|e| e.to_string())?;
    let clean = fetch_clean(&input).await.map_err(|e| e.to_string())?;
    let resource = input.starts_with("http").then(|| input.clone());
    let r = digest(provider.as_ref(), &clean, resource.as_deref(), note.as_deref()).await.map_err(|e| e.to_string())?;
    let s = OkfStore::new(settings.wiki_path.clone());
    s.write_page(&r.page).map_err(|e| e.to_string())?;
    s.append_log(&r.log_entry).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = build_index(&s).map_err(|e| e.to_string())?;
    Ok(PageDto { path: r.page.path, title: r.page.frontmatter.title.unwrap_or_default(),
        body: r.page.body, tags: r.page.frontmatter.tags, note: r.page.frontmatter.note, resource: r.page.frontmatter.resource })
}

#[derive(Serialize)]
pub struct AnswerDto { pub text: String, pub citations: Vec<String> }

#[tauri::command]
pub async fn ask_question(state: State<'_, AppState>, question: String) -> Result<AnswerDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let provider = make_provider(&settings).map_err(|e| e.to_string())?;
    let index = state.index.lock().unwrap().clone();
    let a = ask(provider.as_ref(), &question, &index).await.map_err(|e| e.to_string())?;
    Ok(AnswerDto { text: a.text, citations: a.citations })
}
```

- [ ] **Step 6: Register in lib.rs**

In `src-tauri/src/lib.rs` `run()`:
```rust
mod commands;
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings, commands::set_settings, commands::list_pages,
            commands::submit_source, commands::ask_question
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```
Run: `npm install @tauri-apps/plugin-notification` and `cargo add tauri-plugin-notification` (in `src-tauri`).

- [ ] **Step 7: Verify build + integration test, commit**

Run: `cd src-tauri && cargo test` → PASS; `cargo build` → compiles.
```bash
git add -A
git commit -m "feat: tauri commands (submit_source, ask, list_pages, settings) + integration test"
```

---

## Task 11: Frontend — API client + Svelte stores

**Files:**
- Create: `src/lib/api.ts`
- Create: `src/lib/stores.ts`
- Test: `src/lib/api.test.ts`

- [ ] **Step 1: Write the failing test (mocked invoke)**

`src/lib/api.test.ts`:
```ts
import { describe, it, expect, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (_cmd, args) => ({ echoed: args })) }));
import { listPages, submitSource } from "./api";

describe("api", () => {
  it("submitSource passes input and note", async () => {
    const r: any = await submitSource("https://x", "my note");
    expect(r.echoed).toEqual({ input: "https://x", note: "my note" });
  });
  it("listPages calls through", async () => {
    await expect(listPages()).resolves.toBeDefined();
  });
});
```

- [ ] **Step 2: Implement the client**

`src/lib/api.ts`:
```ts
import { invoke } from "@tauri-apps/api/core";

export interface PageDto { path: string; title: string; body: string; tags: string[]; note?: string; resource?: string; }
export interface AnswerDto { text: string; citations: string[]; }
export interface Settings { provider: string; model: string; api_key: string; wiki_path: string; }

export const listPages = () => invoke<PageDto[]>("list_pages");
export const submitSource = (input: string, note?: string) => invoke<PageDto>("submit_source", { input, note });
export const askQuestion = (question: string) => invoke<AnswerDto>("ask_question", { question });
export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<void>("set_settings", { settings });
```

- [ ] **Step 3: Run test → PASS; add stores**

`src/lib/stores.ts`:
```ts
import { writable } from "svelte/store";
export type Route = "home" | "capture" | "browse" | "ask" | "settings";
export const route = writable<Route>("home");
export const currentPage = writable<string | null>(null);
```
Run: `npm run test` → PASS.

- [ ] **Step 4: Commit**

```bash
git add src/lib
git commit -m "feat(ui): typed Tauri API client + navigation stores"
```

---

## Task 12: Frontend — neo-brutalist theme + app shell (rail + Home)

**Files:**
- Create: `src/styles/neobrutal.css`
- Modify: `src/App.svelte`
- Create: `src/components/Rail.svelte`, `src/components/Home.svelte`

- [ ] **Step 1: Write the neo-brutalist design tokens**

`src/styles/neobrutal.css` (import in `src/main.ts`):
```css
:root{
  --ink:#111; --paper:#f4f1e8; --blue:#2563ff; --yellow:#ffd400; --pink:#ff4d6d; --green:#15c26b;
  --shadow:6px 6px 0 var(--ink); --border:3px solid var(--ink);
  font-family: "Inter", system-ui, sans-serif; color:var(--ink); background:var(--paper);
}
*{box-sizing:border-box}
.nb-card{border:var(--border);background:#fff;box-shadow:var(--shadow);padding:16px}
.nb-btn{border:var(--border);background:#fff;box-shadow:3px 3px 0 var(--ink);font-weight:800;
  text-transform:uppercase;font-size:12px;padding:8px 14px;cursor:pointer}
.nb-btn:active{box-shadow:none;transform:translate(3px,3px)}
.nb-btn.accent{background:var(--blue);color:#fff}
.nb-input{border:var(--border);background:#fff;padding:12px;font-size:15px;width:100%}
h1,h2,h3{font-weight:900;text-transform:uppercase;letter-spacing:-.5px}
.nb-chip{display:inline-block;border:2px solid var(--ink);font-weight:700;font-size:11px;padding:2px 8px;margin:0 5px 5px 0}
```
**Constraint reminder:** no gradients, no blur/glass, no soft shadows, no emoji confetti. See spec §7.

- [ ] **Step 2: Build the Rail**

`src/components/Rail.svelte`:
```svelte
<script lang="ts">
  import { route, type Route } from "../lib/stores";
  const items: {id: Route; label: string}[] = [
    {id:"home",label:"Home"},{id:"capture",label:"＋ Capture"},
    {id:"browse",label:"Browse"},{id:"ask",label:"Ask"},{id:"settings",label:"⚙ Settings"},
  ];
</script>
<nav style="width:150px;border-right:var(--border);background:var(--yellow);padding:12px;min-height:100vh">
  {#each items as it}
    <button class="nb-btn" style="display:block;width:100%;margin-bottom:8px;{$route===it.id?'background:var(--blue);color:#fff':''}"
      on:click={() => route.set(it.id)}>{it.label}</button>
  {/each}
</nav>
```

- [ ] **Step 3: Build Home (calm capture/ask bar + recent pages)**

`src/components/Home.svelte`:
```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, submitSource, type PageDto } from "../lib/api";
  import { route, currentPage } from "../lib/stores";
  let input = ""; let note = ""; let busy = false; let pages: PageDto[] = [];
  onMount(async () => { pages = await listPages(); });
  async function go() {
    if (!input.trim()) return;
    busy = true;
    try { await submitSource(input, note || undefined); input=""; note=""; pages = await listPages(); }
    finally { busy = false; }
  }
</script>
<section style="padding:32px;max-width:720px;margin:0 auto">
  <h1>Good morning</h1>
  <div class="nb-card" style="margin:16px 0">
    <input class="nb-input" placeholder="Paste a link or write a note…" bind:value={input} />
    <input class="nb-input" style="margin-top:8px" placeholder="Why are you saving this? (optional)" bind:value={note} />
    <button class="nb-btn accent" style="margin-top:12px" on:click={go} disabled={busy}>{busy ? "Digesting…" : "Capture"}</button>
  </div>
  <h3>Recent</h3>
  {#each pages as p}
    <button class="nb-card" style="display:block;width:100%;text-align:left;margin-bottom:8px;cursor:pointer"
      on:click={() => { currentPage.set(p.path); route.set("browse"); }}>
      <strong>{p.title}</strong>
      <div>{#each p.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
    </button>
  {/each}
</section>
```

- [ ] **Step 4: Wire App.svelte**

`src/App.svelte`:
```svelte
<script lang="ts">
  import Rail from "./components/Rail.svelte";
  import Home from "./components/Home.svelte";
  import Browse from "./components/Browse.svelte";
  import Ask from "./components/Ask.svelte";
  import Settings from "./components/Settings.svelte";
  import { route } from "./lib/stores";
</script>
<main style="display:flex">
  <Rail />
  <div style="flex:1">
    {#if $route==="home"}<Home />{/if}
    {#if $route==="capture"}<Home />{/if}
    {#if $route==="browse"}<Browse />{/if}
    {#if $route==="ask"}<Ask />{/if}
    {#if $route==="settings"}<Settings />{/if}
  </div>
</main>
```

- [ ] **Step 5: Verify + commit**

Run: `npm run test` → PASS (no regressions). (Browse/Ask/Settings created next task; create empty stubs returning a heading so this compiles, or implement Task 13 before running `tauri dev`.)
```bash
git add src/styles src/components/Rail.svelte src/components/Home.svelte src/App.svelte src/main.ts
git commit -m "feat(ui): neo-brutalist theme, rail, and Home capture screen"
```

---

## Task 13: Frontend — Browse (page view), Ask, Settings

**Files:**
- Create: `src/components/Browse.svelte`, `src/components/Ask.svelte`, `src/components/Settings.svelte`

- [ ] **Step 1: Browse — TL;DR-first page view**

`src/components/Browse.svelte`:
```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { listPages, type PageDto } from "../lib/api";
  import { currentPage } from "../lib/stores";
  let pages: PageDto[] = []; let selected: PageDto | undefined;
  onMount(async () => { pages = await listPages(); pick(); });
  $: pick();
  function pick(){ selected = pages.find(p => p.path === $currentPage) ?? pages[0]; }
</script>
<section style="padding:32px;max-width:760px;margin:0 auto">
  {#if selected}
    <span class="nb-chip" style="background:var(--pink);color:#fff">CONCEPT</span>
    <h1>{selected.title}</h1>
    <div>{#each selected.tags as t}<span class="nb-chip">#{t}</span>{/each}</div>
    {#if selected.note}<div class="nb-card" style="background:var(--yellow);margin:12px 0"><strong>★ Your note:</strong> {selected.note}</div>{/if}
    <article class="nb-card" style="margin-top:12px;white-space:pre-wrap">{selected.body}</article>
    {#if selected.resource}<p style="margin-top:12px"><a href={selected.resource} target="_blank">Open source ↗</a></p>{/if}
  {:else}
    <p>No pages yet — capture something from Home.</p>
  {/if}
</section>
```

- [ ] **Step 2: Ask — chat over the wiki with citations**

`src/components/Ask.svelte`:
```svelte
<script lang="ts">
  import { askQuestion, type AnswerDto } from "../lib/api";
  let q = ""; let busy = false; let answer: AnswerDto | undefined;
  async function send(){ if(!q.trim()) return; busy = true; try { answer = await askQuestion(q); } finally { busy = false; } }
</script>
<section style="padding:32px;max-width:720px;margin:0 auto">
  <h1>Ask your wiki</h1>
  <div class="nb-card">
    <input class="nb-input" placeholder="Ask anything from your knowledge…" bind:value={q} on:keydown={(e)=> e.key==="Enter" && send()} />
    <button class="nb-btn accent" style="margin-top:12px" on:click={send} disabled={busy}>{busy?"Thinking…":"Ask"}</button>
  </div>
  {#if answer}
    <article class="nb-card" style="margin-top:16px;white-space:pre-wrap">{answer.text}</article>
    <h3 style="margin-top:12px">Sources</h3>
    {#each answer.citations as c}<span class="nb-chip">{c}</span>{/each}
  {/if}
</section>
```

- [ ] **Step 3: Settings — provider/model/key/folder**

`src/components/Settings.svelte`:
```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { getSettings, setSettings, type Settings } from "../lib/api";
  let s: Settings = { provider:"claude", model:"claude-opus-4-8", api_key:"", wiki_path:"" };
  let saved = false;
  onMount(async () => { s = await getSettings(); });
  async function save(){ await setSettings(s); saved = true; setTimeout(()=>saved=false, 1500); }
</script>
<section style="padding:32px;max-width:560px;margin:0 auto">
  <h1>Settings</h1>
  <div class="nb-card" style="display:grid;gap:10px">
    <label>Provider<select class="nb-input" bind:value={s.provider}><option>claude</option><option>openai</option><option>ollama</option></select></label>
    <label>Model<input class="nb-input" bind:value={s.model} /></label>
    <label>API key<input class="nb-input" type="password" bind:value={s.api_key} /></label>
    <label>Wiki folder<input class="nb-input" bind:value={s.wiki_path} placeholder="/Users/you/wiki" /></label>
    <button class="nb-btn accent" on:click={save}>{saved?"Saved ✓":"Save"}</button>
  </div>
</section>
```

- [ ] **Step 4: Verify build + run, commit**

Run: `npm run test` → PASS. Run: `npm run tauri dev` → app launches; set Settings (folder + key), capture a URL, see a page, ask a question.
```bash
git add src/components
git commit -m "feat(ui): Browse page view, Ask chat, Settings screens"
```

---

## Task 14: Manual verification + neo-brutalist design pass

**Files:** none (verification task)

- [ ] **Step 1: Run the full loop**

Run: `npm run tauri dev`. In Settings, set a real Claude key + a wiki folder. From Home, paste a real article URL with a note. Confirm: notification/"Digesting…" resolves, a page appears in Recent, Browse shows TL;DR-first with your note, Ask returns an answer citing the page.

- [ ] **Step 2: Design audit against spec §7**

Verify every screen uses 3px ink borders, hard offset shadows (no blur), flat accent blocks, heavy uppercase headings, mono only for raw OKF. No gradients/glass/soft cards. Fix any violations inline.

- [ ] **Step 3: Confirm OKF portability**

Open the wiki folder in a text editor. Confirm `concepts/*.md` have valid YAML frontmatter (`type`, `title`, `tags`, `note`) and `log.md` has entries. Confirm files are readable/usable without the app.

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "chore: v1 manual verification and neo-brutalist design pass"
```

---

## Self-Review notes (addressed)

- **Spec coverage:** capture (T10/T12), provider-agnostic interface (T3/T4/T9), OKF store (T2), digest (T7), retrieval (T6), Ask (T8/T13), Browse (T13), neo-brutalist UI (T12/T14), local-first/atomic writes (T2), settings (T9/T13). Feed/Quiz/Tours/PDF/YouTube intentionally deferred (spec §9).
- **Embeddings decision:** v1 uses a deterministic local **hashing embedder** (`hash_embed`) instead of a provider embeddings endpoint — keeps search offline, provider-independent, and dependency-free. Recorded as resolving spec §11's embeddings question for v1; a real embeddings adapter is a later upgrade.
- **Type consistency:** `Page`/`Frontmatter` (T2) reused by T7/T2; `IndexEntry`/`hash_embed`/`search` (T6) reused by T8/T9/T10; `PageDto`/`AnswerDto`/`Settings` shared between T10 (Rust) and T11/T13 (TS) with matching field names.
- **Folder layout:** `concepts/` + `sources/` confirmed; digest writes `concepts/<slug>.md` (sources/ reserved for the later per-source page split).
```
