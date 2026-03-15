# X Bookmarks Pipeline (Rust)

This repository contains a Rust-first migration of the X bookmark pipeline.

The implementation lives in `rust/` and includes:

- multi-provider orchestration (`Cerebras`, `xAI`, `Claude`, `OpenAI`)
- optional vision analysis
- Pine Script v6 generation and validation
- SQLite cache persistence
- native SMTP notifications through `lettre`
- structured error handling with `thiserror` + `anyhow`

## Current status

- Full Rust runtime is implemented in `rust/`.
- The pipeline boots from `.env` via `dotenvy`.
- Runtime providers are shared and instantiated once at startup.
- Local test suite currently passes (`33` tests as of the latest push).

## Setup

```bash
cp .env.example .env
cd rust
cargo build
```

## Required and optional environment variables

- Required for end-to-end execution: `CEREBRAS_API_KEY`, `XAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`
- Optional notification configuration: `SMTP_HOST`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_TO`
- Optional model/runner configuration: `CEREBRAS_MODEL`, `XAI_MODEL`, `ANTHROPIC_MODEL`, `OPENAI_MODEL`, `CACHE_PATH`, `MAX_WORKERS`, `API_TIMEOUT`, `VISION_TIMEOUT`, `FETCH_TIMEOUT`, `DEFAULT_TICKER`, `DEFAULT_TIMEFRAME`

## Build, test, and run

```bash
cd rust
cargo build
cargo test
cargo run -- --text "BTC 4h bullish momentum and breakout"
```

`cargo run` executes the orchestrator workflow.

## Repository layout

```text
.
├── rust/
│   ├── src/
│   │   ├── llm.rs          # shared LLMProvider abstraction and provider clients
│   │   ├── cache.rs        # SQLite cache with Arc<Mutex<Connection>>
│   │   ├── orchestrator.rs # bounded async pipeline orchestration + hooks
│   │   ├── notify.rs       # native lettre notifier (SMTP)
│   │   ├── error.rs        # central PipelineError model
│   │   └── ...
│   ├── Cargo.toml
│   └── Cargo.lock
├── .env.example
└── CLAUDE.md
```

## Notes

Python and Node runtime artifacts are intentionally out of the code path.
