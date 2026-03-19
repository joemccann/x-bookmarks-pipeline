# Lessons Learned

## 2026-03-15

- Kept provider/runtime consistency by adding explicit `.env` bootstrap coverage in an integration test that runs `--clear-cache` to validate startup behavior without live API calls.
- Updated test matrix assertions to avoid model/environment drift by using scoped env restoration in unit tests.
- Fixed the env bootstrap integration path by resolving the built binary from `target/debug` as a fallback when `CARGO_BIN_EXE_*` is unavailable.
- Updated project docs (`README.md`, `CLAUDE.md`) to describe the root-only Rust project layout and usage.
- Adjusted migration tracking (`tasks/todo.md`) so completed/remaining milestones match verified code/test status.
- Fixed a daemon notification regression where the cycle runner returned early before entering the loop and skipped per-cycle completion hooks when cache-completed items were processed.
- Removed migration-centric wording from top-level README and replaced it with product-centric setup/usage documentation.
- Fixed broken `LLMProvider::classify` fallback path by removing a nonexistent helper call (`with_request_text_clues`) and restoring compile-time correctness.
- Hardened fallback classification by first attempting a tolerant JSON parse of provider responses, then falling back to token heuristics, preventing all bookmarks from defaulting to non-finance on malformed responses.
- Added notification-failure logging in `orchestrator` so email delivery regressions in daemon mode are visible without dropping processing flow.

## 2026-03-16

- **dotenvy parse failure is silent and fatal**: `dotenvy::from_filename(".env")` stops parsing the entire file on the first error. An unquoted SMTP password with spaces (`SMTP_PASS=word1 word2`) caused all env vars after it to be missing. Always quote values with spaces in `.env`.
- **macOS Chrome single-instance ignores launch flags**: Spawning Chrome directly with `--user-data-dir` and `--remote-debugging-port` on macOS when Chrome is already running does nothing — the existing instance opens a tab and ignores the flags. Use `open -na "App Name" --args ...` to force a new instance, or better: connect to the existing Chrome via CDP HTTP discovery.
- **CDP connects to existing Chrome, don't launch a new one**: The user's Chrome Debug app already has `--remote-debugging-port=9222`. Use the HTTP endpoint `http://127.0.0.1:9222/json/version` to discover the WebSocket URL. Don't assume `DevToolsActivePort` exists — Chrome deletes it after startup on some versions.
- **Stale cached classifications persist forever**: 99 bookmarks were classified as "other/general" by a broken fallback path. The cache has no invalidation mechanism — once `completed=1`, the pipeline never re-evaluates. Fix: clear stale entries with SQL, then re-run.
- **Notifications must guard on `!result.cached`**: The `finalize()` method was sending email notifications for every bookmark on every daemon cycle, including cached results. This caused hundreds of duplicate emails per day and hit Gmail's daily limit.

## 2026-03-17

- **X API refresh token rotation — persist on EVERY refresh**: X API uses refresh token rotation. Each `/oauth2/token` call returns a new refresh token and invalidates the previous one. The daemon had multiple code paths that called `refresh_x_access_token` (which updates `cfg.refresh_token` in memory) but then either passed `None` for the refresh token when persisting to `.env`, or didn't persist at all. This caused the `.env` to retain a stale/invalidated refresh token, forcing a full browser OAuth reauth on every daemon cycle. Fix: every code path that calls `refresh_x_access_token` must also call `persist_refreshed_access_token` with `Some(&cfg.refresh_token)`.
- **After browser reauth, reload `refresh_config` from env**: `start_interactive_reauth_flow` persists new tokens to `.env` and calls `env::set_var`, but the caller's `refresh_config: &mut Option<XRefreshConfig>` still holds the old values. Must do `*refresh_config = load_refresh_config()` after a successful browser reauth.
- **X API bookmarks need explicit field expansions**: The bookmarks endpoint returns only `id` and `text` by default. To get author and date, must request `tweet.fields=created_at,author_id`, `expansions=author_id`, and `user.fields=username,name`. Then build a user index from `includes.users` to resolve `author_id` to `@username`.
- **Don't re-run the entire pipeline to backfill metadata**: When existing meta files need a field update (like adding author/date), patch them directly with a script + API call instead of clearing the cache and re-processing through LLMs.
- **CDP click strategies should be specific, not greedy**: A "sole submit button" strategy clicked "Try again" on X's error pages instead of "Authorize app" on the consent page. Use `data-testid` and text-match strategies only.

## 2026-03-19

- **LLM planners can return empty strings**: `complete_json()` returned `""` causing `serde_json::from_str` to fail with "expected value at line 1 column 1". Always trim and check for empty before parsing. Include the raw response (first 500 chars) in the error for debuggability.
- **Extract JSON from LLM prose responses**: Models sometimes wrap JSON in markdown code fences (` ```json ... ``` `) or embed it in explanatory text. Try `from_str` first, then scan for ` ```json ` blocks, then scan for `{...}` spans. Retry up to 2× with exponential backoff for transient empty responses.
- **Never close browser tabs by URL substring**: `close_tabs_matching("localhost")` closed ALL localhost tabs including dev servers (`localhost:3000`). The correct approach is to pass the exact `redirect_uri` (e.g. `http://localhost:8080/callback`) and match on the base URL (strip query params from both sides, then do strict equality). This is immune to port confusion and path confusion.
- **OAuth callback close must be in BOTH code paths**: `close_oauth_callback_tab()` was only called in `start_interactive_reauth_flow` (the local-listener path) but not in the `--auth-code` manual exchange path. Both paths must close the tab after a successful token exchange.
- **Test tab-close logic live before shipping**: Use `cdp.mjs list` to snapshot tabs before and after, with a mix of real callback tabs and dev-server tabs open. Confirmed: exact-match on redirect URI base closes only the callback tab.
