# CLAUDE.md

## Project Overview

X Bookmarks Pipeline — categorizes ALL X (Twitter) bookmarks by topic and generates executable TradingView Pine Script v6 strategies/indicators for finance bookmarks via a multi-LLM pipeline (Cerebras + xAI Grok + Claude Opus + ChatGPT).

Every bookmark gets classified with a `category`/`subcategory` and saved as `.meta.json`. Finance bookmarks additionally get vision analysis, strategy planning, and Pine Script generation.

## Tech Stack

- Python 3.9+
- Node.js (nodemailer) for email notifications (`bin/notify.mjs`)
- `httpx` for HTTP (all LLM API calls — no SDKs)
- `rich` for CLI output formatting
- `sqlite3` for bookmark caching
- Cerebras (`qwen-3-235b-a22b-instruct-2507`) for fast text classification (~46x faster than xAI)
- xAI Grok (`grok-4-0709`) for image/vision classification (fallback when text is non-finance)
- Claude Opus (`claude-opus-4-6`) for vision analysis + strategy planning
- ChatGPT (`gpt-5.4`) for Pine Script code generation

## Pipeline Flow

```
Bookmark → [Cerebras] Classify text (category, subcategory, is_finance, has_visual_data)
  → if non-finance + has images: [xAI Grok] Classify images (vision fallback)
  → ALL bookmarks: save .meta.json to output/{category}/{subcategory}/
  → if has images AND (is_finance OR has_visual_data): [Claude] vision → chart_data
  → if is_finance: [Claude] plan → [ChatGPT] generate .pine → validate
```

No bookmarks are discarded. Every bookmark gets a `.meta.json`. Finance bookmarks additionally get `.pine` files.

## Project Structure

```
src/
├── clients/
│   ├── base_client.py              # Shared httpx wrapper
│   ├── cerebras_client.py          # Cerebras (fast text classification)
│   ├── xai_client.py               # xAI Grok (image/vision classification)
│   ├── anthropic_client.py         # Claude Opus (planning + vision)
│   └── openai_client.py            # ChatGPT (code generation)
├── classifiers/
│   └── finance_classifier.py       # BookmarkClassifier: Cerebras text + xAI vision (category + finance)
├── planners/
│   └── strategy_planner.py         # Strategy vs indicator planning
├── generators/
│   ├── pinescript_generator.py     # StrategyPlan → Pine Script (ChatGPT)
│   └── vision_analyzer.py          # Chart image → structured JSON (Claude)
├── parsers/
│   └── bookmark_parser.py          # Tweet text + chart → TradingSignal
├── validators/
│   └── pinescript_validator.py     # Static v6 validation (strategy + indicator)
├── cache/
│   └── bookmark_cache.py           # SQLite cache (thread-safe, with chart_data + completed tracking)
├── fetchers/
│   └── x_bookmark_fetcher.py       # X API v2 fetcher (auto token refresh, note_tweet + article support)
├── prompts/
│   ├── grok_system_prompt.py       # Pine Script generation prompt
│   ├── classification_prompts.py   # Category + finance classification prompts
│   └── planning_prompts.py         # Strategy/indicator planning prompt
├── console.py                      # Shared Rich console + theme
├── config.py                       # Centralized configuration defaults
└── pipeline.py                     # Multi-LLM orchestrator (on_meta_saved hook for real-time indexing)
trading/                            # Trading engine (self-contained, extractable)
├── pyproject.toml                  # Standalone package definition
└── trading/
    ├── config.py                   # DB paths, default tickers (SIGNALS_DB_PATH, BOOKMARKS_DB_PATH)
    ├── db/
    │   ├── schema.py               # SQLite setup: finance_signals, market_data, signals (WAL mode)
    │   └── reader.py               # Read-only query helpers
    ├── fetchers/
    │   └── market_data.py          # yfinance → market_data table (fetch-to-DB pattern)
    ├── indicators/
    │   └── move_psp_spread.py      # MOVE/PSP spread + 90d z-score → signals table
    ├── strategies/
    │   └── vix_vvix_mean_reversion.py  # VIX>30+VVIX>125 buy SPY + backtesting.py backtest
    ├── indexer.py                  # Scan output/finance/ → finance_signals (+ upsert_one hook)
    └── runner.py                   # Orchestrate index → fetch → indicators → strategies
bin/trading_main.py                 # Trading engine CLI (index|fetch|run|list|signals)
bin/main.py                         # Pipeline CLI entrypoint
bin/service.py                      # launchd polling daemon (on_meta_saved hook + email notifications)
service_ctl.sh                      # Daemon management (install/start/stop/logs)
bin/auth_pkce.py                    # OAuth 2.0 PKCE token helper
bin/notify.mjs                      # Email notifier (Node.js/nodemailer) — two modes:
                                    #   --mode error   → token failure alert (sent once per run)
                                    #   --mode bookmarks → per-cycle digest (JSON via stdin)
package.json                        # Node.js deps (nodemailer)
```

## Key Commands

```bash
# Install dependencies
pip install -r requirements.txt
npm install                                                     # nodemailer (email notifications)
pip install backtesting yfinance quantstats pandas-ta-classic  # trading engine extras

# Fetch live bookmarks and process
python3 bin/main.py --fetch
python3 bin/main.py --fetch --max-results 20

# From inline text
python3 bin/main.py --text "BTC breakout above \$42k" --author "handle" --date "2026-03-01"

# From JSON bookmark file
python3 bin/main.py --file example_bookmark.json

# Cache management
python3 bin/main.py --cache-stats
python3 bin/main.py --clear-cache

# Daemon mode (periodic polling)
./service_ctl.sh install   # launchd service (polls every 15 min)
./service_ctl.sh logs      # tail log file
./service_ctl.sh status    # show PID + last exit

# Trading engine
python3 bin/trading_main.py index              # index output/finance/ → signals.db
python3 bin/trading_main.py fetch              # fetch market data (VIX, VVIX, MOVE, SPY, PSP)
python3 bin/trading_main.py run                # run all indicators + strategies
python3 bin/trading_main.py list --type strategy
python3 bin/trading_main.py signals --name vix_vvix_mean_reversion
```

## Environment Variables

| Variable | Required | Provider |
|---|---|---|
| `CEREBRAS_API_KEY` | Always | Cerebras (text classification) |
| `XAI_API_KEY` | Always | xAI (image classification) |
| `ANTHROPIC_API_KEY` | Always | Anthropic (planning + vision) |
| `OPENAI_API_KEY` | Always | OpenAI (code generation) |
| `X_USER_ACCESS_TOKEN` | `--fetch` mode | X API (bookmarks) |
| `X_USER_ID` | `--fetch` mode | X API (user ID) |
| `X_REFRESH_TOKEN` | Optional | Auto-refresh expired tokens |
| `X_CLIENT_ID` | Optional | Required for token refresh |
| `X_CLIENT_SECRET` | Optional | Required for token refresh |
| `EMAIL_FROM` | Email notifications | Sender address |
| `EMAIL_TO` | Email notifications | Recipient address |
| `SMTP_HOST` | Email notifications | SMTP server (e.g. `smtp.gmail.com`) |
| `SMTP_PORT` | Email notifications | `587` (TLS) or `465` (SSL) |
| `SMTP_USER` | Email notifications | SMTP username |
| `SMTP_PASS` | Email notifications | SMTP password / app password |
| `NODE_BIN` | Optional | Path to `node` binary (auto-detected if unset) |

### Optional Config Overrides

All defaults live in `src/config.py` and can be overridden via env vars:

| Variable | Default | What it controls |
|---|---|---|
| `CEREBRAS_MODEL` | `qwen-3-235b-a22b-instruct-2507` | Text classification model |
| `XAI_MODEL` | `grok-4-0709` | Image classification model |
| `ANTHROPIC_MODEL` | `claude-opus-4-6` | Vision + planning model |
| `OPENAI_MODEL` | `gpt-5.4` | Code generation model |
| `API_TIMEOUT` | `120` | LLM API timeout (seconds) |
| `VISION_TIMEOUT` | `60` | Image analysis timeout |
| `FETCH_TIMEOUT` | `30` | X API timeout |
| `MAX_WORKERS` | `5` | Parallel workers (`--workers` CLI flag) |
| `OUTPUT_DIR` | `output` | Output base dir |
| `CACHE_PATH` | `cache/bookmarks.db` | SQLite cache location |
| `DEFAULT_TICKER` | `BTCUSDT` | Fallback ticker |
| `DEFAULT_TIMEFRAME` | `D` | Fallback timeframe |

## SQLite Cache

Located at `cache/bookmarks.db`. Caches each pipeline stage independently:

| Column | Content |
|---|---|
| `tweet_id` | Primary key |
| `classification_json` | Cerebras/xAI classification result (category, subcategory, is_finance) |
| `plan_json` | Claude strategy/indicator plan |
| `pine_script` | Generated Pine Script code |
| `validation_passed` | Boolean |
| `validation_errors` | JSON array of error strings |
| `chart_data_json` | Claude vision structured analysis |
| `completed` | Boolean — all pipeline stages finished |

Cache is thread-safe (uses `threading.Lock`). Completed bookmarks are never re-processed unless `--clear-cache` or `--no-cache` is used. Schema auto-migrates when new columns are added.

## Output Structure

Output is organized by category:

```
output/
├── finance/
│   ├── crypto/
│   │   ├── author_BTCUSDT_2026-03-07.pine
│   │   └── author_BTCUSDT_2026-03-07.meta.json
│   └── equities/
│       └── ...
├── technology/
│   └── ai/
│       └── author_2026-03-03_abc12345.meta.json
└── other/
    └── general/
        └── ...
```

- `.pine` files — generated Pine Script v6 code (finance only)
- `.meta.json` — metadata for ALL bookmarks (category, chart_data, etc.)
- SQLite cache in `cache/` (gitignored)

## Code Conventions

- All modules use `from __future__ import annotations` for modern type hints.
- Imports use absolute paths from `src.*` (run from project root).
- Dataclasses for structured data (`ClassificationResult`, `StrategyPlan`, `PipelineResult`).
- No LLM SDKs — raw `httpx` for all API calls.
- `rich` for all CLI output — import from `src.console`.
- `BookmarkClassifier` is the primary class name (`FinanceClassifier` is a backward-compatible alias).

## Pine Script Rules

Generated scripts must follow these rules (enforced by the system prompt, self-validation checklist, and static validator):

1. `//@version=6` — strictly v6.
2. `strategy()` or `indicator()` declaration matching the plan's `script_type`.
3. All tunable params via `input.*()`.
4. `var`/`varip` for persistent state.
5. `strategy.exit()` with stop-loss and take-profit (strategies only — indicators must NOT use `strategy.*` calls).
6. `plotshape()`/`plotchar()`/`plot()` for visual signals.
7. Citation header crediting the original tweet author.
8. No repainting — `barstate.isconfirmed` for entries, explicit `lookahead` on `request.security()`.
9. ChatGPT runs a 10-point self-validation checklist before returning code.
10. Output must be a single ` ```pinescript ` fenced block — no prose before/after. Extraction is hardened to recover code from any fence format or raw responses.

## Email Notifications

`bin/service.py` calls `bin/notify.mjs` via subprocess after each poll cycle. The script is invoked with the Python process's environment (which includes `.env` vars loaded via `python-dotenv`).

- **Token error** (`--mode error`): sends a one-time alert when `X_REFRESH_TOKEN` is invalid. The `_error_notified` flag suppresses repeat alerts every 15 min until the token is fixed and a successful fetch occurs. Detects: `"Token refresh failed"` or `"token was invalid"` in the `FetchError` message.
- **Bookmark digest** (`--mode bookmarks`): called whenever `poll_once` returns `processed_items` (i.e. `new > 0`). Payload is JSON piped to stdin: `{"bookmarks": [...], "cycle": N}`. Each item carries `author`, `tweet_url`, `text_excerpt`, `is_finance`, `category`, `subcategory`, `plan_title`, `valid`.
- `_call_notifier()` is fire-and-forget with a 30 s timeout — failures are logged as warnings and never crash the daemon.

## Security

- Pre-commit hook blocks commits containing API keys, tokens, PII.
- `.env` is gitignored — secrets never enter version control.
- X API tokens auto-refresh on 401 and persist to `.env`.

## Tests

```bash
python3 -m pytest tests/ -v                    # 151 pipeline tests
cd trading && python3 -m pytest tests/ -v      # 56 trading engine tests
```

207 tests total. The two suites must be run separately (different sys.path roots).
