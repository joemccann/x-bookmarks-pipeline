"""
Classification prompts for xAI Grok — categorize tweets by topic and detect
finance/trading content with actionable patterns.
"""

FINANCE_TEXT_CLASSIFICATION_PROMPT = r"""
You are a content classifier. Analyze the given tweet text and determine:

1. What category and subcategory does this tweet belong to?
2. Is this tweet about finance, trading, or markets?
3. Does it contain an actionable trading pattern or setup?
4. Does it reference visual data (charts, tables, graphs, dashboards)?

Respond with ONLY valid JSON (no markdown, no explanation):

{
  "is_finance": true/false,
  "confidence": 0.0-1.0,
  "category": "finance|technology|science|politics|entertainment|sports|health|education|other",
  "subcategory": "crypto|equities|forex|commodities|macro|options|AI|machine_learning|web_dev|mobile|cybersecurity|physics|biology|climate|us_politics|world_politics|movies|music|gaming|football|basketball|mma|nutrition|mental_health|fitness|tutorials|research|general",
  "has_trading_pattern": true/false,
  "has_visual_data": false,
  "detected_topic": "specific topic detected",
  "summary": "One-sentence summary of the tweet content"
}

Classification guidelines:
- category: broad topic area. Use "finance" for trading/markets/investing content
- subcategory: specific niche within the category. Pick the closest match
- is_finance: true when about trading setups, market analysis, price targets,
  technical analysis, fundamental analysis, earnings plays, macro commentary, crypto analysis
- NOT finance: general tech news, memes without trading context, personal updates,
  politics (unless directly about market impact), jokes
- has_trading_pattern: true when there's a specific entry/exit, price level, indicator
  signal, or chart pattern mentioned
- has_visual_data: always false for text-only classification (images analyzed separately)
""".strip()


FINANCE_IMAGE_CLASSIFICATION_PROMPT = r"""
You are an image analyst. Analyze the given image(s) and determine:

1. What category does this content belong to?
2. Is this a financial chart or trading-related image?
3. Does it contain visual data (charts, tables, graphs, dashboards)?
4. Does it show an actionable trading pattern or setup?

Respond with ONLY valid JSON (no markdown, no explanation):

{
  "is_finance": true/false,
  "confidence": 0.0-1.0,
  "category": "finance|technology|science|politics|entertainment|sports|health|education|other",
  "subcategory": "crypto|equities|forex|commodities|macro|options|AI|machine_learning|web_dev|mobile|cybersecurity|physics|biology|climate|us_politics|world_politics|movies|music|gaming|football|basketball|mma|nutrition|mental_health|fitness|tutorials|research|general",
  "has_trading_pattern": true/false,
  "has_visual_data": true/false,
  "detected_topic": "specific topic detected",
  "summary": "Description of image content, patterns, key data points, and annotations",
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
- has_visual_data: true for ANY image with charts, tables, graphs, dashboards, data grids,
  statistics displays, or structured visual information
- is_finance: true specifically for financial/trading charts, market data
- NOT finance: random photos, memes without data, screenshots of non-data content
- Extract as much detail as possible from chart annotations and drawings
""".strip()
