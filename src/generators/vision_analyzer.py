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

_CHART_ANALYSIS_PROMPT = """
You are an expert technical analyst. Analyze this trading chart image and provide
a detailed, structured description including:

1. Asset / ticker if visible
2. Timeframe (e.g. 4h, Daily, Weekly)
3. Chart type (candlestick, line, bar)
4. Key price levels visible (support, resistance, breakout zones) — include exact numbers
5. Any chart patterns (head & shoulders, triangles, flags, wedges, channels, etc.)
6. Any indicators visible (RSI, MACD, EMAs, Bollinger Bands, Volume, etc.) and their current values/signals
7. Overall trend direction (uptrend, downtrend, ranging)
8. Any trendlines, channels, or Fibonacci levels drawn on the chart
9. Notable candle formations at key levels

Be precise with price levels — extract every number you can see on the price axis.
Format: plain prose, no markdown, no bullet points. Be concise but thorough.
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
        timeout: float = 60.0,
    ) -> None:
        self.client = client or AnthropicClient()
        self.timeout = timeout

    def analyze(self, image_url: str) -> str:
        """
        Send image to Claude vision and return a chart description.

        Downloads the image first (Anthropic requires base64), then sends
        it via the Messages API with vision content blocks.
        """
        if not image_url:
            return ""

        print(f"  [vision] Downloading image: {image_url[:80]}...")
        t0 = time.time()
        try:
            b64_data, media_type = _fetch_image_as_base64(image_url, self.timeout)
        except Exception as exc:
            print(f"  [vision] FAILED to download image ({time.time() - t0:.1f}s): {exc}")
            return ""
        img_kb = len(b64_data) * 3 // 4 // 1024
        print(f"  [vision] Downloaded {img_kb}KB {media_type} ({time.time() - t0:.1f}s)")

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

        print(f"  [vision] Sending to Claude for analysis...")
        t1 = time.time()
        try:
            response = self.client.chat(
                messages=messages,
                temperature=0.1,
                max_tokens=1024,
            )
            elapsed = time.time() - t1
            desc = response.content.strip()
            print(f"  [vision] Claude responded ({elapsed:.1f}s, {len(desc)} chars)")
            return desc
        except ClientError as exc:
            print(f"  [vision] FAILED Claude analysis ({time.time() - t1:.1f}s): {exc}")
            return ""

    def analyze_all(self, image_urls: list[str]) -> str:
        """Analyze multiple images and concatenate their descriptions."""
        parts = []
        for i, url in enumerate(image_urls, 1):
            if url:
                print(f"  [vision] Image {i}/{len(image_urls)}")
                parts.append(self.analyze(url))
        return "\n\n".join(p for p in parts if p)


# Backward-compatible alias
GrokVisionAnalyzer = ClaudeVisionAnalyzer
