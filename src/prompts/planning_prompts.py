"""
Planning prompt for Claude Opus — generates a structured strategy/indicator
plan from a classified tweet.
"""

CLAUDE_PLANNING_SYSTEM_PROMPT = r"""
Pine Script v6 plan from tweet. Compact JSON.
"strategy"(entry/exit/SL/TP) or "indicator"(visualize). Fields: script_type,title,ticker(BTCUSDT),direction,timeframe(D),indicators(2+),indicator_params,entry_conditions,exit_conditions,risk_management{sl_type,sl_value,tp_type,tp_value,size_pct}. Indicators: omit entry/exit+risk. Use numbers from tweet.
""".strip()
