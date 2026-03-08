# CLAUDE.md

## Project Overview

X Bookmarks Pipeline — converts X (Twitter) financial bookmarks into executable TradingView Pine Script v6 strategies and indicators via a multi-LLM pipeline (xAI Grok + Claude Opus + ChatGPT).

## Tech Stack

- Python 3.9+
- `httpx` for HTTP (all LLM API calls — no SDKs)
- `rich` for CLI output formatting
- `sqlite3` for bookmark caching
- xAI Grok (`grok-4-0709`) for tweet classification
- Claude Opus (`claude-opus-4-6`) for vision analysis + strategy planning
- ChatGPT (`gpt-5.4`) for Pine Script code generation

## Pipeline Flow

```
Bookmark → [xAI] Classify text → finance?
  → No: [xAI] Classify images → finance?
    → No: Skip (cached as non-finance)
    → Yes: Continue
  → Yes: Continue
→ [Claude] Analyze chart images (vision) + Create strategy/indicator plan
→ [ChatGPT] Generate Pine Script v6 from plan
→ Validate → Cache → Save
```

## Project Structure

```
src/
├── clients/
│   ├── base_client.py              # Shared httpx wrapper
│   ├── xai_client.py               # xAI Grok (classification)
│   ├── anthropic_client.py         # Claude Opus (planning + vision)
│   └── openai_client.py            # ChatGPT (code generation)
├── classifiers/
│   └── finance_classifier.py       # Two-phase text→image classifier
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
│   └── bookmark_cache.py           # SQLite cache (thread-safe)
├── fetchers/
│   └── x_bookmark_fetcher.py       # X API v2 fetcher (auto token refresh)
├── prompts/
│   ├── grok_system_prompt.py       # Pine Script generation prompt
│   ├── classification_prompts.py   # Finance classification prompts
│   └── planning_prompts.py         # Strategy/indicator planning prompt
├── console.py                      # Shared Rich console + theme
└── pipeline.py                     # Multi-LLM orchestrator
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
| `XAI_API_KEY` | Always | xAI (classification) |
| `ANTHROPIC_API_KEY` | Always | Anthropic (planning + vision) |
| `OPENAI_API_KEY` | Always | OpenAI (code generation) |
| `X_USER_ACCESS_TOKEN` | `--fetch` mode | X API (bookmarks) |
| `X_USER_ID` | `--fetch` mode | X API (user ID) |
| `X_REFRESH_TOKEN` | Optional | Auto-refresh expired tokens |
| `X_CLIENT_ID` | Optional | Required for token refresh |
| `X_CLIENT_SECRET` | Optional | Required for token refresh |

## SQLite Cache

Located at `cache/bookmarks.db`. Caches each pipeline stage independently:

| Column | Content |
|---|---|
| `tweet_id` | Primary key |
| `classification_json` | xAI classification result |
| `plan_json` | Claude strategy/indicator plan |
| `pine_script` | Generated Pine Script code |
| `validation_passed` | Boolean |
| `validation_errors` | JSON array of error strings |

Cache is thread-safe (uses `threading.Lock`). Bookmarks are never re-processed unless `--clear-cache` or `--no-cache` is used.

## Code Conventions

- All modules use `from __future__ import annotations` for modern type hints.
- Imports use absolute paths from `src.*` (run from project root).
- Dataclasses for structured data (`ClassificationResult`, `StrategyPlan`, `PipelineResult`).
- No LLM SDKs — raw `httpx` for all API calls.
- `rich` for all CLI output — import from `src.console`.

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

## Output

- `.pine` files and `.meta.json` metadata go to `output/` (gitignored).
- `.meta.json` includes `tweet_url`, `chart_data` (structured JSON from vision), and `image_urls`.
- SQLite cache in `cache/` (gitignored).

## Tests

```bash
python3 -m pytest tests/ -v
```

68 tests covering clients, classifier, planner, cache, generator, pipeline, validator, and CLI.
