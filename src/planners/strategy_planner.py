"""
Strategy Planner — uses Claude Opus to create a structured plan
for Pine Script generation from a classified tweet.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Optional

from src.clients.anthropic_client import AnthropicClient
from src.clients.base_client import ClientError
from src.classifiers.finance_classifier import ClassificationResult
from src.prompts.planning_prompts import CLAUDE_PLANNING_SYSTEM_PROMPT


@dataclass
class StrategyPlan:
    """Structured plan for Pine Script generation."""
    tweet_id: str
    script_type: str = "strategy"     # "strategy" | "indicator"
    title: str = ""
    ticker: str = "BTCUSDT"
    direction: str = "long"
    timeframe: str = "D"
    indicators: list[str] = field(default_factory=list)
    indicator_params: dict = field(default_factory=dict)
    entry_conditions: list[str] = field(default_factory=list)
    exit_conditions: list[str] = field(default_factory=list)
    risk_management: dict = field(default_factory=dict)
    key_levels: dict = field(default_factory=dict)
    pattern: Optional[str] = None
    visual_signals: list[str] = field(default_factory=list)
    rationale: str = ""
    author: str = ""
    tweet_date: str = ""
    raw_tweet_text: str = ""
    chart_description: str = ""


class PlanningError(Exception):
    """Raised when strategy planning fails."""


class StrategyPlanner:
    """Uses Claude Opus to create strategy/indicator plans."""

    def __init__(self, client: Optional[AnthropicClient] = None) -> None:
        self.client = client or AnthropicClient()

    def plan(
        self,
        classification: ClassificationResult,
        author: str = "",
        tweet_date: str = "",
        chart_description: str = "",
    ) -> StrategyPlan:
        """Create a strategy/indicator plan from a classification result."""
        user_prompt = self._build_prompt(classification, author, tweet_date, chart_description)

        messages = [
            {"role": "system", "content": CLAUDE_PLANNING_SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ]

        try:
            response = self.client.chat(messages=messages, max_tokens=768)
            plan_data = self._parse_json(response.content)
        except ClientError as e:
            raise PlanningError(f"Planning failed: {e}")

        return StrategyPlan(
            tweet_id=classification.tweet_id,
            script_type=plan_data.get("script_type", "strategy"),
            title=plan_data.get("title", ""),
            ticker=plan_data.get("ticker", "BTCUSDT"),
            direction=plan_data.get("direction", "long"),
            timeframe=plan_data.get("timeframe", "D"),
            indicators=plan_data.get("indicators", []),
            indicator_params=plan_data.get("indicator_params", {}),
            entry_conditions=plan_data.get("entry_conditions", []),
            exit_conditions=plan_data.get("exit_conditions", []),
            risk_management=plan_data.get("risk_management", {}),
            key_levels=plan_data.get("key_levels", {}),
            pattern=plan_data.get("pattern"),
            visual_signals=plan_data.get("visual_signals", []),
            rationale=plan_data.get("rationale", ""),
            author=author or classification.raw_text[:50],
            tweet_date=tweet_date,
            raw_tweet_text=classification.raw_text,
            chart_description=chart_description or classification.summary,
        )

    @staticmethod
    def _build_prompt(
        classification: ClassificationResult,
        author: str,
        tweet_date: str,
        chart_description: str,
    ) -> str:
        import json as _json
        data: dict = {
            "author": author,
            "date": tweet_date,
            "topic": classification.detected_topic,
            "pattern": classification.has_trading_pattern,
            "text": classification.raw_text,
        }
        if chart_description:
            data["chart"] = chart_description
        return _json.dumps(data, separators=(",", ":"))

    @staticmethod
    def _parse_json(text: str) -> dict:
        """Parse JSON from LLM response, handling markdown fences."""
        cleaned = text.strip()
        if cleaned.startswith("```"):
            lines = cleaned.split("\n")
            lines = [l for l in lines if not l.strip().startswith("```")]
            cleaned = "\n".join(lines)
        try:
            return json.loads(cleaned)
        except json.JSONDecodeError:
            raise PlanningError(f"Failed to parse planning response as JSON: {cleaned[:200]}")
