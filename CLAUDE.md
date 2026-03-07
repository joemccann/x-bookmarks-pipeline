# CLAUDE.md

## Project Overview

X Bookmarks Pipeline — converts X (Twitter) financial bookmarks into executable TradingView Pine Script v6 strategies via xAI Grok.

## Tech Stack

- Python 3.10+
- `httpx` for HTTP (xAI API calls)
- xAI Grok-4.1 (multimodal) for Pine Script generation

## Project Structure

```
src/
├── prompts/grok_system_prompt.py   # System prompt sent to Grok
├── parsers/bookmark_parser.py      # Tweet text + chart → TradingSignal
├── generators/pinescript_generator.py  # Grok API bridge → Pine Script
├── validators/pinescript_validator.py  # Static v6 validation
└── pipeline.py                     # End-to-end orchestrator
main.py                             # CLI entrypoint
```

## Key Commands

```bash
# Install dependencies
pip install -r requirements.txt

# Run from inline text
python main.py --text "BTC breakout above \$42k" --author "handle" --date "2026-03-01"

# Run from JSON bookmark file
python main.py --file example_bookmark.json

# Stdout-only (no file save)
python main.py --file example_bookmark.json --no-save
```

## Environment Variables

- `XAI_API_KEY` — required. xAI API key for Grok access. See `.env.example`.

## Code Conventions

- All modules use `from __future__ import annotations` for modern type hints.
- Imports use absolute paths from `src.*` (run from project root).
- Dataclasses for structured data (`TradingSignal`, `ValidationResult`, `PipelineResult`).
- No external deps beyond `httpx`. Keep it minimal.

## Pine Script Rules

Generated scripts must follow these rules (enforced by the Grok system prompt and the validator):

1. `//@version=6` — strictly v6.
2. `strategy()` declaration with overlay and position sizing.
3. All tunable params via `input.*()`.
4. `var`/`varip` for persistent state.
5. `strategy.exit()` with stop-loss and take-profit.
6. `plotshape()`/`plotchar()` for visual signals.
7. Citation header crediting the original tweet author.
8. No repainting — `barstate.isconfirmed` for entries, explicit `lookahead` on `request.security()`.

## Output

Generated `.pine` files and `.meta.json` metadata go to `output/` (gitignored).
