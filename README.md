# X Bookmarks Pipeline

Convert **X (Twitter) bookmarks into trading strategies**.

This pipeline ingests bookmarks, detects finance-related content,
analyzes charts, and generates **validated Pine Script strategies**
ready for review or deployment.

It is built in **Rust**, runs asynchronously, and is safe to rerun
thanks to persistent caching.

------------------------------------------------------------------------

# Features

-   Multi-provider LLM orchestration (`Cerebras`, `xAI`, `Claude`,
    `OpenAI`)
-   Parallel processing with bounded worker concurrency
-   Optional vision analysis for chart screenshots
-   Persistent SQLite caching (idempotent reruns)
-   Daemon mode for continuous bookmark ingestion
-   Automatic OAuth reauth with CDP auto-consent (zero manual intervention)
-   Email notifications for completed runs
-   Structured error handling and retry-safe execution

------------------------------------------------------------------------

# Quickstart

Clone the repository and configure environment variables.

``` bash
cp .env.example .env
cargo build
```

Run tests:

``` bash
cargo test
```

Run the pipeline:

``` bash
cargo run -- --text "BTC 4h bullish momentum and breakout"
```

------------------------------------------------------------------------

# Usage

## Process X bookmarks

``` bash
cargo run -- --fetch --fetch-user-id <x_user_id>
```

## Process a local file

``` bash
cargo run -- --file bookmarks.json
```

## Process a single text input

``` bash
cargo run -- --text "ETH breakout after key support hold"
```

## Run continuously (daemon mode)

``` bash
cargo run -- --daemon --daemon-interval 300
```

When daemon mode is enabled the pipeline:

-   polls for new bookmarks
-   processes them incrementally
-   sends per-bookmark notifications
-   sends a summary email per cycle

------------------------------------------------------------------------

# Dry run

Disable external side effects.

``` bash
cargo run -- --no-save --no-cache --text "quick reasoning check"
```

------------------------------------------------------------------------

# OAuth Reauth

When the daemon's X API token expires, the pipeline starts an
interactive OAuth 2.0 PKCE flow and listens for the callback on
localhost. By default the user must click **Authorize app** in the
browser manually.

## Automatic consent via CDP (optional)

Set `XPB_CHROME_USER_DATA_DIR` to a dedicated Chrome profile directory.
When configured, the pipeline will:

1.  Launch Chrome with `--remote-debugging-port=0` using that profile
2.  Connect to Chrome DevTools Protocol via WebSocket
3.  Locate the OAuth consent page and click **Authorize app** automatically
4.  Fall back gracefully to manual mode if CDP is unavailable

### One-time setup

``` bash
# Create a dedicated Chrome profile for OAuth
mkdir -p ~/.chrome-oauth-profile

# Add to .env
echo 'XPB_CHROME_USER_DATA_DIR=/path/to/.chrome-oauth-profile' >> .env
```

The first time the reauth flow runs, Chrome will open with a fresh
profile. Log into X once in that browser window. After that, all future
OAuth reauths are fully automatic.

### Failure modes

All failures degrade cleanly to the existing manual flow:

| Scenario | Behavior |
|---|---|
| `XPB_CHROME_USER_DATA_DIR` not set | Manual flow only (no regression) |
| Chrome not launched with remote debugging | Log + manual fallback |
| `DevToolsActivePort` missing or unreadable | Log + manual fallback |
| User not logged into X | Log "login required", keep polling |
| Consent button selector drift | Return manual fallback with diagnostics |
| Callback arrives before CDP finishes | CDP task aborted cleanly |

------------------------------------------------------------------------

# Environment Variables

## Required

    CEREBRAS_API_KEY
    XAI_API_KEY
    ANTHROPIC_API_KEY
    OPENAI_API_KEY

## X API Authentication

    X_BEARER_TOKEN          # or X_ACCESS_TOKEN
    X_FETCH_USER_ID         # numeric user ID

## OAuth Refresh (optional)

    X_CLIENT_ID
    X_CLIENT_SECRET
    X_REFRESH_TOKEN
    X_OAUTH_SCOPE

## CDP Auto-Consent (optional)

    XPB_CHROME_USER_DATA_DIR   # dedicated Chrome profile for reauth
    XPB_CHROME_BINARY          # Chrome binary path override

## Email Notifications (optional)

    SMTP_HOST
    SMTP_USER
    SMTP_PASSWORD
    SMTP_FROM
    SMTP_TO

## Runtime Configuration (optional)

    CEREBRAS_MODEL
    XAI_MODEL
    ANTHROPIC_MODEL
    OPENAI_MODEL

    CACHE_PATH
    MAX_WORKERS

    API_TIMEOUT
    VISION_TIMEOUT
    FETCH_TIMEOUT

    DEFAULT_TICKER
    DEFAULT_TIMEFRAME

## Daemon Configuration (optional)

    XPB_DAEMON
    XPB_DAEMON_INTERVAL_SECONDS
    XPB_DAEMON_MAX_CYCLES
    XPB_FETCH_LOOP

------------------------------------------------------------------------

# Dependencies

Key Rust crates:

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime (multi-thread, networking, I/O) |
| `reqwest` | HTTP client for LLM and X API calls |
| `rusqlite` | SQLite caching (bundled) |
| `serde` / `serde_json` | JSON serialization |
| `clap` | CLI argument parsing |
| `lettre` | SMTP email notifications |
| `tokio-tungstenite` | WebSocket client for CDP auto-consent |
| `futures-util` | Stream/sink combinators for WebSocket |
| `anyhow` / `thiserror` | Error handling |

See `Cargo.toml` for the full dependency list with versions.

------------------------------------------------------------------------

# Repository Structure

    .
    ├── src/
    │   ├── main.rs           # Startup, CLI, OAuth flow
    │   ├── orchestrator.rs   # Pipeline workflow
    │   ├── llm.rs            # LLM provider abstraction
    │   ├── cache.rs          # SQLite caching
    │   ├── browser.rs        # CDP auto-consent client
    │   ├── notify.rs         # SMTP notifications
    │   ├── error.rs          # Structured pipeline errors
    │   ├── classifier.rs     # Text/vision classification
    │   ├── fetcher.rs        # X API bookmark fetcher
    │   ├── generator.rs      # Pine Script generation
    │   ├── planner.rs        # Strategy planning
    │   ├── validator.rs      # Pine Script validation
    │   ├── vision.rs         # Image analysis
    │   └── ...
    ├── Cargo.toml
    ├── Cargo.lock
    ├── .env.example
    └── CLAUDE.md

------------------------------------------------------------------------

# Pipeline

The pipeline executes the following stages:

    Bookmarks
      → Classification
      → Vision Analysis
      → Strategy Generation
      → Script Validation
      → Artifact Output

Each stage is cached by bookmark id, allowing safe reruns without
recomputation.

------------------------------------------------------------------------

# Architecture

The system is implemented as a **single Rust crate**.

Core modules:

| Module | Purpose |
|---|---|
| `orchestrator.rs` | Pipeline workflow and concurrency |
| `llm.rs` | LLM provider abstraction |
| `cache.rs` | SQLite caching and migrations |
| `browser.rs` | CDP WebSocket client for OAuth auto-consent |
| `notify.rs` | SMTP notifications |
| `error.rs` | Structured pipeline errors |

The pipeline runs asynchronously with bounded worker concurrency.

------------------------------------------------------------------------

# Design Principles

-   **Idempotent execution** --- safe to rerun any time
-   **Deterministic artifacts** --- cached intermediate results
-   **Parallel processing** --- high throughput
-   **Provider abstraction** --- easy model switching
-   **Graceful degradation** --- CDP auto-consent falls back to manual
-   **Minimal runtime dependencies** --- Rust-only execution

Python and Node runtimes are intentionally **not in the execution
path**.
