# Rust Migration Sprint Plan

Last updated: 2026-03-14

## Dependency graph

- **T1:** Rust project scaffolding bootstrap (no dependencies)
- **T2:** CLI argument model foundation (depends on T1)
- **T3:** Bookmark input abstraction (file/text/direct) (depends on T2)
- **T4:** X fetcher client interface (depends on T2)
- **T5:** Cache schema parity + migration layer (depends on T1)
- **T6:** Classification stage wiring (depends on T2, T5, T4)
- **T7:** Vision stage wiring + image selection heuristics (depends on T2, T5, T4)
- **T8:** Planner stage implementation (depends on T2, T5)
- **T9:** Generator stage implementation (depends on T2, T5, T8)
- **T10:** Validator stage + rule mapping (depends on T9)
- **T11:** Output persistence (meta + finance `.pine`) (depends on T5, T10)
- **T12:** `on_meta_saved` hook contract (depends on T11)
- **T13:** Notifier parity (`SmtpNotifier`) (depends on T1, T12)
- **T14:** Parallel runner integration (depends on T6, T7, T8, T9, T10, T11, T12)
- **T15:** Daemon/long-running cycle runner (depends on T2, T4, T14)
- **T16:** Rust test suite parity baseline (depends on T1..T15)
- **T17:** Feature smoke run + docs/ops validation (depends on T14, T13, T15, T16)

## Milestones (issue-sized)

### Milestone A — Foundations & Inputs (Issue 101)

- [x] **T1** Initialize/verify runtime scaffolding and provider initialization
  - `depends_on: []`
  - Acceptance: `cargo build` succeeds in clean checkout with only `cargo` + `.env` keys.
- [x] **T2** Implement a CLI/app command model that mirrors legacy modes (`fetch`, `text`, `file`, `--no-cache`, `--no-vision`, `--workers`, cache ops)
  - `depends_on: [T1]`
  - Acceptance: command parser supports required flags and resolves to deterministic config.
- [x] **T3** Define bookmark input adapters (`text`, `file`, `memory`, API fetch payload)
  - `depends_on: [T2]`
  - Acceptance: at least one test proves each input path maps into `Bookmark` domain model.

### Milestone B — Data Plane (Issue 102)

- [ ] **T5** Bring SQLite cache to parity (tables/columns for classification/vision/plan/script/validation/completion)
  - `depends_on: [T1]`
  - Acceptance: migration test verifies create/open/read/write for all cache stages.
- [x] **T4** Implement X fetcher + auth/token handling in Rust
  - `depends_on: [T2]`
  - Acceptance: fetcher test covers pagination + error-on-expired token.
- [ ] **T6** Implement classification stage via `LLMProvider::classify` with cached short-circuit
  - `depends_on: [T2, T5, T4]`
  - Acceptance: cached + fresh classification paths both covered by tests.
- [ ] **T7** Implement vision stage with cache-aware chart JSON synthesis
  - `depends_on: [T2, T5, T4]`
  - Acceptance: no-image/empty vision paths do not call analyzer; cached chart paths skip provider.
- [ ] **T8** Implement planning stage and structured plan persistence
  - `depends_on: [T2, T5]`
  - Acceptance: valid finance flow persists plan and rejects invalid plan payloads.
- [ ] **T9** Implement Pine Script generation stage (`generate_code`) from plan
  - `depends_on: [T2, T5, T8]`
  - Acceptance: output always contains Pine v6 script block and expected metadata fields.
- [ ] **T10** Integrate Pine Script validator with explicit failure diagnostics
  - `depends_on: [T9]`
  - Acceptance: failing scripts return structured validation errors and set `validation_passed=false`.
- [ ] **T11** Persist outputs (finance `.pine`, `.meta.json` for all, naming/location)
  - `depends_on: [T5, T10]`
  - Acceptance: parity check validates directory/file names and JSON fields.

### Milestone C — Orchestration & Side Effects (Issue 103)

- [ ] **T12** Enforce `on_meta_saved` hook contract and non-fatal error behavior
  - `depends_on: [T11]`
  - Acceptance: hook panic/Err must not fail final result return.
- [ ] **T13** Implement native notification parity using `SmtpNotifier`
  - `depends_on: [T1, T12]`
  - Acceptance: token-failure and bookmark-digest send paths available behind config gates.
- [ ] **T14** Wire orchestrator pipeline with bounded worker concurrency
  - `depends_on: [T6, T7, T8, T9, T10, T11, T12]`
  - Acceptance: run_batch processes N items with worker cap and preserves ordering policy.
- [ ] **T15** Implement periodic polling runner/daemon behavior
  - `depends_on: [T2, T4, T14]`
  - Acceptance: supports interval config, handles empty cycles, and supports graceful stop.

### Milestone D — Hardening & Release Confidence (Issue 104)

- [ ] **T16** Rebuild Rust test matrix to match prior behavior coverage
  - `depends_on: [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15]`
  - Acceptance: test categories for cache hit/miss, failure propagation, hook resilience, and end-to-end happy-path.
- [ ] **T17** Run final parity verification and documentation freeze
  - `depends_on: [T14, T13, T15, T16]`
  - Acceptance: `cargo test` passes, migration README and `CLAUDE.md` align, and runbook for deploy/recovery is complete.
