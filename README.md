# X Bookmarks Pipeline

The X Bookmarks Pipeline automates end-to-end processing of X (Twitter) bookmarks into trading artifacts. It:

- Loads bookmarks from the X API or local input
- Classifies whether a bookmark is finance-related
- Optionally analyzes chart images (vision)
- Generates a Pine Script v6 strategy/reasoning payload
- Validates generated scripts and writes artifacts for review/deployment
- Stores intermediate/final results in SQLite for idempotent reruns
- Sends email notifications for completed metadata writes and daemon cycle summaries

## Features

- Multi-provider LLM orchestration (`Cerebras`, `xAI`, `Claude`, `OpenAI`)
- Async parallel execution with bounded worker concurrency
- Optional vision fallback and cache reuse across stages
- Persistent caching by bookmark id (`classification`, `plan`, `script`, validation, and completion state)
- Daemon mode for periodic polling and incremental processing
- Structured errors and robust retry-safe cache lock handling

## Repository layout

```text
.
├── Cargo.toml             # Rust crate manifest
├── Cargo.lock
├── src/
│   ├── llm.rs             # LLM provider abstraction + implementations
│   ├── cache.rs           # SQLite cache and migration helpers
│   ├── orchestrator.rs    # Pipeline orchestration + hooks
│   ├── notify.rs          # Native SMTP notifications
│   ├── error.rs           # PipelineError/structured failures
│   └── ...
├── tests/
│   └── dotenv_bootstrap.rs # CLI integration sanity test
├── .env.example
└── CLAUDE.md
```

The runtime entrypoint is `src/main.rs` and execution is in this crate root.

## Setup

```bash
cp .env.example .env
cargo build
```

Optional: run tests before first use.

```bash
cargo test
```

## Required and optional environment variables

- Required for end-to-end execution: `CEREBRAS_API_KEY`, `XAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`
- Optional fetch auth configuration: `X_BEARER_TOKEN`, `X_ACCESS_TOKEN`, `X_USER_ACCESS_TOKEN`
- Optional automatic refresh configuration: `X_CLIENT_ID` / `XPB_X_CLIENT_ID`, `X_CLIENT_SECRET` / `XPB_X_CLIENT_SECRET`, `X_REFRESH_TOKEN` / `XPB_X_REFRESH_TOKEN`
- Optional notification configuration: `SMTP_HOST`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_TO`
- Optional model/runner configuration: `CEREBRAS_MODEL`, `XAI_MODEL`, `ANTHROPIC_MODEL`, `OPENAI_MODEL`, `CACHE_PATH`, `MAX_WORKERS`, `API_TIMEOUT`, `VISION_TIMEOUT`, `FETCH_TIMEOUT`, `DEFAULT_TICKER`, `DEFAULT_TIMEFRAME`
- Optional X fetch configuration: `X_FETCH_USER_ID`, `X_FETCH_USERNAME`, `XPB_X_FETCH_USER_ID`, `XPB_X_FETCH_USERNAME`
- Optional daemon scheduling configuration: `XPB_DAEMON`, `XPB_DAEMON_INTERVAL_SECONDS`, `XPB_DAEMON_MAX_CYCLES`, `XPB_FETCH_LOOP`

## Build, test, and run

```bash
cargo build
cargo test
# one-time execution from text input
cargo run -- --text "BTC 4h bullish momentum and breakout"

# periodic mode (polling)
cargo run -- --daemon --daemon-interval 300
```

`cargo run` executes the orchestrator workflow.

When daemon mode is enabled, per-bookmark notifications are sent from the SMTP notifier (if configured), and a cycle summary is also sent for each non-empty batch.

## Automatic token refresh

If X returns an authentication-expired error while fetching bookmarks, the pipeline will automatically request a new access token using your configured refresh credentials and retry the fetch once.

## Common usage patterns

```bash
# fetch latest bookmarks and process them (user ID)
cargo run -- --fetch --fetch-user-id <x_user_id>
# fetch latest bookmarks and process them (username)
cargo run -- --fetch --fetch-username joemccann

# process bookmarks from JSON/text file
cargo run -- --file path/to/bookmarks.json
cargo run -- --text "ETH breakout after key support hold"

# disable external side effects for dry-runs
cargo run -- --no-save --no-cache --text "quick reasoning check"
```

## Notes

Python and Node runtime artifacts are intentionally out of the code path.
