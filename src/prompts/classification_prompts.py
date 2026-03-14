"""
Classification prompts for xAI Grok — categorize tweets by topic and detect
finance/trading content with actionable patterns.
"""

_CATEGORIES = "finance|technology|science|politics|entertainment|sports|health|education|other"
_SUBCATEGORIES = "crypto|equities|forex|commodities|macro|options|AI|machine_learning|web_dev|mobile|cybersecurity|physics|biology|climate|us_politics|world_politics|movies|music|gaming|football|basketball|mma|nutrition|mental_health|fitness|tutorials|research|general"

FINANCE_TEXT_CLASSIFICATION_PROMPT = (
    f"Classify tweet. JSON: is_finance(bool),confidence(0-1),category({_CATEGORIES}),"
    f"subcategory({_SUBCATEGORIES}),has_trading_pattern(bool),has_visual_data(false),detected_topic,summary.\n"
    "is_finance=true for trading/TA/FA/crypto. has_trading_pattern=true for entry/exit/levels."
)

FINANCE_IMAGE_CLASSIFICATION_PROMPT = (
    f"Classify image(s). JSON: is_finance(bool),confidence(0-1),category({_CATEGORIES}),"
    f"subcategory({_SUBCATEGORIES}),has_trading_pattern(bool),has_visual_data(bool),detected_topic,summary.\n"
    "has_visual_data=true for charts/tables/graphs. is_finance=true for financial charts."
)
