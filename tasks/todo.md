# X Bookmarks Pipeline — Status

Last updated: 2026-03-19

## Completed

- [x] Rust-only pipeline: classify → vision → plan → generate → validate → output
- [x] 4 LLM providers: Cerebras, xAI, Claude, OpenAI
- [x] SQLite cache with per-stage persistence
- [x] Bounded worker concurrency (orchestrator)
- [x] Daemon mode with periodic polling
- [x] OAuth 2.0 PKCE reauth with automatic token refresh
- [x] CDP auto-consent: auto-clicks "Authorize app" via Chrome DevTools Protocol
- [x] Tab close after OAuth: closes **only** the exact callback tab (matched by full redirect URI base), never touches other localhost tabs
- [x] Refresh token rotation: persist rotated tokens on every refresh
- [x] Rich HTML email notifications (per-bookmark + cycle summaries)
- [x] Notifications only for new bookmarks (cached reruns silent)
- [x] Per-bookmark LLM cost tracking with `output/cost_report.md`
- [x] Real author usernames and tweet dates from X API (expansions)
- [x] X API cost optimizations: username caching, token validation caching, incremental fetch, budget limits
- [x] `--reset` command: clears all caches and output files
- [x] `--send-test-email-last <N>` command: send test email from last N saved bookmarks
- [x] Planner resilience: empty/invalid LLM responses detected early, JSON extracted from markdown blocks, retries up to 2× with backoff
- [x] 92 tests (84 lib + 7 main + 1 integration)

## Open Items

- [ ] Cache invalidation mechanism (version stamp or `--force-reclassify` flag)
- [ ] Playwright article scraper for t.co-only bookmarks (Tier 2)
- [ ] Gmail sending limit handling: batch/throttle notifications or switch to SES
