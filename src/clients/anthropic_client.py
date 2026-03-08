"""
Anthropic Claude client — Messages API via httpx.
"""
from __future__ import annotations

import os
from typing import Optional

from .base_client import BaseClient, ClientError, LLMResponse
from src.config import ANTHROPIC_MODEL, API_TIMEOUT

_BASE_URL = "https://api.anthropic.com/v1"


class AnthropicClient(BaseClient):
    """Thin httpx wrapper for Anthropic Messages API."""

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str | None = None,
        timeout: float | None = None,
    ) -> None:
        self.api_key = api_key or os.environ.get("ANTHROPIC_API_KEY", "")
        if not self.api_key:
            raise ValueError(
                "Anthropic API key required. Set ANTHROPIC_API_KEY env var or pass api_key=."
            )
        self.model = model or ANTHROPIC_MODEL
        super().__init__(
            base_url=_BASE_URL,
            headers={
                "x-api-key": self.api_key,
                "anthropic-version": "2023-06-01",
                "Content-Type": "application/json",
            },
            timeout=timeout or API_TIMEOUT,
        )

    def chat(
        self,
        messages: list[dict],
        model: Optional[str] = None,
        temperature: float = 0.2,
        max_tokens: int = 4096,
        response_format: Optional[dict] = None,
    ) -> LLMResponse:
        model = model or self.model

        # Anthropic Messages API uses a separate system param
        system_text = ""
        api_messages = []
        for msg in messages:
            if msg["role"] == "system":
                system_text = msg["content"]
            else:
                api_messages.append(msg)

        payload: dict = {
            "model": model,
            "messages": api_messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
        }
        if system_text:
            payload["system"] = system_text

        try:
            data = self._post("/messages", payload)
            # Anthropic returns content as a list of blocks
            content_blocks = data.get("content", [])
            text = ""
            for block in content_blocks:
                if block.get("type") == "text":
                    text += block.get("text", "")
            return LLMResponse(
                content=text,
                model=data.get("model", model),
                usage=data.get("usage"),
            )
        except ClientError:
            raise
        except (KeyError, IndexError, TypeError) as e:
            raise ClientError(f"Unexpected Anthropic response format: {e}")
