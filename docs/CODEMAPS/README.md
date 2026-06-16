# CODEMAPS — navigating okf-llm-wiki

A map of where things live and how to find them. Pair with `docs/architecture.md` (the "why") — this is the "where".

## Top-level layout

```
okf-llm-wiki/
├── src-tauri/            # Rust backend (Tauri 2)
│   ├── src/
│   │   ├── core/         # framework-agnostic domain logic  ← most business logic
│   │   ├── commands.rs   # Tauri #[command] IPC bridge       ← frontend entry into Rust
│   │   ├── state.rs      # AppState (settings + retrieval index)
│   │   ├── lib.rs        # Tauri builder + run()              ← app wiring
│   │   └── main.rs       # binary entry (calls lib::run)
│   ├── tests/            # Rust integration tests
│   └── Cargo.toml
├── src/                  # Frontend (SvelteKit SPA)
│   ├── lib/
│   │   ├── api.ts        # typed Tauri command client        ← wire contract (mirror of Rust DTOs)
│   │   ├── stores.ts     # route + currentPage navigation stores
│   │   ├── components/   # Rail, Home, Browse, Ask, Settings
│   │   └── styles/       # neobrutal.css design tokens
│   └── routes/           # +layout.svelte, +page.svelte (SPA shell), +layout.ts (ssr=false)
├── docs/
│   ├── architecture.md   # system design
│   ├── adr/              # architecture decision records
│   ├── CODEMAPS/         # this guide
│   └── superpowers/      # specs + implementation plans
└── CLAUDE.md             # harness instructions for Claude
```

## "I want to change X" → go here

| Task | Start in |
|---|---|
| Change how a source is digested into a page | `src-tauri/src/core/digest.rs` |
| Change OKF file format / read / write | `src-tauri/src/core/store.rs`, `core/page.rs` |
| Add/modify an LLM provider | `src-tauri/src/core/provider/` (+ `settings.rs::make_provider`) |
| Change retrieval / search behavior | `src-tauri/src/core/retrieval.rs` |
| Change how questions are answered | `src-tauri/src/core/ask.rs` |
| Change URL fetching / cleaning | `src-tauri/src/core/fetch.rs` |
| Add/modify a Tauri command | `src-tauri/src/commands.rs` (+ register in `lib.rs`, + add to `src/lib/api.ts`) |
| Change app state / shared index | `src-tauri/src/state.rs` |
| Change a screen's UI | `src/lib/components/<Screen>.svelte` |
| Change navigation / which screen shows | `src/routes/+page.svelte`, `src/lib/stores.ts` |
| Change design tokens / styling | `src/lib/styles/neobrutal.css` |
| Change the frontend↔backend contract | BOTH `src-tauri/src/commands.rs` (DTOs) AND `src/lib/api.ts` |

## Data flow cheat-sheet

```
Home.svelte ──submitSource()──► api.ts ──invoke──► commands::submit_source
   └─► fetch_clean → digest → store.write_page + append_log → build_index
Ask.svelte  ──askQuestion()──► api.ts ──invoke──► commands::ask_question
   └─► retrieval::search(top-k) → provider.complete → Answer{text, citations}
Browse.svelte ──listPages()──► api.ts ──invoke──► commands::list_pages → store.list_pages
```

## Conventions reminders

- Generated dirs — never edit: `.svelte-kit/`, `src-tauri/target/`, `build/`.
- Rust core stays Tauri-free; only `commands.rs` is Tauri-aware.
- Keep `PageDto`/`AnswerDto`/`Settings` field names identical on both sides of the IPC.
- See `CLAUDE.md` for the full gotchas + workflow gates.
