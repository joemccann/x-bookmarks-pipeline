"""
X Bookmark Fetcher — retrieves bookmarks from X (Twitter) API v2.

Authentication
--------------
The bookmarks endpoint requires OAuth 2.0 user-context authentication.
Required env var:

    X_USER_ACCESS_TOKEN  — OAuth 2.0 user access token with scopes:
                           bookmark.read  tweet.read  users.read
                           Generate via PKCE flow at https://console.x.com/

Optional:
    X_USER_ID            — your numeric X user ID
                           (auto-resolved via --x-username if omitted)
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import Optional

import httpx

_X_API_BASE = "https://api.twitter.com/2"


class FetchError(Exception):
    """Raised when fetching bookmarks from the X API fails."""


@dataclass
class XBookmark:
    """A single bookmark with all data needed by the pipeline."""

    tweet_id: str
    text: str
    author: str = ""
    date: str = ""          # ISO-8601 date portion: YYYY-MM-DD
    media_urls: list[str] = field(default_factory=list)


class XBookmarkFetcher:
    """
    Fetches bookmarks from X API v2 using OAuth 2.0 Bearer auth.

    Credentials are read from env vars (or .env):
        X_USER_ACCESS_TOKEN  — OAuth 2.0 user access token
        X_USER_ID            — numeric X user ID (optional if --x-username used)
    """

    def __init__(
        self,
        user_id: Optional[str] = None,
        timeout: float = 30.0,
    ) -> None:
        self.access_token = os.environ.get("X_USER_ACCESS_TOKEN", "")
        self.user_id = user_id or os.environ.get("X_USER_ID", "")
        self.timeout = timeout

        if not self.access_token:
            raise ValueError(
                "Missing required env var: X_USER_ACCESS_TOKEN\n"
                "Generate an OAuth 2.0 user access token with scopes\n"
                "  bookmark.read  tweet.read  users.read\n"
                "via the PKCE flow at https://console.x.com/"
            )

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def fetch(self, max_results: int = 100) -> list[XBookmark]:
        """
        Fetch up to *max_results* bookmarks, following pagination automatically.
        X API allows 1–100 per page.
        """
        if not self.user_id:
            raise ValueError(
                "user_id is required. Set X_USER_ID in .env or use "
                "resolve_user_id() / pass --x-username on the CLI."
            )

        bookmarks: list[XBookmark] = []
        next_token: Optional[str] = None
        page_size = min(max_results, 100)

        with httpx.Client(timeout=self.timeout) as client:
            while len(bookmarks) < max_results:
                batch = self._fetch_page(client, page_size, next_token)
                bookmarks.extend(batch["bookmarks"])
                next_token = batch.get("next_token")
                if not next_token:
                    break

        return bookmarks[:max_results]

    def resolve_user_id(self, username: str) -> str:
        """
        Resolve a @username to its numeric X user ID using the OAuth 2.0 token.
        Sets self.user_id as a side effect.
        """
        url = f"{_X_API_BASE}/users/by/username/{username}"
        try:
            with httpx.Client(timeout=self.timeout) as client:
                resp = client.get(url, headers=self._auth_headers())
                resp.raise_for_status()
        except httpx.HTTPStatusError as e:
            raise FetchError(
                f"X API returned status {e.response.status_code} resolving username '{username}'"
            )
        except httpx.RequestError as e:
            raise FetchError(f"Network error resolving username '{username}': {e}")
        data = resp.json()
        if "data" not in data:
            raise FetchError(f"Could not resolve username '{username}': {data}")
        self.user_id = data["data"]["id"]
        return self.user_id

    # ------------------------------------------------------------------
    # Private helpers
    # ------------------------------------------------------------------

    def _auth_headers(self) -> dict[str, str]:
        return {"Authorization": f"Bearer {self.access_token}"}

    def _fetch_page(
        self,
        client: httpx.Client,
        page_size: int,
        next_token: Optional[str],
    ) -> dict:
        """Fetch one page of bookmarks."""
        url = f"{_X_API_BASE}/users/{self.user_id}/bookmarks"
        params: dict = {
            "max_results": str(page_size),
            "tweet.fields": "text,author_id,created_at,attachments",
            "expansions": "author_id,attachments.media_keys",
            "user.fields": "username",
            "media.fields": "url,preview_image_url,type",
        }
        if next_token:
            params["pagination_token"] = next_token

        try:
            resp = client.get(url, headers=self._auth_headers(), params=params)
            resp.raise_for_status()
        except httpx.HTTPStatusError as e:
            raise FetchError(
                f"X API returned status {e.response.status_code}: {e.response.text}"
            )
        except httpx.TimeoutException as e:
            raise FetchError(f"X API request timeout: {e}")
        except httpx.RequestError as e:
            raise FetchError(f"Network error fetching bookmarks: {e}")
        payload = resp.json()

        # Build lookup maps from the includes block
        users_by_id: dict[str, str] = {}
        for u in payload.get("includes", {}).get("users", []):
            users_by_id[u["id"]] = u.get("username", "")

        media_by_key: dict[str, str] = {}
        for m in payload.get("includes", {}).get("media", []):
            img_url = m.get("url") or m.get("preview_image_url") or ""
            if img_url:
                media_by_key[m["media_key"]] = img_url

        bookmarks: list[XBookmark] = []
        for tweet in payload.get("data", []):
            author = users_by_id.get(tweet.get("author_id", ""), "")
            raw_date = tweet.get("created_at", "")
            date = raw_date[:10] if raw_date else ""

            media_keys = tweet.get("attachments", {}).get("media_keys", [])
            media_urls = [media_by_key[k] for k in media_keys if k in media_by_key]

            bookmarks.append(
                XBookmark(
                    tweet_id=tweet["id"],
                    text=tweet.get("text", ""),
                    author=author,
                    date=date,
                    media_urls=media_urls,
                )
            )

        return {
            "bookmarks": bookmarks,
            "next_token": payload.get("meta", {}).get("next_token"),
        }
