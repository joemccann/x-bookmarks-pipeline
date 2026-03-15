# Rust Migration Boilerplate: X Bookmarks Pipeline

This folder contains an initial Rust implementation of the core pipeline services:

- unified multi-provider LLM abstraction
- structured error model with `thiserror`
- async pipeline orchestration with `tokio::task`
- native email notifications using `lettre`
- thread-safe cache using `rusqlite` and `Arc<Mutex<Connection>>`

## Crate layout

- `rust/src/error.rs`  
  Central `PipelineError` enum and `PipelineResult<T>` alias.
- `rust/src/models.rs`  
  Pipeline domain models: `Bookmark`, `ClassificationOutput`, `ImageAnalysisOutput`, `CodeGenOutput`, `BookmarkMeta`, `FinalScript`.
- `rust/src/llm.rs`  
  `LLMProvider` trait and adapters for `Cerebras`, `xAI`, `Claude`, `OpenAI`.
- `rust/src/cache.rs`  
  SQLite cache wrapper with `Arc<Mutex<Connection>>`, `get`, `upsert`.
- `rust/src/notify.rs`  
  Native `SmtpNotifier` replacement for `notify.mjs` using `lettre`.
- `rust/src/orchestrator.rs`  
  `Pipeline` orchestrator with bounded parallelism + `on_meta_saved` hook.
- `rust/src/main.rs`  
  Bootstrap example showing single provider instances and dependency wiring.

## 1) Unified LLM interface

All providers implement one trait:

```rust
pub trait LLMProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput>;
    fn analyze_image<'a>(&'a self, input: ImageAnalysisInput) -> LlmFuture<'a, ImageAnalysisOutput>;
    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput>;
}
```

`BaseLLMProvider` centralizes request/response behavior, while `CerebrasProvider`, `XaiProvider`, `ClaudeProvider`, and `OpenAIProvider` are thin wrappers configuring endpoint/model/API-key and reusing the same interface.

## 2) Robust error handling

`PipelineError` includes:

- `ApiTimeout`: provider request timeout
- `SqliteLock`: lock contention
- `PineValidation`: Pine Script rejection (`//@version=6` checks and declaration checks)
- `Cache`, `CachePoisoned`, `TaskJoin`, `Email`, `ProviderResponse`

Conversion helpers (`From<rusqlite::Error>`, `From<reqwest::Error>`, etc.) keep internal callsites clean and keep orchestration layer explicit through `anyhow`.

## 3) Native notification module (`notify.rs`)

`SmtpNotifier::send_meta_saved(...)` is a small async wrapper around `lettre` blocking send.
It is used from the orchestrator’s `on_meta_saved` path to replace the old Node.js notification script.

## 4) Orchestrator and `on_meta_saved` hook (`orchestrator.rs`)

- `Pipeline` owns three shared provider clients:
  `classifier`, `image_analyzer`, `code_generator`.
- A `Semaphore` limits active workers.
- For each bookmark:
  1. check cache
  2. call `classify`
  3. call `analyze_image` (if image URL exists)
  4. call `generate_code`
  5. validate Pine Script
  6. upsert into cache
  7. run `on_meta_saved` callback
  8. optional `SmtpNotifier::send_meta_saved(...)`.

## 5) SQLite cache strategy

`BookmarkCache` uses:

```rust
conn: Arc<Mutex<Connection>>
```

All DB operations are serialized by the mutex, and executed in `tokio::task::spawn_blocking` to avoid blocking the async runtime.

## Running

```bash
cd rust
cargo build
cargo run
```

Set at least:
- `OPENAI_API_KEY`
- `XAI_API_KEY`
- `ANTHROPIC_API_KEY`
- `CEREBRAS_API_KEY`

to run the migration binary end-to-end.

## Think Step-by-Step: how data flows

1. `X Fetcher` yields `Vec<Bookmark>`.
2. `Pipeline::run` receives bookmarks and spawns worker tasks (bounded by semaphore permits).
3. Each worker calls `process_single`:
   1. Reads `Bookmark` from function input (ownership begins here).
   2. Passes ownership of `ClassificationInput` (contains `Bookmark`) to `classifier.classify`.
   3. Uses `classification` and borrowed `Bookmark` to optionally build `ImageAnalysisInput` and send to `image_analyzer.analyze_image`.
4. Ownership of `Bookmark` is moved into `CodeGenInput` when constructing `generate_code`.
5. `CodeGenInput` is consumed by `code_generator.generate_code`, producing `CodeGenOutput`.
6. `validate_pine_script` checks script safety and format.
7. `FinalScript` is assembled and inserted into SQLite via `cache.upsert`.
8. `on_meta_saved` hook runs using a borrowed `&FinalScript`; any side effects happen after persistence.
9. Worker returns `FinalScript` to caller; orchestrator collects all task outputs.
