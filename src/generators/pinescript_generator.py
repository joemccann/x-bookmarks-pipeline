"""
Pine Script Generator — calls OpenAI ChatGPT to convert a StrategyPlan into
a complete Pine Script v6 strategy or indicator.
"""
from __future__ import annotations

import json
import re
from typing import Optional

from src.clients.openai_client import OpenAIClient
from src.clients.base_client import ClientError
from src.planners.strategy_planner import StrategyPlan
from src.prompts import GROK_PINESCRIPT_SYSTEM_PROMPT


class GenerationError(Exception):
    """Raised when Pine Script generation fails."""


class PineScriptGenerator:
    """Bridge that sends a StrategyPlan to ChatGPT and returns Pine Script."""

    def __init__(
        self,
        client: Optional[OpenAIClient] = None,
        api_key: Optional[str] = None,
        model: str = "gpt-5.4",
        timeout: float = 120.0,
    ) -> None:
        if client:
            self.client = client
        else:
            self.client = OpenAIClient(api_key=api_key, model=model, timeout=timeout)
        self.model = self.client.model

    def generate(self, plan: StrategyPlan) -> str:
        """Send the strategy plan to ChatGPT and return raw Pine Script code."""
        user_prompt = self._build_user_prompt(plan)
        messages = [
            {"role": "system", "content": GROK_PINESCRIPT_SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ]
        try:
            response = self.client.chat(messages=messages, max_tokens=4096)
            return self._extract_pinescript(response.content)
        except ClientError as e:
            raise GenerationError(f"Pine Script generation failed: {e}")

    @staticmethod
    def _build_user_prompt(plan: StrategyPlan) -> str:
        parts: list[str] = []
        parts.append("=== STRATEGY PLAN ===\n")
        parts.append(f"Script Type : {plan.script_type}")
        parts.append(f"Title       : {plan.title}")
        parts.append(f"Author      : @{plan.author}")
        parts.append(f"Date        : {plan.tweet_date}")
        parts.append(f"Ticker      : {plan.ticker}")
        parts.append(f"Direction   : {plan.direction}")
        parts.append(f"Timeframe   : {plan.timeframe}")

        if plan.indicators:
            parts.append(f"\n--- Indicators ---\n{', '.join(plan.indicators)}")

        if plan.indicator_params:
            parts.append(f"\n--- Indicator Parameters ---\n{json.dumps(plan.indicator_params, indent=2)}")

        if plan.entry_conditions:
            parts.append(f"\n--- Entry Conditions ---")
            for i, cond in enumerate(plan.entry_conditions, 1):
                parts.append(f"  {i}. {cond}")

        if plan.exit_conditions:
            parts.append(f"\n--- Exit Conditions ---")
            for i, cond in enumerate(plan.exit_conditions, 1):
                parts.append(f"  {i}. {cond}")

        if plan.risk_management:
            parts.append(f"\n--- Risk Management ---\n{json.dumps(plan.risk_management, indent=2)}")

        if plan.key_levels:
            parts.append(f"\n--- Key Price Levels ---\n{json.dumps(plan.key_levels, indent=2)}")

        if plan.pattern:
            parts.append(f"\n--- Chart Pattern ---\n{plan.pattern}")

        if plan.visual_signals:
            parts.append(f"\n--- Visual Signals ---")
            for sig in plan.visual_signals:
                parts.append(f"  - {sig}")

        if plan.rationale:
            parts.append(f"\n--- Rationale ---\n{plan.rationale}")

        if plan.raw_tweet_text:
            parts.append(f"\n--- Original Tweet ---\n{plan.raw_tweet_text}")

        if plan.chart_description:
            parts.append(f"\n--- Chart Description ---\n{plan.chart_description}")

        script_type = plan.script_type
        parts.append(
            f"\nGenerate the Pine Script v6 {script_type} now. "
            f"Follow every rule in your system prompt."
        )
        if script_type == "indicator":
            parts.append(
                "Use indicator() instead of strategy(). "
                "Do NOT include strategy.entry/exit/order calls."
            )

        return "\n".join(parts)

    @staticmethod
    def _extract_pinescript(response: str) -> str:
        """Extract Pine Script code block from the LLM response."""
        m = re.search(
            r"```(?:pinescript|pine)?\s*\n(.*?)```",
            response,
            re.DOTALL,
        )
        if m:
            return m.group(1).strip()

        if response.strip().startswith("//@version"):
            return response.strip()

        return response.strip()
