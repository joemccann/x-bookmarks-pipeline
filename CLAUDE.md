# CLAUDE.md

## Rust-only architecture

This repository is a Rust implementation of the X bookmark pipeline with shared provider abstractions and a single executable workflow in `src/main.rs`.

- `llm.rs` exposes the shared `LLMProvider` trait (`classify`, `analyze_image`, `generate_code`) and provider wrappers.
- `cache.rs` owns SQLite persistence with shared mutable access using `Arc<Mutex<Connection>>`.
- `orchestrator.rs` coordinates bounded worker parallelism and `on_meta_saved` side effects.
- `notify.rs` implements `SmtpNotifier` via `lettre`; one email per cycle listing new bookmarks only.
- `error.rs` centralizes `PipelineError` and conversion of external failures.
- `browser.rs` implements CDP auto-consent (connects to existing Chrome via HTTP discovery on port 9222), tab close after OAuth callback.
- `cost.rs` tracks per-bookmark LLM token usage and USD costs with per-provider pricing.
- `main.rs` handles startup, env loading, provider bootstrap, and CLI dispatch.

## Setup

```bash
cp .env.example .env
cargo build
cargo test
cargo run -- --help
```

The binary loads `.env` from current directory at startup.

## Environment

Required API keys:
- `CEREBRAS_API_KEY`
- `XAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`

Optional SMTP notifications:
- `SMTP_HOST`
- `SMTP_USER`
- `SMTP_PASSWORD`
- `SMTP_FROM`
- `SMTP_TO`

Optional runtime tuning:
- `CEREBRAS_MODEL`
- `XAI_MODEL`
- `ANTHROPIC_MODEL`
- `OPENAI_MODEL`
- `CACHE_PATH`
- `MAX_WORKERS`
- `API_TIMEOUT`
- `VISION_TIMEOUT`
- `FETCH_TIMEOUT`
- `XPB_DAEMON` / `DAEMON_MODE`
- `DAEMON_INTERVAL_SECONDS` / `XPB_DAEMON_INTERVAL_SECONDS`
- `DAEMON_MAX_CYCLES` / `XPB_DAEMON_MAX_CYCLES`
- `XPB_CHROME_USER_DATA_DIR` — Chrome profile for CDP discovery (defaults to platform default)
- `XPB_CHROME_APP` — macOS app name for `open -a` (e.g. `Chrome Debug`)

## CDP auto-consent

The pipeline connects to Chrome's existing DevTools Protocol endpoint to auto-click "Authorize app" during OAuth reauth. Discovery order:
1. `DevToolsActivePort` in `XPB_CHROME_USER_DATA_DIR` (or default Chrome profile)
2. HTTP discovery at `http://127.0.0.1:9222/json/version`

After successful OAuth, the localhost callback tab is closed via `/json/close`.

Daemon filters bookmarks against cache before processing — only new bookmarks enter the pipeline. One summary email per cycle lists new bookmarks (URL, category, summary). No email when nothing is new.

## Notes

- No Python/Node runtime modules are tracked in this repo.
- SMTP is optional; missing SMTP values disable notifier setup cleanly.
- Keep changes focused to Rust-first execution and avoid reintroducing legacy non-Rust entrypoints.

## Migration checklist (compact)

- [x] CLI parity for text/file/cache/snapshot execution modes.
- [x] X fetcher input path and token-expiry handling.
- [x] Cache read/write paths for classification, plan, script, validation, chart data, completion.
- [x] Classification + vision branch coverage with cache short-circuits.
- [x] Planning and generation pipeline with Pine Script validation.
- [x] Native `lettre` notifier integration.
- [x] Bounded orchestrator concurrency.
- [x] Non-fatal `on_meta_saved` callback behavior.
- [x] Daemon/runner lifecycle parity (periodic poll + graceful stop).
- [x] Tests for unit + integration behavior.

See `tasks/todo.md` for current execution plan and open items.
