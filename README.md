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

### 2. Set your xAI API key

```bash
cp .env.example .env
# Edit .env and add your key from https://console.x.ai/
export XAI_API_KEY=xai-your-key-here
```

### 3. Run

**From inline text:**

```bash
python main.py \
  --text "BTC breakout above \$42k, RSI oversold on 4h. Target \$45k, SL \$40k" \
  --author "CryptoTrader99" \
  --date "2026-03-01"
```

**From a JSON bookmark file:**

```bash
python main.py --file example_bookmark.json
```

**With Grok vision chart description:**

```bash
python main.py \
  --text "Long ETH here" \
  --chart "4h chart showing ascending triangle with support at 3200 and resistance at 3500" \
  --author "DeFiWhale" \
  --date "2026-03-05"
```

### CLI Options

| Flag | Short | Description |
|------|-------|-------------|
| `--text` | `-t` | Tweet text content |
| `--chart` | `-c` | Chart image description (from Grok vision) |
| `--author` | `-a` | Tweet author handle |
| `--date` | `-d` | Tweet date (YYYY-MM-DD) |
| `--file` | `-f` | Path to JSON bookmark file |
| `--model` | `-m` | xAI model (default: `grok-4.1`) |
| `--output-dir` | `-o` | Output directory (default: `output/`) |
| `--no-save` | | Print to stdout only |

## Bookmark JSON Format

```json
{
  "text": "BTC looking ready for a breakout above $42,000. RSI oversold on the 4h chart...",
  "chart_description": "4-hour BTCUSDT chart showing ascending triangle pattern...",
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
├── prompts/
│   └── grok_system_prompt.py      # System prompt for Grok
├── parsers/
│   └── bookmark_parser.py         # Tweet → TradingSignal
├── generators/
│   └── pinescript_generator.py    # TradingSignal → Pine Script (via Grok)
├── validators/
│   └── pinescript_validator.py    # Static v6 validation
└── pipeline.py                    # End-to-end orchestrator
main.py                            # CLI entrypoint
example_bookmark.json              # Sample input
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
