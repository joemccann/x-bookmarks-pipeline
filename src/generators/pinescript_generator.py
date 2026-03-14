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
        model: str | None = None,
        timeout: float | None = None,
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
            response = self.client.chat(messages=messages, max_tokens=3072)
            return self._extract_pinescript(response.content)
        except ClientError as e:
            raise GenerationError(f"Pine Script generation failed: {e}")

    @staticmethod
    def _build_user_prompt(plan: StrategyPlan) -> str:
        """Build a compact JSON plan for the code generator.

        Sends only the structured plan data — raw tweet text and chart
        description are omitted because the plan already encodes all
        relevant information extracted from those fields.
        """
        compact: dict = {
            "type": plan.script_type,
            "title": plan.title,
            "author": plan.author,
            "date": plan.tweet_date,
            "ticker": plan.ticker,
            "direction": plan.direction,
            "timeframe": plan.timeframe,
        }
        if plan.indicators:
            # Merge indicators + params into compact format
            if plan.indicator_params:
                ind_parts = []
                for ind in plan.indicators:
                    p = plan.indicator_params.get(ind)
                    if p:
                        vals = "/".join(str(v) if not isinstance(v, list) else "+".join(str(x) for x in v) for v in p.values())
                        ind_parts.append(f"{ind}({vals})")
                    else:
                        ind_parts.append(ind)
                compact["ind"] = ",".join(ind_parts)
            else:
                compact["indicators"] = plan.indicators
        if plan.entry_conditions:
            compact["entry"] = plan.entry_conditions
        if plan.exit_conditions:
            compact["exit"] = plan.exit_conditions
        if plan.risk_management:
            rm = plan.risk_management
            sl_type = rm.get("stop_loss_type") or rm.get("sl_type", "pct")
            sl_val = rm.get("stop_loss_value") or rm.get("sl_value", 2)
            tp_type = rm.get("take_profit_type") or rm.get("tp_type", "pct")
            tp_val = rm.get("take_profit_value") or rm.get("tp_value", 4)
            size = rm.get("position_size_pct") or rm.get("size_pct", 10)
            compact["risk"] = f"SL:{sl_type}@{sl_val},TP:{tp_type}@{tp_val},size:{size}%"
        # key_levels omitted — risk_management has SL/TP values, entry_conditions has entry logic
        # pattern omitted — captured in entry_conditions/indicators
        # rationale omitted — entry/exit conditions capture the logic

        prompt = json.dumps(compact, separators=(",", ":"))

        if plan.script_type == "indicator":
            prompt += "\nUse indicator(), no strategy.* calls."

        return prompt

    @staticmethod
    def _extract_pinescript(response: str) -> str:
        """Extract Pine Script code block from the LLM response.

        Handles: ```pinescript, ```pine, ```typescript, ```javascript,
        bare ``` fences, and raw //@version responses.
        """
        # Try fenced code block with any language tag
        m = re.search(
            r"```\w*\s*\n(.*?)```",
            response,
            re.DOTALL,
        )
        if m:
            code = m.group(1).strip()
            if "//@version" in code:
                return code

        # Raw response starting with //@version (no fences)
        if response.strip().startswith("//@version"):
            return response.strip()

        # Last resort: find //@version=6 anywhere and take everything from there
        idx = response.find("//@version")
        if idx != -1:
            # Take from //@version to end, strip trailing ``` if present
            code = response[idx:].strip()
            if code.endswith("```"):
                code = code[:-3].strip()
            return code

        return response.strip()
