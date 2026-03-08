# X Bookmarks → Pine Script v6 Pipeline

A multi-LLM Python pipeline that converts X (Twitter) financial bookmarks into executable [TradingView Pine Script v6](https://www.tradingview.com/pine-script-docs/) strategies and indicators.

Three LLMs, each doing what it's best at:
- **xAI Grok** — tweet classification (finance or not?)
- **Claude Opus** — chart vision analysis + strategy/indicator planning
- **ChatGPT** — Pine Script v6 code generation

## How It Works

```
X Bookmark (text + chart images)
        │
        ▼
┌──────────────────────┐
│  xAI Grok Classifier │  Is this tweet about finance?
│  (text → image fallback)│  Text first, then images if needed
└────────┬─────────────┘
         │ ClassificationResult
         ▼
┌──────────────────────┐
│  Claude Vision       │  Extract structured data from chart images
│  (Anthropic Opus)    │  Returns JSON: price levels, indicators, tables
└────────┬─────────────┘
         │ chart_data JSON
         ▼
┌──────────────────────┐
│  Claude Planner      │  Strategy or indicator? What parameters?
│  (Anthropic Opus)    │  Returns StrategyPlan with full spec
└────────┬─────────────┘
         │ StrategyPlan
         ▼
┌──────────────────────┐
│  ChatGPT Generator   │  Convert plan into Pine Script v6 code
│  (OpenAI GPT-5.4)    │  Follows strict system prompt rules
└────────┬─────────────┘
         │ Pine Script
         ▼
┌──────────────────────┐
│  Validator           │  Static checks: version, declaration,
│                      │  inputs, risk mgmt, repainting
└────────┬─────────────┘
         │
         ▼
   .pine file + .meta.json + SQLite cache
```

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
| `XAI_API_KEY` | [console.x.ai](https://console.x.ai/) | Tweet classification |
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

## SQLite Cache

The pipeline caches results in `cache/bookmarks.db` so bookmarks are never re-processed. Each stage is cached independently:

| Stage | Column | Cached After |
|-------|--------|-------------|
| Classification | `classification_json` | xAI determines finance/non-finance |
| Plan | `plan_json` | Claude creates strategy/indicator plan |
| Script | `pine_script` | ChatGPT generates Pine Script |
| Validation | `validation_passed`, `validation_errors` | Validator checks the script |

**Cache behavior:**
- Fully cached bookmarks skip all API calls (including vision analysis)
- Partially cached bookmarks resume from the last completed stage
- Non-finance tweets are cached as skipped — never re-classified
- Cache is thread-safe for parallel processing

**Management:**

```bash
python3 main.py --cache-stats    # Show counts
python3 main.py --clear-cache    # Delete all cached results
python3 main.py --no-cache       # Disable cache for this run
```

## Output Format

### Pine Script (`.pine`)

Generated Pine Script v6 code, ready to paste into TradingView.

### Metadata (`.meta.json`)

```json
{
  "tweet_id": "2030348041346302177",
  "tweet_url": "https://x.com/Bluekurtic/status/2030348041346302177",
  "script_type": "strategy",
  "author": "Bluekurtic",
  "date": "2026-03-07",
  "ticker": "SPX",
  "direction": "long",
  "timeframe": "D",
  "indicators": ["VIX", "VVIX"],
  "pattern": null,
  "key_levels": {"entry": 5770, "stop_loss": 5700},
  "rationale": "VIX spike historically precedes mean reversion...",
  "image_urls": ["https://pbs.twimg.com/media/..."],
  "chart_data": {
    "image_type": "chart",
    "asset": {"ticker": "VIX", "name": "Volatility Index"},
    "price_levels": {"current": 23.37, "support": [20], "resistance": [30]},
    "indicators": [{"name": "VVIX", "value": "140+", "signal": "bearish"}],
    "tabular_data": {"headers": [...], "rows": [...]}
  },
  "validation_passed": true,
  "validation_errors": [],
  "validation_warnings": []
}
```

## Parallel Processing

In `--fetch` mode, bookmarks are processed in parallel (up to 5 workers). Each bookmark runs its own vision analysis + classification + planning + generation pipeline concurrently. Cached bookmarks are detected before any API calls and skipped immediately.

## Project Structure

```
src/
├── clients/                        # LLM API wrappers (httpx, no SDKs)
│   ├── base_client.py
│   ├── xai_client.py
│   ├── anthropic_client.py
│   └── openai_client.py
├── classifiers/
│   └── finance_classifier.py       # Two-phase text→image classifier (xAI)
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
│   └── x_bookmark_fetcher.py       # X API v2 (auto token refresh)
├── prompts/
│   ├── grok_system_prompt.py       # Pine Script system prompt
│   ├── classification_prompts.py   # Finance classification prompts
│   └── planning_prompts.py         # Strategy planning prompt
├── console.py                      # Rich console + theme
└── pipeline.py                     # Multi-LLM orchestrator
main.py                             # CLI entrypoint
auth_pkce.py                        # OAuth 2.0 PKCE token helper
tests/                              # 68 unit tests
```

## Tests

```bash
python3 -m pytest tests/ -v
```

## License

MIT
