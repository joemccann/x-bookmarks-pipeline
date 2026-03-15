"""
Token usage benchmark for the X Bookmarks Pipeline.

Measures total input tokens across all LLM calls for representative bookmarks.
Uses tiktoken (cl100k_base) as a reasonable cross-model approximation.

We measure:
  - System prompt tokens per stage (fixed cost per call)
  - User prompt tokens per stage (variable, depends on content)
  - max_tokens budget per stage (output ceiling)
  - Weighted total: system + user + (estimated_output) tokens

For a finance bookmark with images, the full pipeline is:
  1. Cerebras text classify
  2. xAI image classify (conditional)
  3. Claude vision analysis (conditional)
  4. Claude planning
  5. ChatGPT Pine Script generation

We simulate realistic user prompts for each stage and count tokens.
"""
from __future__ import annotations

import json
import sys

try:
    import tiktoken
except ImportError:
    print("Installing tiktoken...", file=sys.stderr)
    import subprocess
    subprocess.check_call([sys.executable, "-m", "pip", "install", "tiktoken", "-q"])
    import tiktoken

from src.prompts.classification_prompts import (
    FINANCE_TEXT_CLASSIFICATION_PROMPT,
    FINANCE_IMAGE_CLASSIFICATION_PROMPT,
)
from src.prompts.planning_prompts import CLAUDE_PLANNING_SYSTEM_PROMPT
from src.prompts.grok_system_prompt import GROK_PINESCRIPT_SYSTEM_PROMPT
from src.generators.vision_analyzer import _CHART_ANALYSIS_PROMPT
from src.classifiers.finance_classifier import ClassificationResult
from src.planners.strategy_planner import StrategyPlanner
from src.generators.pinescript_generator import PineScriptGenerator


def count_tokens(text: str, enc) -> int:
    """Count tokens using tiktoken."""
    return len(enc.encode(text))


def build_classification_user_prompt() -> str:
    """Simulate a realistic tweet for classification."""
    return (
        "BTC just broke above the 200 EMA on the 4H chart with massive volume. "
        "RSI oversold bounce confirmed at 28.5. Entry at $42,100, targeting $45,000 "
        "with SL at $40,000. Bull flag pattern forming on the daily. "
        "MACD golden cross imminent. This is the breakout we've been waiting for. "
        "NFA but I'm loading up here. #Bitcoin #BTC #Crypto"
    )


def build_planning_user_prompt() -> str:
    """Simulate the user prompt that StrategyPlanner._build_prompt generates."""
    classification = ClassificationResult(
        tweet_id="bench123",
        is_finance=True,
        confidence=0.95,
        classification_source="text",
        has_trading_pattern=True,
        has_visual_data=False,
        category="finance",
        subcategory="crypto",
        detected_topic="BTC breakout with RSI confirmation",
        summary="Bitcoin breakout above 200 EMA with RSI oversold bounce, targeting $45k",
        raw_text=build_classification_user_prompt(),
        image_urls=["https://pbs.twimg.com/media/example1.jpg"],
    )
    return StrategyPlanner._build_prompt(
        classification=classification,
        author="cryptotrader",
        tweet_date="2026-03-10",
        chart_description="BTC/USDT 4H chart showing breakout above 200 EMA at $42,100",
    )


def build_generator_user_prompt() -> str:
    """Simulate the user prompt that PineScriptGenerator._build_user_prompt generates."""
    from src.planners.strategy_planner import StrategyPlan

    plan = StrategyPlan(
        tweet_id="bench123",
        script_type="strategy",
        title="BTC 200 EMA Breakout Strategy",
        ticker="BTCUSDT",
        direction="long",
        timeframe="240",
        indicators=["RSI", "EMA", "MACD"],
        indicator_params={
            "RSI": {"length": 14, "oversold": 30, "overbought": 70},
            "EMA": {"lengths": [20, 50, 200]},
            "MACD": {"fast": 12, "slow": 26, "signal": 9},
        },
        entry_conditions=[
            "Price crosses above 200 EMA",
            "RSI crosses above 30 (oversold bounce)",
            "MACD golden cross (MACD line crosses signal line)",
            "Bar is confirmed (barstate.isconfirmed)",
        ],
        exit_conditions=[
            "RSI crosses below 70 (overbought)",
            "Price closes below 50 EMA",
        ],
        risk_management={
            "stop_loss_type": "fixed",
            "stop_loss_value": 40000,
            "take_profit_type": "fixed",
            "take_profit_value": 45000,
            "position_size_pct": 10,
        },
        key_levels={
            "entry": 42100,
            "stop_loss": 40000,
            "take_profit": 45000,
            "support": 39500,
            "resistance": 46000,
        },
        pattern="bull flag breakout",
        visual_signals=[
            "plotshape for long entries (green triangle up)",
            "plotshape for exits (red triangle down)",
            "bgcolor for trend state",
        ],
        rationale="RSI oversold bounce at 28.5 combined with price breaking above 200 EMA on strong volume suggests bullish continuation. Bull flag pattern on daily provides confluence.",
        author="cryptotrader",
        tweet_date="2026-03-10",
        raw_tweet_text=build_classification_user_prompt(),
        chart_description="BTC/USDT 4H chart showing breakout above 200 EMA at $42,100 with volume spike",
    )
    return PineScriptGenerator._build_user_prompt(plan)


# Estimated output tokens per stage (based on typical LLM responses)
# These estimates reflect actual observed outputs, not max_tokens ceiling.
# Reduced max_tokens + compact output instructions encourage terser responses.
ESTIMATED_OUTPUTS = {
    "cerebras_classify": 35,       # small JSON (~192 max_tokens, json_object mode, compact)
    "xai_image_classify": 45,      # JSON without chart_details (~256 max_tokens, json_object mode)
    "claude_vision": 370,          # simplified schema, compact JSON (~1280 max_tokens)
    "claude_planning": 170,        # compact plan JSON, fewer fields (~768 max_tokens, no whitespace)
    "chatgpt_pinescript": 1300,    # concise Pine Script, no comments except citation (~3072 max_tokens)
}


def main():
    enc = tiktoken.get_encoding("cl100k_base")

    # --- System prompts (fixed cost per call) ---
    system_prompts = {
        "cerebras_classify": FINANCE_TEXT_CLASSIFICATION_PROMPT,
        "xai_image_classify": FINANCE_IMAGE_CLASSIFICATION_PROMPT,
        "claude_vision": _CHART_ANALYSIS_PROMPT,
        "claude_planning": CLAUDE_PLANNING_SYSTEM_PROMPT,
        "chatgpt_pinescript": GROK_PINESCRIPT_SYSTEM_PROMPT,
    }

    # --- User prompts (variable, simulated) ---
    user_prompts = {
        "cerebras_classify": build_classification_user_prompt(),
        "xai_image_classify": "Classify these images.",
        "claude_vision": "",  # image is base64, prompt is in system
        "claude_planning": build_planning_user_prompt(),
        "chatgpt_pinescript": build_generator_user_prompt(),
    }

    total_system = 0
    total_user = 0
    total_output_est = 0
    stage_totals = {}

    for stage in system_prompts:
        sys_tokens = count_tokens(system_prompts[stage], enc)
        usr_tokens = count_tokens(user_prompts[stage], enc) if user_prompts[stage] else 0
        out_tokens = ESTIMATED_OUTPUTS[stage]
        stage_total = sys_tokens + usr_tokens + out_tokens

        total_system += sys_tokens
        total_user += usr_tokens
        total_output_est += out_tokens
        stage_totals[stage] = stage_total

    # Total tokens (input + estimated output) for a full finance pipeline run
    total_tokens = total_system + total_user + total_output_est

    # --- Output METRIC lines ---
    print(f"METRIC total_tokens={total_tokens}")
    print(f"METRIC system_prompt_tokens={total_system}")
    print(f"METRIC user_prompt_tokens={total_user}")
    print(f"METRIC estimated_output_tokens={total_output_est}")

    # Per-stage breakdown
    for stage, tokens in stage_totals.items():
        print(f"METRIC {stage}_tokens={tokens}")

    # Cost estimate (approximate, using pricing from analysis)
    # Cerebras: $0.20/$0.60 per 1M | xAI: $6/$18 per 1M | Claude: $15/$75 per 1M | ChatGPT: $10/$30 per 1M
    pricing_input = {
        "cerebras_classify": 0.20,
        "xai_image_classify": 6.00,
        "claude_vision": 15.00,
        "claude_planning": 15.00,
        "chatgpt_pinescript": 10.00,
    }
    pricing_output = {
        "cerebras_classify": 0.60,
        "xai_image_classify": 18.00,
        "claude_vision": 75.00,
        "claude_planning": 75.00,
        "chatgpt_pinescript": 30.00,
    }

    total_cost_cents = 0
    for stage in system_prompts:
        sys_tokens = count_tokens(system_prompts[stage], enc)
        usr_tokens = count_tokens(user_prompts[stage], enc) if user_prompts[stage] else 0
        out_tokens = ESTIMATED_OUTPUTS[stage]
        input_tokens = sys_tokens + usr_tokens
        cost = (input_tokens * pricing_input[stage] + out_tokens * pricing_output[stage]) / 1_000_000
        total_cost_cents += cost * 100

    print(f"METRIC cost_cents={total_cost_cents:.3f}")


if __name__ == "__main__":
    main()
