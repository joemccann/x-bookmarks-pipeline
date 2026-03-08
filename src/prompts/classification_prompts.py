"""
Classification prompts for xAI Grok — determine if a tweet is finance-related
and whether it contains actionable trading patterns.
"""

FINANCE_TEXT_CLASSIFICATION_PROMPT = r"""
You are a financial content classifier. Analyze the given tweet text and determine:

1. Is this tweet about finance, trading, or markets?
2. Does it contain an actionable trading pattern or setup?
3. What specific topic does it cover?

Respond with ONLY valid JSON (no markdown, no explanation):

{
  "is_finance": true/false,
  "confidence": 0.0-1.0,
  "has_trading_pattern": true/false,
  "detected_topic": "crypto|equities|forex|commodities|macro|options|none",
  "summary": "One-sentence summary of the trading idea or why it's not finance"
}

Classification guidelines:
- Finance includes: trading setups, market analysis, price targets, technical analysis,
  fundamental analysis, earnings plays, macro commentary, crypto analysis
- NOT finance: general tech news, memes without trading context, personal updates,
  politics (unless directly about market impact), jokes
- has_trading_pattern = true when there's a specific entry/exit, price level, indicator
  signal, or chart pattern mentioned
""".strip()


FINANCE_IMAGE_CLASSIFICATION_PROMPT = r"""
You are a financial chart analyst. Analyze the given image(s) and determine:

1. Is this a financial chart or trading-related image?
2. Does it show an actionable trading pattern or setup?
3. What details can you extract?

Respond with ONLY valid JSON (no markdown, no explanation):

{
  "is_finance": true/false,
  "confidence": 0.0-1.0,
  "has_trading_pattern": true/false,
  "detected_topic": "crypto|equities|forex|commodities|macro|options|none",
  "summary": "Description of the chart pattern, indicators visible, key levels, and any annotations",
  "chart_details": {
    "ticker": "detected ticker symbol or empty string",
    "timeframe": "detected timeframe or empty string",
    "indicators_visible": ["list of visible indicators"],
    "patterns_visible": ["list of chart patterns"],
    "key_levels": ["list of notable price levels"],
    "annotations": "any text annotations on the chart"
  }
}

Analysis guidelines:
- Look for: candlestick charts, line charts, indicator panels (RSI, MACD, volume),
  drawn trendlines, support/resistance levels, pattern annotations
- NOT finance: random photos, screenshots of non-chart content, memes
- Extract as much detail as possible from chart annotations and drawings
""".strip()
