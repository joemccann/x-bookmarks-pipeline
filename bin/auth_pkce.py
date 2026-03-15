#!/usr/bin/env python3
"""
OAuth 2.0 PKCE flow helper for X (Twitter) API.

Runs a local HTTP server, opens your browser for authorization,
captures the callback, exchanges the code for tokens, and prints
the access token to paste into .env.

Usage:
    python bin/auth_pkce.py
"""

from __future__ import annotations

import base64
import hashlib
import http.server
import json
import os
import re
import secrets
import sys
import threading
import time
import urllib.parse
import webbrowser
from pathlib import Path

from dotenv import load_dotenv

# Project root (repo root, one level up from bin/)
_PROJECT_DIR = Path(__file__).resolve().parent.parent

load_dotenv(_PROJECT_DIR / ".env")

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

CLIENT_ID = os.environ.get("X_CLIENT_ID", "")
CLIENT_SECRET = os.environ.get("X_CLIENT_SECRET", "")
REDIRECT_URI = "http://localhost:8080/callback"
SCOPES = "bookmark.read tweet.read users.read offline.access"

AUTHORIZE_URL = "https://x.com/i/oauth2/authorize"
TOKEN_URL = "https://api.x.com/2/oauth2/token"

if not CLIENT_ID:
    sys.exit("ERROR: X_CLIENT_ID not set in .env")


# ---------------------------------------------------------------------------
# PKCE helpers
# ---------------------------------------------------------------------------

def _generate_code_verifier() -> str:
    return secrets.token_urlsafe(64)[:128]


def _generate_code_challenge(verifier: str) -> str:
    digest = hashlib.sha256(verifier.encode("ascii")).digest()
    return base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")


# ---------------------------------------------------------------------------
# Local callback server
# ---------------------------------------------------------------------------

authorization_code: str | None = None
server_error: str | None = None
shutdown_event = threading.Event()


class CallbackHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        global authorization_code, server_error
        qs = urllib.parse.urlparse(self.path).query
        params = urllib.parse.parse_qs(qs)

        if "error" in params:
            server_error = params["error"][0]
            desc = params.get("error_description", [""])[0]
            self.send_response(400)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(
                f"<h2>Authorization failed</h2><p>{server_error}: {desc}</p>".encode()
            )
        elif "code" in params:
            authorization_code = params["code"][0]
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(
                b"<h2>Authorization successful!</h2>"
                b"<p>You can close this tab and return to your terminal.</p>"
            )
        else:
            self.send_response(400)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(b"<h2>Missing authorization code</h2>")

        shutdown_event.set()

    def log_message(self, format, *args) -> None:
        pass  # suppress request logs


# ---------------------------------------------------------------------------
# Token persistence helpers
# ---------------------------------------------------------------------------

_ENV_PATH = _PROJECT_DIR / ".env"
_TOKEN_STATE_PATH = _PROJECT_DIR / ".token_state.json"


def _write_tokens_to_env(access_token: str, refresh_token: str) -> None:
    """Update X_USER_ACCESS_TOKEN and X_REFRESH_TOKEN in .env in-place."""
    if not _ENV_PATH.exists():
        print(f"Note: {_ENV_PATH} not found — skipping auto-save to .env.")
        return
    content = _ENV_PATH.read_text()
    if re.search(r"^X_USER_ACCESS_TOKEN=", content, re.MULTILINE):
        content = re.sub(
            r"^X_USER_ACCESS_TOKEN=.*",
            f"X_USER_ACCESS_TOKEN={access_token}",
            content, flags=re.MULTILINE,
        )
    else:
        content += f"\nX_USER_ACCESS_TOKEN={access_token}\n"
    if refresh_token:
        if re.search(r"^X_REFRESH_TOKEN=", content, re.MULTILINE):
            content = re.sub(
                r"^X_REFRESH_TOKEN=.*",
                f"X_REFRESH_TOKEN={refresh_token}",
                content, flags=re.MULTILINE,
            )
        else:
            content += f"X_REFRESH_TOKEN={refresh_token}\n"
    _ENV_PATH.write_text(content)
    print(f"\u2713 Tokens written to {_ENV_PATH}")


def _write_token_state(access_token: str, refresh_token: str, expires_in: int) -> None:
    """Persist tokens and expiry epoch to .token_state.json."""
    state = {
        "access_token": access_token,
        "refresh_token": refresh_token,
        "expires_at": time.time() + expires_in if expires_in else 0,
    }
    _TOKEN_STATE_PATH.write_text(json.dumps(state, indent=2))
    print(f"\u2713 Token state saved to {_TOKEN_STATE_PATH}")


# ---------------------------------------------------------------------------
# Token exchange
# ---------------------------------------------------------------------------

def exchange_code(code: str, code_verifier: str) -> dict:
    import httpx

    data = {
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": REDIRECT_URI,
        "code_verifier": code_verifier,
        "client_id": CLIENT_ID,
    }

    # Native apps on X can use client_secret if provided
    auth = None
    if CLIENT_SECRET:
        auth = (CLIENT_ID, CLIENT_SECRET)

    resp = httpx.post(
        TOKEN_URL,
        data=data,
        auth=auth,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    resp.raise_for_status()
    return resp.json()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    code_verifier = _generate_code_verifier()
    code_challenge = _generate_code_challenge(code_verifier)
    state = secrets.token_urlsafe(32)

    auth_params = urllib.parse.urlencode({
        "response_type": "code",
        "client_id": CLIENT_ID,
        "redirect_uri": REDIRECT_URI,
        "scope": SCOPES,
        "state": state,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
    })
    auth_url = f"{AUTHORIZE_URL}?{auth_params}"

    # Start local server
    server = http.server.HTTPServer(("127.0.0.1", 8080), CallbackHandler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()

    print("Opening browser for X authorization...")
    print(f"If it doesn't open automatically, visit:\n\n  {auth_url}\n")
    webbrowser.open(auth_url)

    print("Waiting for callback...")
    shutdown_event.wait(timeout=120)
    server.shutdown()

    if server_error:
        print(f"\nERROR: {server_error}")
        return 1

    if not authorization_code:
        print("\nERROR: Timed out waiting for authorization callback.")
        return 1

    print("Exchanging authorization code for tokens...")
    tokens = exchange_code(authorization_code, code_verifier)

    access_token = tokens.get("access_token", "")
    refresh_token = tokens.get("refresh_token", "")
    expires_in = tokens.get("expires_in", 0)

    print(f"\n{'=' * 60}")
    print("SUCCESS!")
    print(f"  access_token : {access_token[:20]}...")
    if refresh_token:
        print(f"  refresh_token: {refresh_token[:20]}...")
    print(f"  expires_in   : {expires_in}s")
    print(f"{'=' * 60}\n")

    _write_tokens_to_env(access_token, refresh_token)
    _write_token_state(access_token, refresh_token, int(expires_in) if expires_in else 0)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
