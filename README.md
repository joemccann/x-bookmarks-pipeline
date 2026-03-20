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
-   Per-bookmark LLM cost tracking (USD) with `output/cost_report.md`
-   Email notifications for new bookmarks only (link, category, summary)
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

-   polls for new bookmarks (skips already-processed ones)
-   classifies, plans, and generates Pine Scripts for finance bookmarks
-   sends one email per cycle listing the new bookmarks

------------------------------------------------------------------------

# Dry run

Disable external side effects.

``` bash
cargo run -- --no-save --no-cache --text "quick reasoning check"
```

------------------------------------------------------------------------

# Cache Management

``` bash
# View cache statistics
cargo run -- --cache-stats

# Clear bookmark cache only (keeps output files)
cargo run -- --clear-cache

# Full reset: clear all caches AND delete output files
cargo run -- --reset
```

The `--reset` command performs a complete reset:
- Clears bookmark cache (SQLite)
- Deletes X API cache file (username mappings, request stats)  
- Recursively deletes all output files (.meta.json, .pine)

------------------------------------------------------------------------

# OAuth Reauth

When the daemon's X API token expires, the pipeline starts an
interactive OAuth 2.0 PKCE flow and listens for the callback on
localhost. By default the user must click **Authorize app** in the
browser manually.

## Automatic consent via CDP

The pipeline connects to your **existing Chrome** via the Chrome
DevTools Protocol to auto-click "Authorize app". No separate browser
instance is launched.

**How it works:**

1.  The OAuth URL opens in Chrome (or a configured Chrome app like `Chrome Debug`)
2.  CDP discovers Chrome via `http://127.0.0.1:9222/json/version` or `DevToolsActivePort`
3.  Finds the OAuth consent tab and clicks **Authorize app** via `data-testid`
4.  After the callback is received, **only the exact OAuth callback tab is closed** — other localhost tabs (dev servers, etc.) are never touched

### Setup

Chrome must have remote debugging enabled on port 9222. The recommended
approach is a dedicated Chrome Debug app (an Automator wrapper that
launches Chrome with `--remote-debugging-port=9222`).

``` bash
# Add to .env
XPB_CHROME_APP=Chrome Debug    # macOS: open -a "Chrome Debug" for OAuth URLs
```

Optionally point to a specific Chrome profile for `DevToolsActivePort` discovery:

``` bash
XPB_CHROME_USER_DATA_DIR=/path/to/ChromeDebugProfile
```

If neither is set, the pipeline falls back to the default Chrome profile
directory and the default browser.

### Failure modes

All failures degrade cleanly to the existing manual flow:

| Scenario | Behavior |
|---|---|
| Chrome not running with remote debugging | Log + manual fallback |
| No `DevToolsActivePort` and port 9222 unreachable | Log + manual fallback |
| User not logged into X | Log "login required", keep polling |
| Consent button selector drift | Return manual fallback with diagnostics |
| Callback arrives before CDP finishes | CDP task aborted cleanly |

------------------------------------------------------------------------

# X App Registration

To fetch your bookmarks you need an **X Developer App** with OAuth 2.0
enabled. Follow these steps once before running the pipeline.

## 1. Create a developer account

1.  Go to [developer.x.com](https://developer.x.com) and sign in with
    your X account.
2.  Accept the Developer Agreement and fill in the required use-case
    information.
3.  Your account will be provisioned a **Free** tier automatically.
    Bookmark access requires the **Basic** tier ($100/month). Upgrade
    under *Products → X API → Subscribe*.

## 2. Create a project and app

1.  In the Developer Portal, click **+ Add Project**.
2.  Give the project a name (e.g. `x-bookmarks-pipeline`).
3.  Select the **Basic** access level when prompted.
4.  Inside the project, click **+ Add App** and give the app a name.
5.  Copy and save the **API Key** (Consumer Key) and **API Key Secret**
    shown at this step — they are only shown once.

## 3. Configure OAuth 2.0

1.  Open the app in the portal and go to **Settings → User
    authentication settings**.
2.  Click **Set up** (or **Edit**).
3.  Set **App permissions** to *Read* (bookmarks are read-only).
4.  Set **Type of App** to *Web App, Automated App or Bot*.
5.  Add a **Callback URI / Redirect URL**:

    ``` 
    http://localhost:8080/callback
    ```

    This must match `X_REDIRECT_URI` in your `.env` (default is
    `http://localhost:8080/callback`).
6.  Add a **Website URL** (any valid URL, e.g. `https://example.com`).
7.  Click **Save**.
8.  Under the **Keys and tokens** tab, click **Generate** next to
    *OAuth 2.0 Client ID and Client Secret*. Copy both values.

## 4. Populate your `.env`

```bash
cp .env.example .env
```

Open `.env` and fill in the credentials obtained above:

```bash
X_CLIENT_ID=<OAuth 2.0 Client ID>
X_CLIENT_SECRET=<OAuth 2.0 Client Secret>
X_OAUTH_SCOPE="tweet.read users.read bookmark.read offline.access"
X_REDIRECT_URI=http://localhost:8080/callback

# Your numeric X user ID (find it at https://tweeterid.com or similar)
X_FETCH_USER_ID=<your_numeric_user_id>
```

The Bearer Token shown under **Keys and tokens → Bearer Token** can be
used for public-data lookups but **is not sufficient for bookmarks**.
Leave `X_BEARER_TOKEN` empty until you complete the OAuth flow below.

## 5. Run the initial OAuth flow

Start the pipeline with `--reauth` to open a browser window, grant
consent, and save a long-lived refresh token to your `.env`:

```bash
cargo run -- --reauth
```

1.  A browser window opens to the X authorization page.
2.  Click **Authorize app** (or let CDP auto-consent handle it — see
    [OAuth Reauth](#oauth-reauth)).
3.  After the callback is received the pipeline writes `X_REFRESH_TOKEN`
    (and optionally `X_BEARER_TOKEN`) back into your `.env`.

Subsequent runs will silently refresh the token in the background. You
only need to repeat this step if the refresh token expires (~6 months of
inactivity) or if you change the requested scopes.

------------------------------------------------------------------------

# LLM Cost Tracking

Every LLM API call captures `prompt_tokens` and `completion_tokens`
from the provider response and computes a USD cost using per-model
pricing tables. Costs are:

-   Attached to each bookmark's `.meta.json` under a `"cost"` key
-   Aggregated into `output/cost_report.md` after each pipeline run

The cost report includes breakdowns by provider, pipeline stage, and
per-bookmark.

------------------------------------------------------------------------

# Email Notifications

When SMTP is configured, the daemon sends one email per cycle listing
only **new** bookmarks. Each row shows:

-   **Link** to the original tweet
-   **Category** (e.g. finance/equities, technology/ai)
-   **Summary** of the bookmark content

Errors are listed per-bookmark if any pipeline stage failed. No email
is sent when there are no new bookmarks.

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

    XPB_CHROME_USER_DATA_DIR   # Chrome profile dir for DevToolsActivePort discovery
    XPB_CHROME_APP             # macOS app name (e.g. "Chrome Debug")

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
    │   ├── browser.rs        # CDP auto-consent + tab management
    │   ├── cost.rs           # LLM token usage and USD cost tracking
    │   ├── notify.rs         # Rich HTML email notifications
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
| `llm.rs` | LLM provider abstraction with token usage capture |
| `cache.rs` | SQLite caching and migrations |
| `browser.rs` | CDP auto-consent, HTTP discovery, tab management |
| `cost.rs` | Per-provider pricing, cost tracking, report generation |
| `notify.rs` | Rich HTML email notifications (per-bookmark + cycle) |
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
