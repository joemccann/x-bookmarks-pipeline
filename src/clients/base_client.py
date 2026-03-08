"""
Base LLM client — shared httpx logic for all providers.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import httpx


class ClientError(Exception):
    """Raised when an LLM API call fails."""


@dataclass
class LLMResponse:
    """Normalized response from any LLM provider."""
    content: str
    model: str = ""
    usage: dict | None = None


class BaseClient:
    """Thin httpx wrapper shared by all provider clients."""

    def __init__(
        self,
        base_url: str,
        headers: dict[str, str],
        timeout: float = 120.0,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.headers = headers
        self.timeout = timeout

    def _post(self, path: str, payload: dict) -> dict:
        """POST JSON to the provider and return parsed response."""
        try:
            with httpx.Client(timeout=self.timeout) as client:
                resp = client.post(
                    f"{self.base_url}{path}",
                    headers=self.headers,
                    json=payload,
                )
                resp.raise_for_status()
            return resp.json()
        except httpx.ReadTimeout:
            raise ClientError("API read timeout — the model took too long to respond.")
        except httpx.ConnectTimeout:
            raise ClientError("API connection timeout — could not connect to server.")
        except httpx.HTTPStatusError as e:
            raise ClientError(
                f"API returned status {e.response.status_code}: {e.response.text}"
            )
        except httpx.RequestError as e:
            raise ClientError(f"Network error: {e}")

    def chat(
        self,
        messages: list[dict],
        model: str,
        temperature: float = 0.2,
        max_tokens: int = 4096,
        response_format: Optional[dict] = None,
    ) -> LLMResponse:
        """Send a chat completion request. Subclasses override for provider-specific payloads."""
        raise NotImplementedError
