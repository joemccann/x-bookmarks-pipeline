# X Bookmarks Pipeline

A multi-LLM Python pipeline that fetches your X (Twitter) bookmarks, classifies every one by category, extracts structured data from chart images, and — for finance bookmarks — generates executable [TradingView Pine Script v6](https://www.tradingview.com/pine-script-docs/) strategies and indicators. All automatic, parallel, with SQLite caching so nothing is processed twice.

**Every bookmark is categorized and saved.** Finance bookmarks additionally get full Pine Script generation.

Four LLMs, each doing what it's best at:
- **Cerebras** — fast text classification (~46x faster than xAI, category/subcategory + finance detection)
- **xAI Grok** — image/vision classification (fallback when text is non-finance but images contain charts)
- **Claude Opus** — chart vision analysis (structured JSON extraction) + strategy/indicator planning
- **ChatGPT** — Pine Script v6 code generation (with self-validation checklist)

## How It Works

![Pipeline Diagram](.github/pipeline-diagram.png)

## Quick Start

### 1. Install

```bash
pip install -r requirements.txt
```

### 2. Configure

```bash
cp .env.example .env
# Fill in all API keys
```

**Required env vars:**

| Variable | Provider | Purpose |
|---|---|---|
| `CEREBRAS_API_KEY` | [cerebras.ai](https://cloud.cerebras.ai/) | Fast text classification |
| `XAI_API_KEY` | [console.x.ai](https://console.x.ai/) | Image/vision classification |
| `ANTHROPIC_API_KEY` | [console.anthropic.com](https://console.anthropic.com/) | Vision analysis + strategy planning |
| `OPENAI_API_KEY` | [platform.openai.com](https://platform.openai.com/) | Pine Script code generation |

**For `--fetch` mode (live X bookmarks):**

| Variable | Purpose |
|---|---|
| `X_USER_ACCESS_TOKEN` | OAuth 2.0 token with `bookmark.read tweet.read users.read` |
| `X_USER_ID` | Your numeric X user ID |
| `X_REFRESH_TOKEN` | Enables auto-refresh on 401 (recommended) |
| `X_CLIENT_ID` | Required for token refresh |
| `X_CLIENT_SECRET` | Required for token refresh |

Generate tokens with: `python auth_pkce.py`

### 3. Run

**Fetch and process live bookmarks (parallel, with caching):**

```bash
python3 main.py --fetch
python3 main.py --fetch --max-results 25
python3 main.py --fetch --x-username YourHandle
```

**From inline text:**

```bash
python3 main.py \
  --text "BTC breakout above \$42k, RSI oversold on 4h. Target \$45k, SL \$40k" \
  --author "CryptoTrader99" \
  --date "2026-03-01"
```

**With a chart image URL:**

```bash
python3 main.py \
  --text "Long ETH here" \
  --chart-url "https://pbs.twimg.com/media/example.jpg" \
  --author "DeFiWhale" \
  --date "2026-03-05"
```

**From a JSON bookmark file:**

```bash
python3 main.py --file bookmark.json
```

### CLI Options

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--fetch` | — | — | Fetch bookmarks live from X API |
| `--x-username` | — | — | Resolve user ID from handle |
| `--max-results` | — | `10` | Max bookmarks to fetch |
| `--text` | `-t` | — | Tweet text content |
| `--chart` | `-c` | — | Plain-text chart description |
| `--chart-url` | — | — | Chart image URL for Claude vision |
| `--author` | `-a` | — | Tweet author handle |
| `--date` | `-d` | — | Tweet date (YYYY-MM-DD) |
| `--file` | `-f` | — | Path to JSON bookmark file |
| `--output-dir` | `-o` | `output/` | Output directory |
| `--no-save` | — | — | Print to stdout only |
| `--no-vision` | — | — | Skip chart image analysis |
| `--no-cache` | — | — | Disable SQLite cache |
| `--clear-cache` | — | — | Clear all cached results and exit |
| `--cache-stats` | — | — | Show cache statistics and exit |
| `--workers` | `-w` | `5` | Max parallel workers |
| `--daemon` | — | — | Run as polling daemon (inline, for testing) |
| `--interval` | — | `900` | Daemon poll interval in seconds |

## Daemon Mode

A launchd daemon periodically fetches and processes new bookmarks:

```bash
# Run inline (for testing)
python3 main.py --daemon
python3 main.py --daemon --interval 60

# Install as launchd service
./service_ctl.sh install     # Copy plist + load
./service_ctl.sh status      # Show PID, log info
./service_ctl.sh logs        # tail -f the log file
./service_ctl.sh stop        # Stop daemon
./service_ctl.sh uninstall   # Unload + remove plist
```

Logs go to `~/.local/log/x-bookmarks-pipeline.log`. The daemon skips already-completed bookmarks via the cache.

## Output Structure

Output is organized by category and subcategory:

```
output/
├── finance/
│   ├── crypto/
│   │   ├── trader_BTCUSDT_2026-03-07.pine
│   │   └── trader_BTCUSDT_2026-03-07.meta.json
│   └── equities/
│       └── analyst_SPX_2026-03-05.pine
├── technology/
│   └── ai/
│       └── researcher_2026-03-03_abc12345.meta.json
├── science/
│   └── climate/
│       └── scientist_2026-03-02_def67890.meta.json
└── other/
    └── general/
        └── user_2026-03-01_ghi11223.meta.json
```

### Pine Script (`.pine`)

Generated Pine Script v6 code, ready to paste into TradingView. Finance bookmarks only.

### Metadata (`.meta.json`)

**All bookmarks** get a `.meta.json` with classification info:

```json
{
  "tweet_id": "2030348041346302177",
  "tweet_url": "https://x.com/user/status/2030348041346302177",
  "category": "finance",
  "subcategory": "crypto",
  "is_finance": true,
  "confidence": 0.95,
  "has_visual_data": true,
  "detected_topic": "crypto",
  "summary": "BTC breakout with RSI confirmation",
  "author": "CryptoTrader",
  "date": "2026-03-07",
  "image_urls": ["https://pbs.twimg.com/media/..."],
  "chart_data": {
    "image_type": "chart",
    "asset": {"ticker": "BTC", "name": "Bitcoin"},
    "price_levels": {"current": 42000, "support": [40000], "resistance": [45000]},
    "indicators": [{"name": "RSI", "value": "28", "signal": "bullish"}]
  }
}
```

Finance `.meta.json` additionally includes: `script_type`, `ticker`, `direction`, `timeframe`, `indicators`, `pattern`, `key_levels`, `rationale`, `validation_passed`, `validation_errors`, `validation_warnings`.

## SQLite Cache

The pipeline caches results in `cache/bookmarks.db` so bookmarks are never re-processed. Each stage is cached independently:

| Stage | Column | Cached After |
|-------|--------|-------------|
| Classification | `classification_json` | Cerebras/xAI determines category + finance detection |
| Vision | `chart_data_json` | Claude analyzes chart images |
| Plan | `plan_json` | Claude creates strategy/indicator plan |
| Script | `pine_script` | ChatGPT generates Pine Script |
| Validation | `validation_passed`, `validation_errors` | Validator checks the script |
| Completion | `completed` | All stages finished for this bookmark |

**Cache behavior:**
- Completed bookmarks skip all API calls
- Partially cached bookmarks resume from the last completed stage
- All bookmarks are cached (not just finance) — category info persists
- Cache is thread-safe for parallel processing
- Schema auto-migrates when new columns are added

**Management:**

```bash
python3 main.py --cache-stats    # Show counts
python3 main.py --clear-cache    # Delete all cached results
python3 main.py --no-cache       # Disable cache for this run
```

## Parallel Processing

In `--fetch` mode, bookmarks are processed in parallel (up to 5 workers). Each bookmark runs its own classification + vision analysis + planning + generation pipeline concurrently. Completed bookmarks are detected before any API calls and returned from cache immediately.

## Project Structure

```
src/
├── clients/                        # LLM API wrappers (httpx, no SDKs)
│   ├── base_client.py
│   ├── cerebras_client.py          # Cerebras (fast text classification)
│   ├── xai_client.py
│   ├── anthropic_client.py
│   └── openai_client.py
├── classifiers/
│   └── finance_classifier.py       # BookmarkClassifier: Cerebras text + xAI vision
├── planners/
│   └── strategy_planner.py         # Strategy/indicator planning (Claude)
├── generators/
│   ├── pinescript_generator.py     # Pine Script generation (ChatGPT)
│   └── vision_analyzer.py          # Chart image analysis (Claude vision)
├── parsers/
│   └── bookmark_parser.py          # Regex-based tweet parser
├── validators/
│   └── pinescript_validator.py     # Static v6 validation
├── cache/
│   └── bookmark_cache.py           # Thread-safe SQLite cache
├── fetchers/
│   └── x_bookmark_fetcher.py       # X API v2 (auto token refresh, note_tweet + article)
├── prompts/
│   ├── grok_system_prompt.py       # Pine Script system prompt
│   ├── classification_prompts.py   # Category + finance classification prompts
│   └── planning_prompts.py         # Strategy planning prompt
├── console.py                      # Rich console + theme
├── config.py                       # Centralized configuration defaults
└── pipeline.py                     # Multi-LLM orchestrator
main.py                             # CLI entrypoint
service.py                          # launchd polling daemon
service_ctl.sh                      # Daemon management (install/start/stop/logs)
auth_pkce.py                        # OAuth 2.0 PKCE token helper
tests/                              # 141 unit tests
```

## Security

A pre-commit hook scans all staged files for leaked secrets (API keys, tokens, PII) and blocks the commit if found. Patterns checked: Anthropic, OpenAI, xAI, AWS keys, private keys, SSNs, emails.

## Tests

```bash
python3 -m pytest tests/ -v
```

141 unit tests covering all modules: clients (Cerebras, xAI, Anthropic, OpenAI), classifier, planner, cache, generator, pipeline, validator, vision analyzer, fetcher, and CLI.

### Evaluation Scripts

```bash
# Compare Cerebras vs xAI Grok classification accuracy/speed — generates reports/cerebras_eval.html
python3 tests/cerebras_eval.py

# Live X API test: fetch bookmarks and verify note_tweet + entities for article posts
python3 tests/test_article_live.py
```

## License

MIT
