"""
Pine Script Generator — calls xAI Grok to convert a TradingSignal into
a complete Pine Script v6 strategy.

This module is the core bridge between X bookmark data and executable
TradingView code.
"""

from __future__ import annotations

import json
import os
import re
from dataclasses import asdict
from typing import Optional

import httpx

from src.parsers.bookmark_parser import TradingSignal
from src.prompts import GROK_PINESCRIPT_SYSTEM_PROMPT


_XAI_BASE_URL = "https://api.x.ai/v1"
_DEFAULT_MODEL = "grok-4.1"


class PineScriptGenerator:
    """Bridge that sends structured bookmark data to Grok and returns Pine Script."""

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str = _DEFAULT_MODEL,
        base_url: str = _XAI_BASE_URL,
        timeout: float = 120.0,
    ) -> None:
        self.api_key = api_key or os.environ.get("XAI_API_KEY", "")
        if not self.api_key:
            raise ValueError(
                "xAI API key is required. Set XAI_API_KEY env var or pass api_key=."
            )
        self.model = model
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def generate(self, signal: TradingSignal) -> str:
        """Send the trading signal to Grok and return raw Pine Script code."""
        user_prompt = self._build_user_prompt(signal)
        response_text = self._call_grok(user_prompt, signal.chart_description)
        pine_code = self._extract_pinescript(response_text)
        return pine_code

    # ------------------------------------------------------------------
    # Prompt construction
    # ------------------------------------------------------------------

    @staticmethod
    def _build_user_prompt(signal: TradingSignal) -> str:
        parts: list[str] = []
        parts.append("=== BOOKMARK PAYLOAD ===\n")
        parts.append(f"Author : @{signal.author}")
        parts.append(f"Date   : {signal.tweet_date}")
        parts.append(f"Ticker : {signal.ticker}")
        parts.append(f"Direction : {signal.direction}")
        parts.append(f"Timeframe : {signal.timeframe}")
        parts.append(f"\n--- Tweet Text ---\n{signal.raw_text}")

        if signal.chart_description:
            parts.append(f"\n--- Chart Image Description ---\n{signal.chart_description}")

        if signal.indicators:
            parts.append(f"\n--- Detected Indicators ---\n{', '.join(signal.indicators)}")

        if signal.pattern:
            parts.append(f"\n--- Detected Pattern ---\n{signal.pattern}")

        if signal.key_levels:
            levels_str = json.dumps(signal.key_levels, indent=2)
            parts.append(f"\n--- Key Price Levels ---\n{levels_str}")

        parts.append(
            "\nGenerate the Pine Script v6 strategy now. "
            "Follow every rule in your system prompt."
        )
        return "\n".join(parts)

    # ------------------------------------------------------------------
    # Grok API call
    # ------------------------------------------------------------------

    def _call_grok(self, user_prompt: str, chart_description: str = "") -> str:
        """Call the xAI chat completions endpoint."""
        messages = [
            {"role": "system", "content": GROK_PINESCRIPT_SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ]

        payload = {
            "model": self.model,
            "messages": messages,
            "temperature": 0.2,
            "max_tokens": 4096,
        }

        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }

        with httpx.Client(timeout=self.timeout) as client:
            resp = client.post(
                f"{self.base_url}/chat/completions",
                headers=headers,
                json=payload,
            )
            resp.raise_for_status()

        data = resp.json()
        return data["choices"][0]["message"]["content"]

    # ------------------------------------------------------------------
    # Post-processing
    # ------------------------------------------------------------------

    @staticmethod
    def _extract_pinescript(response: str) -> str:
        """Extract the Pine Script code block from Grok's response."""
        # Try fenced code block first
        m = re.search(
            r"```(?:pinescript|pine)?\s*\n(.*?)```",
            response,
            re.DOTALL,
        )
        if m:
            return m.group(1).strip()

        # Fallback: if the response starts with //@version, take everything
        if response.strip().startswith("//@version"):
            return response.strip()

        # Last resort: return the whole response
        return response.strip()
