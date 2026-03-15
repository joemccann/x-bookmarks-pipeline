use crate::error::PipelineError;
use crate::models::{
    ClassificationInput, ClassificationOutput, CodeGenInput, CodeGenOutput, ImageAnalysisInput,
    ImageAnalysisOutput,
};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{self, json, Value};
use std::{future::Future, pin::Pin, time::Duration};

pub type LlmFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, PipelineError>> + Send + 'a>>;

pub trait LLMProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput>;
    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
    ) -> LlmFuture<'a, ImageAnalysisOutput>;
    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ApiFlavor {
    OpenAiCompatible,
    Anthropic,
}

#[derive(Debug, Clone)]
pub struct BaseLLMProvider {
    name: &'static str,
    endpoint: String,
    api_path: String,
    api_key: String,
    model: String,
    timeout: Duration,
    client: Client,
    flavor: ApiFlavor,
}

impl BaseLLMProvider {
    pub(crate) fn new(
        name: &'static str,
        endpoint: String,
        api_path: String,
        api_key: String,
        model: String,
        flavor: ApiFlavor,
        client: Client,
    ) -> Self {
        Self {
            name,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_path: api_path.trim_start_matches('/').to_string(),
            api_key,
            model,
            timeout: Duration::from_secs(30),
            client,
            flavor,
        }
    }

    fn endpoint_url(&self) -> String {
        format!("{}/{}", self.endpoint, self.api_path)
    }

    fn request_text<'a>(&'a self, operation: &'a str, payload: Value) -> LlmFuture<'a, String> {
        let request_payload = self.build_request_payload(operation, payload);
        let client = self.client.clone();
        let url = self.endpoint_url();
        let provider = self.name.to_string();
        let timeout = self.timeout;
        let api_key = self.api_key.clone();
        let flavor = self.flavor;

        Box::pin(async move {
            let mut request = client.post(url).timeout(timeout).json(&request_payload);
            request = match flavor {
                ApiFlavor::OpenAiCompatible => request.bearer_auth(api_key),
                ApiFlavor::Anthropic => request
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01"),
            };

            let response = request
                .send()
                .await
                .map_err(|err| PipelineError::Http {
                    operation: format!("{provider}:{operation}"),
                    details: err.to_string(),
                })?;

            let status = response.status();
            let body = response.text().await?;
            if !status.is_success() {
                return Err(PipelineError::ProviderResponse {
                    provider: provider.clone(),
                    details: format!("status={status}, body={body}"),
                });
            }

            let content = match flavor {
                ApiFlavor::OpenAiCompatible => {
                    let envelope: OpenAiChatResponse = serde_json::from_str(&body).map_err(|err| {
                        PipelineError::ProviderResponse {
                            provider: provider.clone(),
                            details: format!("invalid OpenAI payload: {err}"),
                        }
                    })?;
                    let Some(choice) = envelope.choices.into_iter().next() else {
                        return Err(PipelineError::ProviderResponse {
                            provider: provider.clone(),
                            details: "empty model choices".to_string(),
                        });
                    };
                    choice.message.content.unwrap_or_default()
                }
                ApiFlavor::Anthropic => {
                    let envelope: AnthropicResponse = serde_json::from_str(&body).map_err(|err| {
                        PipelineError::ProviderResponse {
                            provider: provider.clone(),
                            details: format!("invalid Anthropic payload: {err}"),
                        }
                    })?;
                    let mut out = String::new();
                    for block in envelope.content {
                        if block.kind == "text" {
                            out.push_str(&block.text);
                        }
                    }
                    out
                }
            };

            Ok(normalize_llm_content(content))
        })
    }

    fn build_request_payload(&self, operation: &str, payload: Value) -> Value {
        let system_prompt = system_prompt(operation);
        let user_prompt = user_prompt(operation, &payload);

        match self.flavor {
            ApiFlavor::OpenAiCompatible => {
                let mut request = json!({
                    "model": self.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_prompt}
                    ],
                    "max_completion_tokens": 1536,
                });

                if operation != "generate_code" {
                    request["response_format"] = json!({"type": "json_object"});
                }
                request
            }
            ApiFlavor::Anthropic => json!({
                "model": self.model,
                "system": system_prompt,
                "messages": [
                    {"role": "user", "content": user_prompt}
                ],
                "max_tokens": 1536,
            }),
        }
    }
}

impl LLMProvider for BaseLLMProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput> {
        let payload = json!({ "bookmark": input.bookmark, "strategy_hint": input.strategy_hint });
        Box::pin(async move {
            let text = self.request_text("classify", payload).await?;
            parse_with_fallback(
                &text,
                || ClassificationOutput {
                    category: "other".to_string(),
                    confidence: 0.0,
                    rationale: "fallback parse".to_string(),
                },
            )
            .or_else(|_| {
                parse_with_fallback::<ClassificationOutput, _>(
                    &normalize_jsonish(&text),
                    || ClassificationOutput {
                        category: "other".to_string(),
                        confidence: 0.0,
                        rationale: "fallback parse".to_string(),
                    },
                )
            })
        })
    }

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
    ) -> LlmFuture<'a, ImageAnalysisOutput> {
        let payload = json!({
            "bookmark_id": input.bookmark_id,
            "image_url": input.image_url,
            "context": input.context,
        });
        Box::pin(async move {
            let text = self.request_text("analyze_image", payload).await?;
            parse_with_fallback(
                &text,
                || ImageAnalysisOutput {
                    signal: "unavailable".to_string(),
                    summary: "fallback parse".to_string(),
                    indicators: vec!["manual_review".to_string()],
                    confidence: 0.1,
                },
            )
            .or_else(|_| {
                parse_with_fallback::<ImageAnalysisOutput, _>(
                    &normalize_jsonish(&text),
                    || ImageAnalysisOutput {
                        signal: "unavailable".to_string(),
                        summary: "fallback parse".to_string(),
                        indicators: vec!["manual_review".to_string()],
                        confidence: 0.1,
                    },
                )
            })
        })
    }

    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput> {
        let payload = json!({
            "bookmark": input.bookmark,
            "classification": input.classification,
            "analysis": input.analysis,
            "requirements": input.additional_requirements,
        });
        Box::pin(async move {
            let text = self.request_text("generate_code", payload).await?;
            parse_generated_code(&text).or_else(|_| {
                let code = ensure_pinescript_wrappers(extract_script_like_block(&text));
                Ok(CodeGenOutput {
                    pine_script: code,
                    confidence: 0.25,
                    notes: vec!["fallback extraction".to_string()],
                })
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct CerebrasProvider {
    inner: BaseLLMProvider,
}

#[derive(Debug, Clone)]
pub struct XaiProvider {
    inner: BaseLLMProvider,
}

#[derive(Debug, Clone)]
pub struct ClaudeProvider {
    inner: BaseLLMProvider,
}

#[derive(Debug, Clone)]
pub struct OpenAIProvider {
    inner: BaseLLMProvider,
}

impl CerebrasProvider {
    pub fn new(api_key: String, client: Client) -> Self {
        Self {
            inner: BaseLLMProvider::new(
                "cerebras",
                "https://api.cerebras.ai/v1".to_string(),
                "chat/completions".to_string(),
                api_key,
                std::env::var("CEREBRAS_MODEL")
                    .unwrap_or_else(|_| "qwen-3-235b-a22b-instruct-2507".to_string()),
                ApiFlavor::OpenAiCompatible,
                client,
            ),
        }
    }
}

impl XaiProvider {
    pub fn new(api_key: String, client: Client) -> Self {
        Self {
            inner: BaseLLMProvider::new(
                "xai",
                "https://api.x.ai/v1".to_string(),
                "chat/completions".to_string(),
                api_key,
                std::env::var("XAI_MODEL")
                    .unwrap_or_else(|_| "grok-4-0709".to_string()),
                ApiFlavor::OpenAiCompatible,
                client,
            ),
        }
    }
}

impl ClaudeProvider {
    pub fn new(api_key: String, client: Client) -> Self {
        Self {
            inner: BaseLLMProvider::new(
                "claude",
                "https://api.anthropic.com/v1".to_string(),
                "messages".to_string(),
                api_key,
                std::env::var("ANTHROPIC_MODEL")
                    .unwrap_or_else(|_| "claude-opus-4-6".to_string()),
                ApiFlavor::Anthropic,
                client,
            ),
        }
    }
}

impl OpenAIProvider {
    pub fn new(api_key: String, client: Client) -> Self {
        Self {
            inner: BaseLLMProvider::new(
                "openai",
                "https://api.openai.com/v1".to_string(),
                "chat/completions".to_string(),
                api_key,
                std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
                ApiFlavor::OpenAiCompatible,
                client,
            ),
        }
    }
}

impl LLMProvider for CerebrasProvider {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput> {
        self.inner.classify(input)
    }

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
    ) -> LlmFuture<'a, ImageAnalysisOutput> {
        self.inner.analyze_image(input)
    }

    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput> {
        self.inner.generate_code(input)
    }
}

impl LLMProvider for XaiProvider {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput> {
        self.inner.classify(input)
    }

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
    ) -> LlmFuture<'a, ImageAnalysisOutput> {
        self.inner.analyze_image(input)
    }

    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput> {
        self.inner.generate_code(input)
    }
}

impl LLMProvider for ClaudeProvider {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput> {
        self.inner.classify(input)
    }

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
    ) -> LlmFuture<'a, ImageAnalysisOutput> {
        self.inner.analyze_image(input)
    }

    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput> {
        self.inner.generate_code(input)
    }
}

impl LLMProvider for OpenAIProvider {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn classify<'a>(&'a self, input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput> {
        self.inner.classify(input)
    }

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
    ) -> LlmFuture<'a, ImageAnalysisOutput> {
        self.inner.analyze_image(input)
    }

    fn generate_code<'a>(&'a self, input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput> {
        self.inner.generate_code(input)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

fn system_prompt(operation: &str) -> &'static str {
    match operation {
        "classify" => {
            "Return JSON only. Schema: {\"category\": string, \"confidence\": float, \"rationale\": string}."
        }
        "analyze_image" => {
            "Return JSON only. Schema: {\"signal\": string, \"summary\": string, \"indicators\": [string], \"confidence\": float}."
        }
        "generate_code" => {
            "Produce valid Pine Script v6. Return only JSON with fields: pine_script string, confidence float, notes array[string]. pine_script must start with //@version=6 and contain strategy() or indicator()."
        }
        _ => "Return JSON only.",
    }
}

fn user_prompt(operation: &str, payload: &Value) -> String {
    match operation {
        "classify" => {
            format!(
                "Classify this bookmark for finance/trading readiness.\nInput:\n{}",
                serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string())
            )
        }
        "analyze_image" => {
            format!(
                "Analyze this chart/visual context. Return technical trading signal, indicators, and confidence.\nInput:\n{}",
                serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string())
            )
        }
        "generate_code" => {
            format!(
                "Generate a Pine Script v6 strategy/indicator. Prefer concise, production-safe code.\nInput:\n{}",
                serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string())
            )
        }
        _ => serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string()),
    }
}

fn normalize_llm_content(raw: String) -> String {
    strip_markdown_fence(raw.trim()).to_string()
}

fn normalize_jsonish(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some((start, end)) = find_outer_json_object(trimmed) {
        trimmed[start..=end].to_string()
    } else {
        trimmed.to_string()
    }
}

fn strip_markdown_fence(value: &str) -> String {
    let trimmed = value.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut out = String::new();
    let mut started = false;
    for line in trimmed.lines() {
        if line.trim().starts_with("```") {
            if !started {
                started = true;
                continue;
            }
            break;
        }
        if started {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

fn parse_with_fallback<T, F>(text: &str, fallback: F) -> Result<T, PipelineError>
where
    T: DeserializeOwned + 'static,
    F: Fn() -> T,
{
    if let Ok(payload) = serde_json::from_str::<T>(text) {
        return Ok(payload);
    }

    if let Some((start, end)) = find_outer_json_object(text) {
        if let Ok(payload) = serde_json::from_str::<T>(&text[start..=end]) {
            return Ok(payload);
        }
    }

    Ok(fallback())
}

fn parse_generated_code(text: &str) -> Result<CodeGenOutput, PipelineError> {
    if let Ok(mut payload) = serde_json::from_str::<CodeGenOutput>(text) {
        payload.pine_script = ensure_pinescript_wrappers(payload.pine_script);
        return Ok(payload);
    }

    if let Some((start, end)) = find_outer_json_object(text) {
        if let Ok(mut payload) =
            serde_json::from_str::<CodeGenOutput>(&text[start..=end].to_string())
        {
            payload.pine_script = ensure_pinescript_wrappers(payload.pine_script);
            return Ok(payload);
        }
    }

    let code = extract_script_like_block(text);
    if code.trim().is_empty() {
        return Err(PipelineError::ProviderResponse {
            provider: "generate_code".to_string(),
            details: "Could not parse model output".to_string(),
        });
    }
    Ok(CodeGenOutput {
        pine_script: ensure_pinescript_wrappers(code),
        confidence: 0.2,
        notes: vec!["fallback extraction".to_string()],
    })
}

fn extract_script_like_block(text: &str) -> String {
    let candidate = strip_markdown_fence(text).trim().to_string();
    if candidate.starts_with("//@version") {
        return candidate;
    }
    if let Some(start) = candidate.find("//@version") {
        let maybe = candidate[start..].trim().to_string();
        if !maybe.is_empty() {
            return maybe;
        }
    }

    if candidate.is_empty() {
        return candidate;
    }
    if candidate.contains("strategy(") || candidate.contains("indicator(") || candidate.contains("study(") {
        return candidate;
    }

    format!("//@version=6\nstrategy(\"Generated\", overlay=true)\n{}\nplot(close)\n", candidate)
}

fn ensure_pinescript_wrappers(script: String) -> String {
    let mut out = script.trim().to_string();
    if !out.starts_with("//@version=6") {
        out = format!("//@version=6\n{}", out);
    }

    if !out.contains("strategy(") && !out.contains("indicator(") && !out.contains("study(") {
        out = format!("//@version=6\nstrategy(\"Generated\", overlay=true)\n{}\n", out);
    }
    out
}

fn find_outer_json_object(value: &str) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = None;

    for (idx, ch) in value.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        return start.map(|s| (s, idx));
                    }
                }
            }
            _ => {}
        }
    }

    None
}
