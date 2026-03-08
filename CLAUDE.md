# CLAUDE.md

## Project Overview

X Bookmarks Pipeline — categorizes ALL X (Twitter) bookmarks by topic and generates executable TradingView Pine Script v6 strategies/indicators for finance bookmarks via a multi-LLM pipeline (Cerebras + xAI Grok + Claude Opus + ChatGPT).

Every bookmark gets classified with a `category`/`subcategory` and saved as `.meta.json`. Finance bookmarks additionally get vision analysis, strategy planning, and Pine Script generation.

## Tech Stack

- Python 3.9+
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
│   └── x_bookmark_fetcher.py       # X API v2 fetcher (auto token refresh)
├── prompts/
│   ├── grok_system_prompt.py       # Pine Script generation prompt
│   ├── classification_prompts.py   # Category + finance classification prompts
│   └── planning_prompts.py         # Strategy/indicator planning prompt
├── console.py                      # Shared Rich console + theme
├── config.py                       # Centralized configuration defaults
└── pipeline.py                     # Multi-LLM orchestrator (classify → vision → plan → generate → save)
main.py                             # CLI entrypoint
auth_pkce.py                        # OAuth 2.0 PKCE token helper
```

## Key Commands

```bash
# Install dependencies
pip install -r requirements.txt

# Fetch live bookmarks and process
python main.py --fetch
python main.py --fetch --max-results 20

# From inline text
python main.py --text "BTC breakout above \$42k" --author "handle" --date "2026-03-01"

# From JSON bookmark file
python main.py --file example_bookmark.json

# Stdout-only (no file save)
python main.py --file example_bookmark.json --no-save

# Cache management
python main.py --cache-stats
python main.py --clear-cache
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

## Security

- Pre-commit hook blocks commits containing API keys, tokens, PII.
- `.env` is gitignored — secrets never enter version control.
- X API tokens auto-refresh on 401 and persist to `.env`.

## Tests

```bash
python3 -m pytest tests/ -v
```

134 tests covering clients (Cerebras, xAI, Anthropic, OpenAI), classifier, planner, cache, generator, pipeline, validator, vision analyzer, and CLI.
