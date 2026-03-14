"""
Bookmark Classifier — uses Cerebras for fast text classification and
xAI Grok for image/vision classification.

Two-phase classification:
  1. Analyze tweet text (Cerebras — fast)
  2. If text is non-finance but images exist, analyze images via vision (xAI Grok)

Every bookmark gets a category/subcategory. Finance bookmarks continue
through the full Pine Script pipeline.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Optional

from src.clients.base_client import ClientError
from src.prompts.classification_prompts import (
    FINANCE_TEXT_CLASSIFICATION_PROMPT,
    FINANCE_IMAGE_CLASSIFICATION_PROMPT,
)


@dataclass
class ClassificationResult:
    """Result of bookmark classification."""
    tweet_id: str
    is_finance: bool = False
    confidence: float = 0.0
    classification_source: str = ""   # "text" | "image" | "none"
    has_trading_pattern: bool = False
    has_visual_data: bool = False
    category: str = "other"
    subcategory: str = "general"
    detected_topic: str = ""
    summary: str = ""
    raw_text: str = ""
    image_urls: list[str] = field(default_factory=list)


class ClassificationError(Exception):
    """Raised when classification fails."""


class BookmarkClassifier:
    """Two-phase bookmark classifier: Cerebras (text) + xAI Grok (vision)."""

    def __init__(
        self,
        text_client=None,
        vision_client=None,
        # Backward-compatible: single `client` kwarg uses same client for both
        client=None,
    ) -> None:
        if client is not None:
            # Legacy single-client mode
            self.text_client = client
            self.vision_client = client
        else:
            if text_client is None:
                from src.clients.cerebras_client import CerebrasClient
                text_client = CerebrasClient()
            if vision_client is None:
                from src.clients.xai_client import XAIClient
                vision_client = XAIClient()
            self.text_client = text_client
            self.vision_client = vision_client

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

        # Phase 1: text classification (Cerebras — fast)
        text_result = self._classify_text(text)
        self._apply_result(result, text_result, source="text")

        # If text says finance, we're done
        if result.is_finance:
            return result

        # Phase 2: image classification (xAI Grok — vision)
        if image_urls:
            image_result = self._classify_images(image_urls)
            if image_result.get("is_finance"):
                self._apply_result(result, image_result, source="image")
                return result
            # Even if not finance, images may have visual data
            if image_result.get("has_visual_data"):
                result.has_visual_data = True

        # Finalize non-finance result
        if not result.is_finance:
            result.classification_source = result.classification_source or "none"

        return result

    @staticmethod
    def _apply_result(result: ClassificationResult, data: dict, source: str) -> None:
        """Apply parsed classification data to result."""
        result.is_finance = data.get("is_finance", False)
        result.confidence = data.get("confidence", 0.0)
        result.classification_source = source
        result.has_trading_pattern = data.get("has_trading_pattern", False)
        result.has_visual_data = data.get("has_visual_data", False)
        result.category = data.get("category", "other")
        result.subcategory = data.get("subcategory", "general")
        result.detected_topic = data.get("detected_topic", "")
        result.summary = data.get("summary", "")

    def _classify_text(self, text: str) -> dict:
        """Classify tweet text via Cerebras."""
        messages = [
            {"role": "system", "content": FINANCE_TEXT_CLASSIFICATION_PROMPT},
            {"role": "user", "content": text},
        ]
        try:
            response = self.text_client.chat(
                messages=messages,
                max_tokens=192,
                response_format={"type": "json_object"},
            )
            return self._parse_json(response.content)
        except ClientError as e:
            raise ClassificationError(f"Text classification failed: {e}")

    def _classify_images(self, image_urls: list[str]) -> dict:
        """Classify chart images via xAI Grok vision."""
        try:
            response = self.vision_client.chat_with_vision(
                system_prompt=FINANCE_IMAGE_CLASSIFICATION_PROMPT,
                text_prompt="Classify these images.",
                image_urls=image_urls,
                max_tokens=384,
                response_format={"type": "json_object"},
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


# Backward-compatible alias
FinanceClassifier = BookmarkClassifier
