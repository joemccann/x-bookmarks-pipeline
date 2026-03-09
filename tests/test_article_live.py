#!/usr/bin/env python3
"""Live test: fetch bookmarks and find the @gemchange_ltd article to verify note_tweet + entities."""
from __future__ import annotations

from dotenv import load_dotenv
load_dotenv()

import json
import os
import httpx

_X_API_BASE = "https://api.twitter.com/2"

def main():
    token = os.environ.get("X_USER_ACCESS_TOKEN", "")
    user_id = os.environ.get("X_USER_ID", "")

    if not token or not user_id:
        print("ERROR: Set X_USER_ACCESS_TOKEN and X_USER_ID in .env")
        return

    headers = {"Authorization": f"Bearer {token}"}
    url = f"{_X_API_BASE}/users/{user_id}/bookmarks"
    params = {
        "max_results": "20",
        "tweet.fields": "text,author_id,created_at,attachments,note_tweet,entities",
        "expansions": "author_id,attachments.media_keys",
        "user.fields": "username",
        "media.fields": "url,preview_image_url,type",
    }

    print("Fetching bookmarks with note_tweet + entities fields...\n")

    with httpx.Client(timeout=30.0) as client:
        resp = client.get(url, headers=headers, params=params)
        if resp.status_code == 401:
            print(f"401 Unauthorized — token may be expired. Run: python auth_pkce.py")
            return
        resp.raise_for_status()

    payload = resp.json()

    # Build user lookup
    users = {}
    for u in payload.get("includes", {}).get("users", []):
        users[u["id"]] = u.get("username", "")

    # Find the gemchange_ltd article (or any article)
    target_found = False
    articles_found = 0

    for tweet in payload.get("data", []):
        author_id = tweet.get("author_id", "")
        author = users.get(author_id, "unknown")
        tweet_id = tweet["id"]
        text = tweet.get("text", "")
        note_tweet = tweet.get("note_tweet")
        entities = tweet.get("entities")

        # Check for note_tweet
        has_note = note_tweet is not None
        note_text = note_tweet.get("text", "") if note_tweet else ""

        # Check for article URL in entities
        expanded_urls = []
        article_url = ""
        if entities and "urls" in entities:
            for u in entities["urls"]:
                eu = u.get("expanded_url", "")
                if eu:
                    expanded_urls.append(eu)
                if "/articles/" in eu:
                    article_url = eu

        is_article = bool(article_url)
        if is_article:
            articles_found += 1

        # Check if this is the gemchange_ltd tweet
        is_target = "gemchange" in author.lower() or "quant desk" in text.lower() or "quant desk" in note_text.lower()

        if is_target or is_article:
            target_found = target_found or is_target
            print("=" * 80)
            print(f"TWEET ID:    {tweet_id}")
            print(f"AUTHOR:      @{author}")
            print(f"IS ARTICLE:  {is_article}")
            print(f"ARTICLE URL: {article_url or 'N/A'}")
            print(f"HAS NOTE:    {has_note}")
            print(f"TEXT FIELD:   {text[:200]}{'...' if len(text) > 200 else ''}")
            if note_text:
                print(f"NOTE TEXT:   {note_text[:500]}{'...' if len(note_text) > 500 else ''}")
            else:
                print(f"NOTE TEXT:   (empty/missing)")
            if expanded_urls:
                print(f"EXPANDED URLS: {json.dumps(expanded_urls, indent=2)}")
            print()

            # Dump full raw tweet JSON for inspection
            print("--- RAW TWEET JSON ---")
            print(json.dumps(tweet, indent=2)[:2000])
            print("=" * 80)
            print()

    if not target_found:
        print(f"\n@gemchange_ltd article not found in the last 20 bookmarks.")
        print(f"Articles found: {articles_found}")
        print(f"\nShowing ALL {len(payload.get('data', []))} bookmarks summary:\n")
        for tweet in payload.get("data", []):
            author = users.get(tweet.get("author_id", ""), "?")
            text = tweet.get("text", "")[:80]
            has_note = tweet.get("note_tweet") is not None
            urls = tweet.get("entities", {}).get("urls", [])
            has_article = any("/articles/" in u.get("expanded_url", "") for u in urls)
            print(f"  @{author:20s} note={has_note}  article={has_article}  {text}")

    print(f"\nTotal bookmarks fetched: {len(payload.get('data', []))}")
    print(f"Articles detected: {articles_found}")


if __name__ == "__main__":
    main()
