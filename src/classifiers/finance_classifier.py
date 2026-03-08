"""
Finance Classifier — uses xAI Grok to determine if a tweet is finance-related.

Two-phase classification:
  1. Analyze tweet text
  2. If text is non-finance but images exist, analyze images via vision
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Optional

from src.clients.xai_client import XAIClient
from src.clients.base_client import ClientError
from src.prompts.classification_prompts import (
    FINANCE_TEXT_CLASSIFICATION_PROMPT,
    FINANCE_IMAGE_CLASSIFICATION_PROMPT,
)


@dataclass
class ClassificationResult:
    """Result of finance classification."""
    tweet_id: str
    is_finance: bool = False
    confidence: float = 0.0
    classification_source: str = ""   # "text" | "image" | "none"
    has_trading_pattern: bool = False
    detected_topic: str = ""
    summary: str = ""
    raw_text: str = ""
    image_urls: list[str] = field(default_factory=list)


class ClassificationError(Exception):
    """Raised when classification fails."""


class FinanceClassifier:
    """Two-phase finance classifier using xAI Grok."""

    def __init__(self, client: Optional[XAIClient] = None) -> None:
        self.client = client or XAIClient()

    def classify(
        self,
        tweet_id: str,
        text: str,
        image_urls: list[str] | None = None,
    ) -> ClassificationResult:
        """Classify a tweet — text first, then images if needed."""
        image_urls = image_urls or []

        result = ClassificationResult(
            tweet_id=tweet_id,
            raw_text=text,
            image_urls=image_urls,
        )

        # Phase 1: text classification
        text_result = self._classify_text(text)
        if text_result.get("is_finance"):
            result.is_finance = True
            result.confidence = text_result.get("confidence", 0.0)
            result.classification_source = "text"
            result.has_trading_pattern = text_result.get("has_trading_pattern", False)
            result.detected_topic = text_result.get("detected_topic", "")
            result.summary = text_result.get("summary", "")
            return result

        # Phase 2: image classification (if text was non-finance)
        if image_urls:
            image_result = self._classify_images(image_urls)
            if image_result.get("is_finance"):
                result.is_finance = True
                result.confidence = image_result.get("confidence", 0.0)
                result.classification_source = "image"
                result.has_trading_pattern = image_result.get("has_trading_pattern", False)
                result.detected_topic = image_result.get("detected_topic", "")
                result.summary = image_result.get("summary", "")
                return result

        # Neither text nor images are finance
        result.classification_source = "none"
        result.summary = text_result.get("summary", "Not finance-related")
        result.confidence = text_result.get("confidence", 0.0)
        return result

    def _classify_text(self, text: str) -> dict:
        """Classify tweet text via Grok."""
        messages = [
            {"role": "system", "content": FINANCE_TEXT_CLASSIFICATION_PROMPT},
            {"role": "user", "content": text},
        ]
        try:
            response = self.client.chat(messages=messages, max_tokens=512)
            return self._parse_json(response.content)
        except ClientError as e:
            raise ClassificationError(f"Text classification failed: {e}")

    def _classify_images(self, image_urls: list[str]) -> dict:
        """Classify chart images via Grok vision."""
        try:
            response = self.client.chat_with_vision(
                system_prompt=FINANCE_IMAGE_CLASSIFICATION_PROMPT,
                text_prompt="Analyze these images and classify them.",
                image_urls=image_urls,
                max_tokens=1024,
            )
            return self._parse_json(response.content)
        except ClientError as e:
            raise ClassificationError(f"Image classification failed: {e}")

    @staticmethod
    def _parse_json(text: str) -> dict:
        """Parse JSON from LLM response, handling markdown fences."""
        cleaned = text.strip()
        if cleaned.startswith("```"):
            # Strip markdown code fences
            lines = cleaned.split("\n")
            lines = [l for l in lines if not l.strip().startswith("```")]
            cleaned = "\n".join(lines)
        try:
            return json.loads(cleaned)
        except json.JSONDecodeError:
            return {"is_finance": False, "summary": "Failed to parse classification response"}
