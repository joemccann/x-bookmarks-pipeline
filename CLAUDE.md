# CLAUDE.md

## Rust-only architecture

X Bookmarks Pipeline is now maintained as a Rust migration in `rust/`. There is no Python runtime code in this repository.

- `llm.rs` exposes the shared `LLMProvider` trait (`classify`, `analyze_image`, `generate_code`).
- `cache.rs` owns SQLite persistence with shared mutable state via `Arc<Mutex<Connection>>`.
- `orchestrator.rs` coordinates parallel worker execution and `on_meta_saved` side effects.
- `notify.rs` contains a native `lettre` notifier (`SmtpNotifier`) used by the orchestrator.
- `error.rs` defines `PipelineError` and keeps retryable/provider/SQLite/validation errors explicit.
- `main.rs` shows provider wiring and end-to-end startup for Rust execution.

## Setup

```bash
cd rust
cp ../.env.example .env # if not already at root
cp .env.example .env
cargo build
cargo test
cargo run
```

## Environment

Set the following in `.env`:

- Required API keys: `CEREBRAS_API_KEY`, `XAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`
- Required SMTP: `SMTP_HOST`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_TO`
- Optional tuning/model overrides are read in `rust/src/config.rs`

## Notes for future work

- No Python files, Python tests, Python service scripts, or Python trading modules are tracked anymore.
- Keep this repo Rust-first; do not reintroduce legacy `bin/` Python entrypoints.
