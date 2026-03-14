"""
xAI Grok client — chat completions with vision support.
"""
from __future__ import annotations

import os
from typing import Optional

from .base_client import BaseClient, ClientError, LLMResponse
from src.config import XAI_MODEL, API_TIMEOUT

_BASE_URL = "https://api.x.ai/v1"


class XAIClient(BaseClient):
    """Thin httpx wrapper for xAI Grok API."""

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str | None = None,
        timeout: float | None = None,
    ) -> None:
        self.api_key = api_key or os.environ.get("XAI_API_KEY", "")
        if not self.api_key:
            raise ValueError(
                "xAI API key required. Set XAI_API_KEY env var or pass api_key=."
            )
        self.model = model or XAI_MODEL
        super().__init__(
            base_url=_BASE_URL,
            headers={
                "Authorization": f"Bearer {self.api_key}",
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
        payload: dict = {
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
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
            raise ClientError(f"Unexpected xAI response format: {e}")

    def chat_with_vision(
        self,
        system_prompt: str,
        text_prompt: str,
        image_urls: list[str],
        model: Optional[str] = None,
        temperature: float = 0.2,
        max_tokens: int = 4096,
        response_format: Optional[dict] = None,
    ) -> LLMResponse:
        """Send a vision request with text + images."""
        content: list[dict] = [{"type": "text", "text": text_prompt}]
        for url in image_urls:
            content.append({
                "type": "image_url",
                "image_url": {"url": url},
            })

        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": content},
        ]
        return self.chat(
            messages=messages,
            model=model,
            temperature=temperature,
            max_tokens=max_tokens,
            response_format=response_format,
        )
