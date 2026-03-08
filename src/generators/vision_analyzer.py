"""
Claude Vision Analyzer — uses Anthropic Claude to generate a detailed
chart description from an image URL.

The description is then fed into the pipeline as ``chart_description``
so the Pine Script generator can encode visual patterns and price levels.
"""
from __future__ import annotations

import base64
import os
import time
from typing import Optional

import httpx

from src.clients.anthropic_client import AnthropicClient
from src.clients.base_client import ClientError
from src.config import VISION_TIMEOUT

_CHART_ANALYSIS_PROMPT = """
You are an expert technical analyst and data extraction specialist.

Analyze this image. It may be a trading chart, a data table, a graph, a
screenshot with statistics, or another visual format.

Respond with ONLY valid JSON (no markdown fences, no explanation outside the JSON):

{
  "image_type": "chart" | "table" | "graph" | "screenshot" | "other",
  "description": "Concise plain-text description of what the image shows",
  "asset": {
    "ticker": "detected ticker or empty string",
    "name": "full asset name if visible"
  },
  "chart_analysis": {
    "timeframe": "e.g. 4h, Daily, Weekly or empty string",
    "chart_type": "candlestick, line, bar, area, or empty string",
    "trend_direction": "uptrend, downtrend, ranging, or empty string",
    "patterns": ["list of chart patterns detected"],
    "candle_formations": ["notable candle formations at key levels"]
  },
  "price_levels": {
    "current": null,
    "support": [],
    "resistance": [],
    "breakout_zones": [],
    "all_visible": ["every price number visible on the axis"]
  },
  "indicators": [
    {
      "name": "indicator name",
      "value": "current reading if visible",
      "signal": "bullish, bearish, neutral, or n/a"
    }
  ],
  "drawn_elements": {
    "trendlines": ["description of each trendline"],
    "channels": ["description of channels"],
    "fibonacci_levels": ["fib levels if drawn"],
    "annotations": ["any text annotations on the image"]
  },
  "tabular_data": {
    "headers": ["column headers if table/grid is present"],
    "rows": [["row1col1", "row1col2"], ["row2col1", "row2col2"]],
    "summary": "what the table data represents"
  },
  "statistics": {
    "key_values": {"label": "value"},
    "summary": "what the statistics represent"
  }
}

Rules:
- Extract EVERY number visible in the image — prices, percentages, dates, counts
- For charts: be precise with price levels from the Y-axis
- For tables/grids: extract ALL rows and columns into tabular_data
- For statistics/dashboards: extract all key-value pairs into statistics
- Omit empty sections (empty arrays [] and empty objects {} are fine)
- If a section doesn't apply, use empty defaults ([], {}, null, "")
""".strip()


def _fetch_image_as_base64(url: str, timeout: float = 30.0) -> tuple[str, str]:
    """Download an image URL and return (base64_data, media_type)."""
    with httpx.Client(timeout=timeout) as client:
        resp = client.get(url)
        resp.raise_for_status()
    content_type = resp.headers.get("content-type", "image/jpeg")
    # Normalize media type
    if "png" in content_type:
        media_type = "image/png"
    elif "gif" in content_type:
        media_type = "image/gif"
    elif "webp" in content_type:
        media_type = "image/webp"
    else:
        media_type = "image/jpeg"
    b64 = base64.standard_b64encode(resp.content).decode("ascii")
    return b64, media_type


class ClaudeVisionAnalyzer:
    """
    Analyzes a chart image URL using Claude's vision and returns
    a plain-text description suitable for use as ``chart_description``.
    """

    def __init__(
        self,
        client: Optional[AnthropicClient] = None,
        timeout: float | None = None,
    ) -> None:
        self.client = client or AnthropicClient()
        self.timeout = timeout or VISION_TIMEOUT

    def analyze(self, image_url: str) -> str:
        """
        Send image to Claude vision and return a chart description.

        Downloads the image first (Anthropic requires base64), then sends
        it via the Messages API with vision content blocks.
        """
        from src.console import console

        if not image_url:
            return ""

        console.print(f"    [dim]Downloading image...[/dim]")
        t0 = time.time()
        try:
            b64_data, media_type = _fetch_image_as_base64(image_url, self.timeout)
        except Exception as exc:
            console.print(f"    [bold red]Image download failed[/bold red] [dim]{time.time() - t0:.1f}s: {exc}[/dim]")
            return ""
        img_kb = len(b64_data) * 3 // 4 // 1024
        console.print(f"    [dim]{img_kb}KB {media_type} ({time.time() - t0:.1f}s)[/dim]")

        messages = [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": b64_data,
                        },
                    },
                    {
                        "type": "text",
                        "text": _CHART_ANALYSIS_PROMPT,
                    },
                ],
            }
        ]

        console.print(f"    [dim]Claude vision analyzing...[/dim]")
        t1 = time.time()
        try:
            response = self.client.chat(
                messages=messages,
                temperature=0.1,
                max_tokens=4096,
            )
            elapsed = time.time() - t1
            desc = response.content.strip()
            console.print(f"    [dim]Vision done ({elapsed:.1f}s, {len(desc)} chars)[/dim]")
            return desc
        except ClientError as exc:
            console.print(f"    [bold red]Vision failed[/bold red] [dim]{time.time() - t1:.1f}s: {exc}[/dim]")
            return ""

    def analyze_all(self, image_urls: list[str]) -> str:
        """Analyze multiple images and concatenate their descriptions."""
        from src.console import console
        parts = []
        for i, url in enumerate(image_urls, 1):
            if url:
                console.print(f"    [dim]Image {i}/{len(image_urls)}[/dim]")
                parts.append(self.analyze(url))
        return "\n\n".join(p for p in parts if p)


# Backward-compatible alias
GrokVisionAnalyzer = ClaudeVisionAnalyzer
