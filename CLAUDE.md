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
- `x_api_cache.rs` caches username→user_id mappings, token validation state, and tracks X API request budgets.
- `main.rs` handles startup, env loading, provider bootstrap, and CLI dispatch.

## Setup

```bash
cp .env.example .env
cargo build
cargo test
cargo run -- --help
```

The binary loads `.env` from current directory at startup.

## Cache Management

```bash
# View cache statistics
cargo run -- --cache-stats

# Clear bookmark cache only (keeps output files)
cargo run -- --clear-cache

# Full reset: clear all caches AND delete output files
cargo run -- --reset
```

The `--reset` command performs a complete reset:
1. Clears bookmark cache (SQLite)
2. Deletes X API cache file (username mappings, request stats)
3. Recursively deletes all output files (.meta.json, .pine)

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
- `X_API_CACHE_PATH` — SQLite cache for X API cost optimization (default: `cache/x_api.db`)
- `MAX_WORKERS`
- `API_TIMEOUT`
- `VISION_TIMEOUT`
- `FETCH_TIMEOUT`
- `XPB_DAEMON` / `DAEMON_MODE`
- `DAEMON_INTERVAL_SECONDS` / `XPB_DAEMON_INTERVAL_SECONDS` — (default: 900s/15min)
- `DAEMON_FETCH_LIMIT` — bookmarks per daemon cycle (default: 25)
- `DAEMON_FETCH_PAGES` — max pages per daemon cycle (default: 1)
- `DAEMON_MAX_CYCLES` / `XPB_DAEMON_MAX_CYCLES`
- `TOKEN_VALIDATION_CACHE_SECONDS` — skip /users/me calls within this window (default: 300s)
- `XPB_CHROME_USER_DATA_DIR` — Chrome profile for CDP discovery (defaults to platform default)
- `XPB_CHROME_APP` — macOS app name for `open -a` (e.g. `Chrome Debug`)

## CDP auto-consent

The pipeline connects to Chrome's existing DevTools Protocol endpoint to auto-click "Authorize app" during OAuth reauth. Discovery order:
1. `DevToolsActivePort` in `XPB_CHROME_USER_DATA_DIR` (or default Chrome profile)
2. HTTP discovery at `http://127.0.0.1:9222/json/version`

After successful OAuth, the localhost callback tab is closed via `/json/close`.

Daemon filters bookmarks against cache before processing — only new bookmarks enter the pipeline. One summary email per cycle lists new bookmarks (URL, category, summary). No email when nothing is new.

**Smart error notifications:**
- X API `CreditsDepleted` (402) errors trigger only one notification, then the daemon continues retrying silently
- Notification resets when credits are replenished and a cycle succeeds
- Other errors notify after 10 consecutive failures

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

See `docs/X_API_BOOKMARKS_RESEARCH.md` for research on X API authentication requirements and why OAuth user consent cannot be eliminated.

## X API Cost Optimization

The pipeline includes several mechanisms to minimize X API costs:

**Pricing Reference** (Pay-as-you-go, 2024):
- `GET /2/users/:id/bookmarks`: $0.05 per 100 tweets
- `GET /2/users/me`: $0.01 per request
- `GET /2/users/by/username/:username`: $0.01 per request
- OAuth token refresh: Free

**Cost-Saving Features:**
1. **Username→user_id caching**: Resolved IDs are cached for 30 days in `x_api.db`
2. **Token validation caching**: Skip redundant `/users/me` calls within 5-minute windows
3. **Incremental fetching**: Early termination when hitting N consecutive cached bookmarks (default: 5)
4. **Reduced daemon defaults**: 25 bookmarks/cycle, 1 page/cycle, 15-min intervals (vs old 100/5/5min)
5. **Request budgeting**: Hard limits via `--max-requests-per-cycle`, `--max-requests-per-day`, `--max-cost-per-day`

**CLI Options:**
```bash
--fetch-limit 25            # Max bookmarks to fetch (default: 50)
--fetch-pages 1             # Max API pages (default: 2)
--early-stop-threshold 5    # Stop after N consecutive cached bookmarks
--max-requests-per-cycle 10 # Budget: max requests per daemon cycle
--max-requests-per-day 100  # Budget: max requests per day
--max-cost-per-day 1.00     # Budget: max estimated cost per day (USD)
```

**Monitoring:**
The pipeline prints X API stats to stderr:
```
[x-api] fetch stats: 1 requests, 25 fetched, 3 new, early_stop=true, cost=$0.0125
[x-api] cycle stats: today: 5 reqs ($0.0525), cycle: 1 reqs
```
