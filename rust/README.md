# Rust Migration: X Bookmarks Pipeline

This crate is the Rust implementation of the X bookmark ingestion, classification, and Pine Script generation pipeline.

## Architecture

- `llm.rs`
  - `LLMProvider` trait (`classify`, `analyze_image`, `generate_code`)
  - shared request/response flow for:
    - `CerebrasProvider`
    - `XaiProvider`
    - `ClaudeProvider`
    - `OpenAIProvider`

- `cache.rs`
  - thread-safe SQLite cache with `Arc<Mutex<Connection>>`
  - stores classification, plan, script, validation metadata, chart data, and completion flags

- `planner.rs`
  - JSON plan parsing with defaults and fallback behavior

- `generator.rs`
  - Pine Script generation and version normalization

- `validator.rs`
  - Pine Script rule checks (`//@version=6`, strategy exit checks, declaration requirements)

- `orchestrator.rs`
  - bounded `tokio` concurrency
  - `run_batch` fan-out + `on_meta_saved` callback
  - non-fatal callback failure handling

- `notify.rs`
  - `SmtpNotifier` replacement for Node-based notification logic

- `error.rs`
  - central `PipelineError` variants for API, cache, timeout, and validation failures

- `main.rs`
  - startup/bootstrap and dependency wiring

## Core error model

Current `PipelineError` variants are:

- `ApiTimeout`
- `SqliteLock`
- `Cache`
- `CachePoisoned`
- `PineValidation`
- `Http`
- `ProviderResponse`
- `TokenExpired`
- `Email`
- `TaskJoin`
- `Io`

## Runtime defaults

- Models and endpoints are configured in provider constructors and can be overridden with env:
  - `CEREBRAS_MODEL`
  - `XAI_MODEL`
  - `ANTHROPIC_MODEL`
  - `OPENAI_MODEL`

- Cache, timing, and run controls are sourced from:
  - `CACHE_PATH`
  - `API_TIMEOUT`
  - `VISION_TIMEOUT`
  - `FETCH_TIMEOUT`
  - `MAX_WORKERS`
  - `OUTPUT_DIR`

## Running

```bash
cd rust
cargo build
cargo test
cargo run -- --text "BTC 4h bullish divergence"
cargo run -- --daemon --daemon-interval 300
```

For dry-run bootstrap checks, use:

```bash
cargo run -- --clear-cache --cache-path /tmp/xbp-cache.db
```

`cargo run -- --help` shows all available CLI flags.

## Daemon mode

```bash
cd rust
cargo run -- --daemon --daemon-interval 300 --max-cycles 1
```

Daemon mode keeps the process alive and processes new bookmarks on each interval. To stop after one cycle, use `--max-cycles 1` (useful for health checks).

Notification behavior:
- `--daemon` + SMTP settings: sends per-bookmark meta emails and a cycle summary for non-empty cycles.
- No SMTP settings: notifier stays disabled and the daemon still runs with local output.

## Think Step-by-Step: ownership flow

1. `main` loads environment, builds providers, resolves config, and parses bookmarks.
2. `Pipeline::run_batch` clones input into bounded worker tasks.
3. Each worker reads a `Bookmark`, calls classifier, then optional vision analysis.
4. A `CodeGenInput` is assembled from borrowed/owned plan and bookmark data.
5. `generate_code` returns `CodeGenOutput`; validation converts it into persisted script state.
6. Cache writes are performed, and `on_meta_saved` is invoked with the persisted metadata path.
7. Worker outputs are collected into final `PipelineResult` entries.
