# CLAUDE.md

## Rust-only architecture

X Bookmarks Pipeline is now maintained as a Rust migration in `rust/`. Only Rust runtime code is tracked in this repository.

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

- No legacy non-Rust runtime/tests/trading modules are tracked.
- Keep this repo Rust-first; do not reintroduce legacy non-Rust entrypoints.

## Rust parity migration checklist (compact)

- [ ] CLI parity: add subcommands equivalent to legacy entrypoint options (fetch, text, file input, cache controls, output toggles, worker sizing).
- [ ] X fetcher parity: implement authenticated bookmark polling with refresh/token error handling and optional handle-based user resolution.
- [ ] Cache parity: verify SQLite schema and cache behaviors match legacy:
  - classification
  - vision/chart data
  - plan
  - generated script
  - validation result
  - completion flags and partial resume
- [ ] Classification parity:
  - route text to Cerebras first
  - vision fallback logic via xAI when non-finance + image content indicates chart signals
- [ ] Vision/parsing parity:
  - Claude image path for finance or visual-data bookmarks
  - chart JSON parsing/normalization and fallback handling
- [ ] Planning/generation parity:
  - strategy/indicator plan creation
  - Pine Script generation with strict validation
  - retain validation error reporting contract
- [ ] Persistence parity:
  - meta output for all bookmarks
  - `.pine` output for finance-only with same naming/placement conventions
- [ ] Notification parity:
  - replace Node path with `SmtpNotifier`
  - keep one-time token-failure alert behavior and per-cycle bookmark digest semantics
- [ ] Orchestrator parity:
  - bounded parallelism
  - robust `on_meta_saved` hook execution (non-fatal if hook fails)
- [ ] Daemon/runner parity:
  - periodic polling flow + process lifecycle/restart model
  - poll interval + service-level env wiring
- [ ] Test parity:
  - recreate key behavior coverage in Rust unit/integration tests (classification, planner, generator, cache hit/miss, hook safety, end-to-end orchestrator)
