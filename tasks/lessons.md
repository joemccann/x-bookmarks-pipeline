# AGENTS task notes

## 2026-03-15

- Kept provider/runtime consistency by adding explicit `.env` bootstrap coverage in an integration test that runs `--clear-cache` to validate startup behavior without live API calls.
- Updated test matrix assertions to avoid model/environment drift by using scoped env restoration in unit tests.
- Fixed the env bootstrap integration path by resolving the built binary from `target/debug` as a fallback when `CARGO_BIN_EXE_*` is unavailable.
- Updated project docs (`README.md`, `CLAUDE.md`) to describe the root-only Rust project layout and usage.
- Adjusted migration tracking (`tasks/todo.md`) so completed/remaining milestones match verified code/test status.
- Fixed a daemon notification regression where the cycle runner returned early before entering the loop and skipped per-cycle completion hooks when cache-completed items were processed.
- Removed migration-centric wording from top-level README and replaced it with product-centric setup/usage documentation.
