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
- [x] Tab close after OAuth callback via CDP `/json/close`
- [x] Refresh token rotation: persist rotated tokens on every refresh
- [x] Rich HTML email notifications (per-bookmark + cycle summaries)
- [x] Notifications only for new bookmarks (cached reruns silent)
- [x] Per-bookmark LLM cost tracking with `output/cost_report.md`
- [x] Real author usernames and tweet dates from X API (expansions)
- [x] 75 tests (70 lib + 4 main + 1 integration)

## Open Items

- [ ] Cache invalidation mechanism (version stamp or `--force-reclassify` flag)
- [ ] Playwright article scraper for t.co-only bookmarks (Tier 2)
- [ ] Gmail sending limit handling: batch/throttle notifications or switch to SES

## Current Task

User request: send a test email based on the past 10 bookmarks without re-running the pipeline.

### Dependency Graph

- T1 -> T2
- T1 -> T3

### Task Plan

- [x] T1 `depends_on: []` Add a one-off CLI path that loads the most recent saved bookmark `.meta.json` files and renders a test notification email from them.
- [x] T2 `depends_on: [T1]` Execute the new command for the last 10 bookmarks and confirm the email was sent via configured SMTP settings.
- [x] T3 `depends_on: [T1]` Add targeted coverage where it fits and run the full Rust test suite.

## Review

- Added `--send-test-email-last <COUNT>` so the binary can send a test cycle email from the most recent saved `.meta.json` files without re-running X fetches or LLM stages.
- Reused the existing email renderer by exposing a render helper in `notify.rs`, then prefixed the subject with `[TEST]` for the one-off send path.
- Executed `cargo run -- --send-test-email-last 10`, which reported: `test email sent using 10 recent bookmarks from output`.
- Verification: `cargo test` passed (`84` lib tests, `7` main tests, `1` integration test). Non-fatal warning remains for `cfg(feature = "cdp_live_test")` in `src/browser.rs`.
