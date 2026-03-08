"""
OpenAI ChatGPT client — chat completions via httpx.
"""
from __future__ import annotations

import os
from typing import Optional

from .base_client import BaseClient, ClientError, LLMResponse

_BASE_URL = "https://api.openai.com/v1"
_DEFAULT_MODEL = "gpt-5.4"


class OpenAIClient(BaseClient):
    """Thin httpx wrapper for OpenAI Chat Completions API."""

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str = _DEFAULT_MODEL,
        timeout: float = 120.0,
    ) -> None:
        self.api_key = api_key or os.environ.get("OPENAI_API_KEY", "")
        if not self.api_key:
            raise ValueError(
                "OpenAI API key required. Set OPENAI_API_KEY env var or pass api_key=."
            )
        self.model = model
        super().__init__(
            base_url=_BASE_URL,
            headers={
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": "application/json",
            },
            timeout=timeout,
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
        payload: dict = {
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_completion_tokens": max_tokens,
        }
        if response_format:
            payload["response_format"] = response_format

        try:
            data = self._post("/chat/completions", payload)
            return LLMResponse(
                content=data["choices"][0]["message"]["content"],
                model=data.get("model", model),
                usage=data.get("usage"),
            )
        except ClientError:
            raise
        except (KeyError, IndexError) as e:
            raise ClientError(f"Unexpected OpenAI response format: {e}")
