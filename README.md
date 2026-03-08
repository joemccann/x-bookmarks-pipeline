# X Bookmarks → Pine Script v6 Pipeline

A modular Python pipeline that converts X (Twitter) financial bookmarks into executable [TradingView Pine Script v6](https://www.tradingview.com/pine-script-docs/) strategies using [xAI Grok](https://x.ai/).

## How It Works

```
X Bookmark (text + chart image)
        │
        ▼
┌──────────────────┐
│  BookmarkParser   │  Extract ticker, direction, indicators,
│                   │  patterns, price levels
└────────┬─────────┘
         │ TradingSignal
         ▼
┌──────────────────┐
│ PineScriptGenerator│  Send structured prompt to Grok-4.1
│  (xAI Grok API)   │  with Pine Script v6 system prompt
└────────┬─────────┘
         │ raw Pine Script
         ▼
┌──────────────────┐
│ PineScriptValidator│  Static checks: version, strategy(),
│                   │  inputs, risk mgmt, no repainting
└────────┬─────────┘
         │
         ▼
   .pine file + .meta.json
```

## Quick Start

### 1. Install

```bash
pip install -r requirements.txt
```

### 2. Configure

```bash
cp .env.example .env
# Fill in XAI_API_KEY (always required)
# Fill in X_USER_ACCESS_TOKEN + X_USER_ID for --fetch mode
```

**Required env vars:**

| Variable | When needed | Where to get it |
|---|---|---|
| `XAI_API_KEY` | Always | [console.x.ai](https://console.x.ai/) |
| `X_USER_ACCESS_TOKEN` | `--fetch` | [console.x.com](https://console.x.com/) — OAuth 2.0 with `bookmark.read tweet.read users.read` |
| `X_USER_ID` | `--fetch` | `curl -H "Authorization: Bearer $X_USER_ACCESS_TOKEN" https://api.twitter.com/2/users/me` |

### 3. Run

**Fetch live bookmarks from X (processes all in batch, runs Grok vision on chart images):**

```bash
python3 main.py --fetch
python3 main.py --fetch --max-results 25
python3 main.py --fetch --x-username YourHandle   # resolves ID automatically
```

**From inline text:**

```bash
python3 main.py \
  --text "BTC breakout above \$42k, RSI oversold on 4h. Target \$45k, SL \$40k" \
  --author "CryptoTrader99" \
  --date "2026-03-01"
```

**With a chart image URL (analyzed automatically by Grok vision):**

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

**Live fetch:**

| Flag | Default | Description |
|------|---------|-------------|
| `--fetch` | — | Fetch bookmarks live from X API |
| `--x-username` | — | Resolve user ID from handle (overrides `X_USER_ID`) |
| `--max-results` | `10` | Max bookmarks to fetch |

**Manual input:**

| Flag | Short | Description |
|------|-------|-------------|
| `--text` | `-t` | Tweet text content |
| `--chart` | `-c` | Plain-text chart description |
| `--chart-url` | — | Chart image URL — analyzed by Grok vision automatically |
| `--author` | `-a` | Tweet author handle |
| `--date` | `-d` | Tweet date (YYYY-MM-DD) |
| `--file` | `-f` | Path to JSON bookmark file |

**Pipeline:**

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--model` | `-m` | `grok-4.1` | xAI model for Pine Script generation |
| `--output-dir` | `-o` | `output/` | Directory for output files |
| `--no-save` | — | — | Print to stdout only |
| `--no-vision` | — | — | Skip Grok vision even when image URLs present |

## Bookmark JSON Format

```json
{
  "text": "BTC looking ready for a breakout above $42,000. RSI oversold on the 4h chart...",
  "chart_description": "Optional plain-text description (skips vision)",
  "chart_url": "https://pbs.twimg.com/media/example.jpg",
  "author": "CryptoTrader99",
  "date": "2026-03-01"
}
```

## What the Parser Extracts

- **Tickers** — `$BTC`, `$ETH`, `$SOL`, `$SPX`, `$AAPL`, generic `$TICKER`, etc.
- **Direction** — long/short/both from keywords (buy, sell, bullish, bearish, calls, puts)
- **Timeframe** — 1m through monthly
- **Indicators** — RSI, MACD, EMA, SMA, Bollinger, VWAP, Fibonacci, Ichimoku, ATR, SuperTrend, etc.
- **Chart patterns** — Head & shoulders, triangles, flags, wedges, cup & handle, channels
- **Price levels** — Entry, stop-loss, take-profit, support, resistance (with `$42k` shorthand support)

## Pine Script v6 Rules Enforced

Every generated strategy is validated against these rules:

1. `//@version=6` version pragma
2. `strategy()` declaration with overlay and equity-based sizing
3. `input.*()` for all tunable parameters
4. `var`/`varip` for bar-persistent state
5. `strategy.exit()` with stop-loss and take-profit
6. `plotshape()`/`plotchar()` for visual entry/exit signals
7. Citation header crediting the original tweet author and date
8. No repainting — `barstate.isconfirmed` for entries, explicit `lookahead` on `request.security()`

## Project Structure

```
src/
├── fetchers/
│   └── x_bookmark_fetcher.py      # X API v2 bookmark fetcher (live data)
├── generators/
│   ├── pinescript_generator.py    # TradingSignal → Pine Script (via Grok)
│   └── vision_analyzer.py         # Chart image URL → description (Grok vision)
├── parsers/
│   └── bookmark_parser.py         # Tweet → TradingSignal
├── prompts/
│   └── grok_system_prompt.py      # System prompt for Pine Script generation
├── validators/
│   └── pinescript_validator.py    # Static v6 validation
└── pipeline.py                    # End-to-end orchestrator
main.py                            # CLI entrypoint
.env.example                       # Environment variable template
```

## Programmatic Usage

```python
from src.pipeline import BookmarkToPineScriptPipeline

pipeline = BookmarkToPineScriptPipeline(api_key="xai-...")
result = pipeline.run(
    tweet_text="BTC breakout above $42k, RSI oversold. Target $45k, SL $40k",
    chart_description="Ascending triangle on 4h BTCUSDT",
    author="CryptoTrader99",
    tweet_date="2026-03-01",
)

print(result.pine_script)          # The generated Pine Script v6 code
print(result.validation.valid)     # True if all checks pass
print(result.validation.errors)    # List of hard errors
print(result.validation.warnings)  # List of soft warnings
print(result.output_path)          # Path to saved .pine file
```

## License

MIT
