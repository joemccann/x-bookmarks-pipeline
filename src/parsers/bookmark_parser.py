"""
Bookmark Parser — extracts structured trading data from raw X bookmark content.

Handles both the tweet text and an optional Grok vision-generated chart
description, producing a normalized payload for the Pine Script generator.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class TradingSignal:
    """Normalized representation of a single trading idea from a bookmark."""

    ticker: str = "BTCUSDT"
    direction: str = "long"  # "long", "short", or "both"
    timeframe: str = "D"  # TradingView timeframe string
    entry_price: Optional[float] = None
    stop_loss: Optional[float] = None
    take_profit: Optional[float] = None
    indicators: list[str] = field(default_factory=list)
    key_levels: dict[str, float] = field(default_factory=dict)
    pattern: Optional[str] = None
    author: str = ""
    tweet_date: str = ""
    raw_text: str = ""
    chart_description: str = ""


# Common ticker aliases found in crypto/finance Twitter
_TICKER_PATTERNS: list[tuple[str, str]] = [
    (r"\$?BTC(?:USD[T]?)?", "BTCUSDT"),
    (r"\$?ETH(?:USD[T]?)?", "ETHUSDT"),
    (r"\$?SOL(?:USD[T]?)?", "SOLUSDT"),
    (r"\$?SPX|S&P\s*500", "SPX"),
    (r"\$?QQQ", "QQQ"),
    (r"\$?AAPL", "AAPL"),
    (r"\$?TSLA", "TSLA"),
    (r"\$?NVDA", "NVDA"),
    (r"\$([A-Z]{1,5})\b", r"\1"),  # generic $TICKER
]

_INDICATOR_KEYWORDS: dict[str, str] = {
    "rsi": "RSI",
    "macd": "MACD",
    "ema": "EMA",
    "sma": "SMA",
    "bollinger": "Bollinger Bands",
    "vwap": "VWAP",
    "volume": "Volume",
    "fibonacci": "Fibonacci",
    "fib": "Fibonacci",
    "ichimoku": "Ichimoku",
    "atr": "ATR",
    "stochastic": "Stochastic",
    "obv": "OBV",
    "supertrend": "SuperTrend",
}

_PATTERN_KEYWORDS: list[str] = [
    "head and shoulders",
    "inverse head and shoulders",
    "double top",
    "double bottom",
    "triple top",
    "triple bottom",
    "ascending triangle",
    "descending triangle",
    "symmetrical triangle",
    "bull flag",
    "bear flag",
    "rising wedge",
    "falling wedge",
    "cup and handle",
    "pennant",
    "channel",
]

_TIMEFRAME_MAP: dict[str, str] = {
    "1m": "1",
    "5m": "5",
    "15m": "15",
    "30m": "30",
    "1h": "60",
    "4h": "240",
    "1d": "D",
    "daily": "D",
    "1w": "W",
    "weekly": "W",
    "monthly": "M",
}


class BookmarkParser:
    """Parse raw bookmark content into a structured ``TradingSignal``."""

    def parse(
        self,
        tweet_text: str,
        chart_description: str = "",
        author: str = "",
        tweet_date: str = "",
    ) -> TradingSignal:
        combined = f"{tweet_text} {chart_description}".lower()

        signal = TradingSignal(
            author=author,
            tweet_date=tweet_date,
            raw_text=tweet_text,
            chart_description=chart_description,
        )

        signal.ticker = self._extract_ticker(combined)
        signal.direction = self._extract_direction(combined)
        signal.timeframe = self._extract_timeframe(combined)
        signal.indicators = self._extract_indicators(combined)
        signal.pattern = self._extract_pattern(combined)
        signal.key_levels = self._extract_levels(tweet_text, chart_description)
        signal.entry_price = signal.key_levels.get("entry")
        signal.stop_loss = signal.key_levels.get("stop_loss")
        signal.take_profit = signal.key_levels.get("take_profit")

        return signal

    # ------------------------------------------------------------------
    # Private helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _extract_ticker(text: str) -> str:
        for pattern, replacement in _TICKER_PATTERNS:
            m = re.search(pattern, text, re.IGNORECASE)
            if m:
                if "\\" in replacement:
                    return re.sub(pattern, replacement, m.group(0), flags=re.IGNORECASE)
                return replacement
        return "BTCUSDT"

    @staticmethod
    def _extract_direction(text: str) -> str:
        long_kw = ["long", "buy", "bullish", "calls", "breakout", "bounce"]
        short_kw = ["short", "sell", "bearish", "puts", "breakdown", "dump"]
        has_long = any(kw in text for kw in long_kw)
        has_short = any(kw in text for kw in short_kw)
        if has_long and has_short:
            return "both"
        if has_short:
            return "short"
        return "long"

    @staticmethod
    def _extract_timeframe(text: str) -> str:
        for kw, tf in _TIMEFRAME_MAP.items():
            if kw in text:
                return tf
        return "D"

    @staticmethod
    def _extract_indicators(text: str) -> list[str]:
        found: list[str] = []
        for kw, label in _INDICATOR_KEYWORDS.items():
            if kw in text and label not in found:
                found.append(label)
        return found

    @staticmethod
    def _extract_pattern(text: str) -> str | None:
        for pat in _PATTERN_KEYWORDS:
            if pat in text:
                return pat
        return None

    @staticmethod
    def _extract_levels(tweet_text: str, chart_description: str) -> dict[str, float]:
        """Pull dollar-denominated price levels from the text."""
        levels: dict[str, float] = {}
        combined = f"{tweet_text} {chart_description}"

        def _parse_price(match_str: str) -> float:
            """Parse a price string like '$42,000', '$42k', '$42.5k'."""
            cleaned = match_str.strip().lstrip("$").strip()
            has_k = cleaned.lower().endswith("k")
            if has_k:
                cleaned = cleaned[:-1]
            val = float(cleaned.replace(",", ""))
            if has_k:
                val *= 1000
            return val

        # Regex that captures the full price token including optional k suffix
        _price_tok = r"\$\s?([\d,]+\.?\d*)\s*k?"

        # Try labelled prices first
        labelled = {
            "entry": r"(?:entry|buy|enter|long|short)\s*(?:at|@|:)?\s*(" + _price_tok + r")",
            "stop_loss": r"(?:stop[- ]?loss|sl|stop)\s*(?:at|@|:)?\s*(" + _price_tok + r")",
            "take_profit": r"(?:take[- ]?profit|tp|target|tgt)\s*(?:at|@|:)?\s*(" + _price_tok + r")",
            "support": r"(?:support)\s*(?:at|@|:)?\s*(" + _price_tok + r")",
            "resistance": r"(?:resistance)\s*(?:at|@|:)?\s*(" + _price_tok + r")",
        }

        for label, pattern in labelled.items():
            m = re.search(pattern, combined, re.IGNORECASE)
            if m:
                levels[label] = _parse_price(m.group(1))

        # Grab any remaining unlabelled prices as "level_N"
        all_prices = re.finditer(_price_tok, combined, re.IGNORECASE)
        idx = 0
        for m in all_prices:
            val = _parse_price(m.group(0))
            key = f"level_{idx}"
            levels.setdefault(key, val)
            idx += 1

        return levels
