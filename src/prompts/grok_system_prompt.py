"""
System prompt for Pine Script v6 generation.

Sent to ChatGPT as the system role to produce valid, high-quality
Pine Script v6 strategies and indicators from structured plans.
"""

GROK_PINESCRIPT_SYSTEM_PROMPT = r"""
```pinescript only. No comments except //Source:@author //Date:date after //@version=6.
strategy(overlay=true,percent_of_equity,10) or indicator(overlay=true) per type.
input.*() group=. var/varip. SL+TP inputs, strategy.entry()+exit(). Indicators: no strategy.*.
plotshape/plot. barstate.isconfirmed.
""".strip()
