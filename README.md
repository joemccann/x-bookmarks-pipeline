# X Bookmarks Pipeline (Rust)

This repository is now a **Rust-first** implementation of the multi-LLM bookmark pipeline that classifies X bookmarks, extracts chart signal context, and generates TradingView Pine Script v6 code.

The Rust implementation lives in `rust/` and is built with:

- `tokio` for async orchestration
- `reqwest` for HTTP clients
- `rusqlite` with `Arc<Mutex<Connection>>` for thread-safe caching
- `serde` for structured prompts/results
- `lettre` for native email notifications
- `thiserror` + `anyhow` for structured error handling

## Setup

```bash
cp .env.example .env
cd rust
cargo build
```

## Environment variables

- Required:
  - `CEREBRAS_API_KEY`
  - `XAI_API_KEY`
  - `ANTHROPIC_API_KEY`
  - `OPENAI_API_KEY`
  - `SMTP_HOST`
  - `SMTP_USER`
  - `SMTP_PASSWORD`
  - `SMTP_FROM`
  - `SMTP_TO`

- Optional: `CEREBRAS_MODEL`, `XAI_MODEL`, `ANTHROPIC_MODEL`, `OPENAI_MODEL`, `CACHE_PATH`, `MAX_WORKERS`, `API_TIMEOUT`, `VISION_TIMEOUT`, `FETCH_TIMEOUT`

## Build, test, and run

```bash
cd rust
cargo build
cargo test
cargo run
```

`cargo run` runs the orchestrator example in `rust/src/main.rs`.

## Repository layout

```
.
├── rust/
│   ├── src/
│   │   ├── llm.rs         # LLMProvider trait + clients
│   │   ├── cache.rs       # SQLite cache layer
│   │   ├── orchestrator.rs # Pipeline coordinator + hooks
│   │   ├── notify.rs      # Native lettre email notifier
│   │   ├── error.rs       # shared PipelineError
│   │   └── ...
│   ├── Cargo.toml
│   └── Cargo.lock
├── .env.example
└── CLAUDE.md
```

## Notes

The repo now intentionally contains only the Rust migration and its Rust-native operational docs.
