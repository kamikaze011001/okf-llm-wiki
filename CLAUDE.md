# okf-llm-wiki

Local-first, personal **LLM Wiki** / knowledge base. Paste a URL or note → an LLM digests it into an **Open Knowledge Format (OKF)** Markdown page on disk → Browse your pages and Ask questions answered over a local retrieval index. Provider-agnostic LLM layer (Claude wired for v1).

## Stack

- **Backend:** Rust + Tauri 2 (core domain + IPC commands)
- **Frontend:** SvelteKit + Svelte 5 (runes) + Vite + TypeScript — SPA mode (`adapter-static`, `ssr=false`)
- **Package manager:** npm
- **Tests:** `cargo test` (Rust) + vitest (frontend)
- **Formatting:** rustfmt (`cargo fmt`); no JS formatter configured

## Architecture (modular monolith)

| Layer | Path | Responsibility |
|---|---|---|
| Core domain | `src-tauri/src/core/` | OKF store (atomic IO), `LlmProvider` trait + Claude adapter, hashing-embedder retrieval, fetch, digest, ask, settings |
| IPC bridge | `src-tauri/src/commands.rs` | Tauri commands: `get_settings` / `set_settings` / `list_pages` / `submit_source` / `ask_question` |
| App shell (Rust) | `src-tauri/src/state.rs`, `lib.rs` | `AppState` (settings + index), Tauri builder + `run()` |
| Frontend lib | `src/lib/` | Typed Tauri API client (`api.ts`), navigation stores (`stores.ts`), neo-brutalist components |
| Frontend shell | `src/routes/` | SvelteKit SPA shell, store-driven routing |

**Entry points:** `src-tauri/src/lib.rs::run()`, `src/routes/+page.svelte`.

**External deps:** Claude API (LLM, via `Settings`), local filesystem (OKF wiki folder: `concepts/*.md` + `log.md`). No DB / queue / cache.

See `docs/architecture.md` for detail and `docs/adr/` for decisions.

## Commands

```bash
# Frontend
npm run dev            # vite dev server
npm run build          # build SPA (adapter-static → build/)
npm run check          # svelte-kit sync && svelte-check
npm run test           # vitest run

# Desktop app
npm run tauri dev      # launch the desktop app (needs a Claude API key in Settings)
npm run tauri build    # bundle the desktop app

# Rust (run inside src-tauri/)
cargo test
cargo clippy --all-targets
cargo fmt
```

**Build order:** `svelte-kit sync` (types) → `npm run build` (frontend) → `tauri build` (bundle).

## Gotchas (read before editing)

- **Never hold a `MutexGuard` across an `.await`** in `commands.rs` — the guards are not `Send`. Clone/drop the lock at the statement (`let x = state.lock().unwrap().clone();`) *before* any `.await`.
- **Generated — never edit by hand:** `.svelte-kit/`, `src-tauri/target/`, `build/`. If types are missing, run `svelte-kit sync`.
- **SPA only** (`ssr=false`): no server routes, no server-side `load`. All data flows through Tauri commands.
- **Settings (incl. API key) are in-memory only** — not persisted across app restarts. (Top v1.1 follow-up: persist them.)
- **Test isolation:** unit-test temp dirs use a process-wide `AtomicU64` counter (`okf-{pid}-{n}`), not PID-only, to avoid parallel-run collisions. Follow this pattern in new tests.

## Conventions

- **Commits:** Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`). End commit messages with the standard Co-Authored-By trailer.
- **Branches:** trunk-based — short-lived `feat/` `fix/` `chore/` `docs/` branches off `main`; never commit a new topic straight to `main`. When a branch's PR merges, that branch is done.
- **Rust:** keep `cargo clippy --all-targets` warning-free; `cargo fmt` before commit.
- **OKF files** are portable Markdown + YAML frontmatter — they must remain readable/usable without the app.

## Workflow gates

- **Security review for sensitive areas:** before committing changes that touch any of the sensitive areas below, run a security review (use the `security-reviewer` agent).

### Sensitive areas

- `src-tauri/src/core/settings.rs` — API key in `Settings`
- `src-tauri/src/state.rs` — key held in `AppState`
- `src-tauri/src/core/store.rs` — atomic file writes
- `src-tauri/src/core/provider/` — LLM provider boundary (outbound requests)
