"""Tests for LLM API clients — mock httpx, verify headers/payloads/error handling."""
from __future__ import annotations

import pytest
from unittest.mock import patch, MagicMock

import httpx

from src.clients.base_client import BaseClient, ClientError, LLMResponse
from src.clients.xai_client import XAIClient
from src.clients.cerebras_client import CerebrasClient
from src.config import XAI_MODEL, CEREBRAS_MODEL, ANTHROPIC_MODEL, OPENAI_MODEL
from src.clients.anthropic_client import AnthropicClient
from src.clients.openai_client import OpenAIClient


# ---------------------------------------------------------------------------
# BaseClient
# ---------------------------------------------------------------------------

class TestBaseClient:
    def test_post_timeout_raises_client_error(self):
        client = BaseClient(base_url="https://example.com", headers={})
        with patch("httpx.Client") as mock_cls:
            mock = MagicMock()
            mock.__enter__ = MagicMock(return_value=mock)
            mock.__exit__ = MagicMock(return_value=False)
            mock.post.side_effect = httpx.ReadTimeout("timeout")
            mock_cls.return_value = mock

            with pytest.raises(ClientError, match="timeout"):
                client._post("/test", {})

    def test_post_http_error_raises_client_error(self):
        client = BaseClient(base_url="https://example.com", headers={})
        with patch("httpx.Client") as mock_cls:
            mock = MagicMock()
            mock.__enter__ = MagicMock(return_value=mock)
            mock.__exit__ = MagicMock(return_value=False)
            resp = MagicMock()
            resp.status_code = 500
            resp.text = "Internal Server Error"
            resp.raise_for_status.side_effect = httpx.HTTPStatusError(
                "500", request=MagicMock(), response=resp
            )
            mock.post.return_value = resp
            mock_cls.return_value = mock

            with pytest.raises(ClientError, match="500"):
                client._post("/test", {})


# ---------------------------------------------------------------------------
# XAIClient
# ---------------------------------------------------------------------------

class TestXAIClient:
    def test_missing_api_key_raises(self):
        with patch.dict("os.environ", {}, clear=True):
            with pytest.raises(ValueError, match="xAI API key"):
                XAIClient(api_key="")

    def test_chat_sends_correct_headers(self):
        client = XAIClient(api_key="test-xai-key")
        assert client.headers["Authorization"] == "Bearer test-xai-key"

    def test_chat_returns_llm_response(self):
        client = XAIClient(api_key="test-xai-key")
        with patch.object(client, "_post", return_value={
            "choices": [{"message": {"content": "hello"}}],
            "model": XAI_MODEL,
            "usage": {"total_tokens": 10},
        }):
            resp = client.chat(messages=[{"role": "user", "content": "hi"}])
            assert isinstance(resp, LLMResponse)
            assert resp.content == "hello"

    def test_chat_bad_response_raises(self):
        client = XAIClient(api_key="test-xai-key")
        with patch.object(client, "_post", return_value={"bad": "response"}):
            with pytest.raises(ClientError, match="Unexpected xAI response"):
                client.chat(messages=[{"role": "user", "content": "hi"}])

    def test_vision_builds_content_array(self):
        client = XAIClient(api_key="test-xai-key")
        with patch.object(client, "chat") as mock_chat:
            mock_chat.return_value = LLMResponse(content="chart analysis")
            client.chat_with_vision(
                system_prompt="analyze",
                text_prompt="what is this?",
                image_urls=["https://example.com/chart.png"],
            )
            call_args = mock_chat.call_args
            messages = call_args.kwargs.get("messages") or call_args[1].get("messages") or call_args[0][0]
            user_msg = [m for m in messages if m["role"] == "user"][0]
            assert isinstance(user_msg["content"], list)
            assert any(item["type"] == "image_url" for item in user_msg["content"])


# ---------------------------------------------------------------------------
# AnthropicClient
# ---------------------------------------------------------------------------

class TestAnthropicClient:
    def test_missing_api_key_raises(self):
        with patch.dict("os.environ", {}, clear=True):
            with pytest.raises(ValueError, match="Anthropic API key"):
                AnthropicClient(api_key="")

    def test_headers_include_api_key_and_version(self):
        client = AnthropicClient(api_key="test-anthropic-key")
        assert client.headers["x-api-key"] == "test-anthropic-key"
        assert "anthropic-version" in client.headers

    def test_chat_extracts_system_message(self):
        client = AnthropicClient(api_key="test-anthropic-key")
        with patch.object(client, "_post", return_value={
            "content": [{"type": "text", "text": "response"}],
            "model": ANTHROPIC_MODEL,
        }) as mock_post:
            client.chat(messages=[
                {"role": "system", "content": "be helpful"},
                {"role": "user", "content": "hi"},
            ])
            payload = mock_post.call_args[0][1]
            assert payload["system"] == "be helpful"
            assert len(payload["messages"]) == 1
            assert payload["messages"][0]["role"] == "user"

    def test_chat_returns_llm_response(self):
        client = AnthropicClient(api_key="test-anthropic-key")
        with patch.object(client, "_post", return_value={
            "content": [{"type": "text", "text": "hello from claude"}],
            "model": ANTHROPIC_MODEL,
        }):
            resp = client.chat(messages=[{"role": "user", "content": "hi"}])
            assert resp.content == "hello from claude"


# ---------------------------------------------------------------------------
# OpenAIClient
# ---------------------------------------------------------------------------

class TestOpenAIClient:
    def test_missing_api_key_raises(self):
        with patch.dict("os.environ", {}, clear=True):
            with pytest.raises(ValueError, match="OpenAI API key"):
                OpenAIClient(api_key="")

    def test_chat_sends_bearer_auth(self):
        client = OpenAIClient(api_key="test-openai-key")
        assert client.headers["Authorization"] == "Bearer test-openai-key"

    def test_chat_returns_llm_response(self):
        client = OpenAIClient(api_key="test-openai-key")
        with patch.object(client, "_post", return_value={
            "choices": [{"message": {"content": "pine script code"}}],
            "model": OPENAI_MODEL,
        }):
            resp = client.chat(messages=[{"role": "user", "content": "generate"}])
            assert resp.content == "pine script code"

    def test_chat_bad_response_raises(self):
        client = OpenAIClient(api_key="test-openai-key")
        with patch.object(client, "_post", return_value={"unexpected": True}):
            with pytest.raises(ClientError, match="Unexpected OpenAI response"):
                client.chat(messages=[{"role": "user", "content": "hi"}])


# ---------------------------------------------------------------------------
# CerebrasClient
# ---------------------------------------------------------------------------

class TestCerebrasClient:
    def test_missing_api_key_raises(self):
        with patch.dict("os.environ", {}, clear=True):
            with pytest.raises(ValueError, match="Cerebras API key"):
                CerebrasClient(api_key="")

    def test_chat_sends_bearer_auth(self):
        client = CerebrasClient(api_key="test-cerebras-key")
        assert client.headers["Authorization"] == "Bearer test-cerebras-key"

    def test_base_url_is_cerebras(self):
        client = CerebrasClient(api_key="test-cerebras-key")
        assert "cerebras.ai" in client.base_url

    def test_default_model_from_config(self):
        client = CerebrasClient(api_key="test-cerebras-key")
        assert client.model == CEREBRAS_MODEL

    def test_chat_returns_llm_response(self):
        client = CerebrasClient(api_key="test-cerebras-key")
        with patch.object(client, "_post", return_value={
            "choices": [{"message": {"content": "classified"}}],
            "model": CEREBRAS_MODEL,
            "usage": {"total_tokens": 50},
        }):
            resp = client.chat(messages=[{"role": "user", "content": "classify this"}])
            assert isinstance(resp, LLMResponse)
            assert resp.content == "classified"

    def test_chat_bad_response_raises(self):
        client = CerebrasClient(api_key="test-cerebras-key")
        with patch.object(client, "_post", return_value={"bad": "response"}):
            with pytest.raises(ClientError, match="Unexpected Cerebras response"):
                client.chat(messages=[{"role": "user", "content": "hi"}])
