#!/usr/bin/env python3
"""
OAuth 2.0 PKCE flow helper for X (Twitter) API.

Runs a local HTTP server, opens your browser for authorization,
captures the callback, exchanges the code for tokens, and prints
the access token to paste into .env.

Usage:
    python auth_pkce.py
"""

from __future__ import annotations

import base64
import hashlib
import http.server
import os
import secrets
import sys
import threading
import urllib.parse
import webbrowser

from dotenv import load_dotenv

load_dotenv()

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
    expires_in = tokens.get("expires_in", "?")

    print(f"\n{'=' * 60}")
    print("SUCCESS! Copy this into your .env file:\n")
    print(f"X_USER_ACCESS_TOKEN={access_token}")
    if refresh_token:
        print(f"\nRefresh token (save separately — needed to renew):")
        print(f"X_REFRESH_TOKEN={refresh_token}")
    print(f"\nToken expires in {expires_in} seconds.")
    print(f"{'=' * 60}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
