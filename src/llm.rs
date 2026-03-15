use crate::error::PipelineError;
use crate::parser;
use crate::models::{
    ClassificationInput, ClassificationResult, CodeGenInput, CodeGenOutput, ImageAnalysisInput,
    ImageAnalysisOutput,
};
use reqwest::{header::CONTENT_TYPE, Client};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type LlmFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, PipelineError>> + Send + 'a>>;

pub trait LLMProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn classify<'a>(
        &'a self,
        input: ClassificationInput,
        system_prompt: &'a str,
    ) -> LlmFuture<'a, ClassificationResult>;

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
        system_prompt: &'a str,
    ) -> LlmFuture<'a, ImageAnalysisOutput>;

    fn generate_code<'a>(&'a self, input: CodeGenInput, system_prompt: &'a str)
    -> LlmFuture<'a, CodeGenOutput>;

    fn complete_json<'a>(&'a self, system_prompt: &'a str, user_prompt: &'a str)
    -> LlmFuture<'a, String>;
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
    timeout_ms: u64,
    client: Client,
    flavor: ApiFlavor,
}

impl BaseLLMProvider {
    pub(crate) fn new(
        name: &'static str,
        endpoint: impl Into<String>,
        api_path: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        flavor: ApiFlavor,
        client: Client,
    ) -> Self {
        Self {
            name,
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            api_path: api_path.into().trim_start_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            timeout_ms: 120_000,
            client,
            flavor,
        }
    }

    fn endpoint_url(&self) -> String {
        format!("{}/{}", self.endpoint, self.api_path)
    }

    fn build_payload(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        with_json_object: bool,
        image_urls: Option<&[String]>,
    ) -> Value {
        match self.flavor {
            ApiFlavor::OpenAiCompatible => {
                let mut messages = Vec::new();
                messages.push(json!({"role": "system", "content": system_prompt}));

                if let Some(urls) = image_urls.filter(|urls| !urls.is_empty()) {
                    let mut content = Vec::<Value>::new();
                    content.push(json!({"type": "text", "text": user_prompt}));
                    for url in urls {
                        content.push(json!({
                            "type": "image_url",
                            "image_url": {"url": url}
                        }));
                    }
                    messages.push(json!({"role": "user", "content": content}));
                } else {
                    messages.push(json!({"role": "user", "content": user_prompt}));
                }

                let mut payload = json!({
                    "model": self.model,
                    "messages": messages,
                    "max_completion_tokens": if with_json_object { 768 } else { 3072 },
                });
                if with_json_object {
                    payload["response_format"] = json!({"type": "json_object"});
                }
                payload
            }
            ApiFlavor::Anthropic => {
                let user = if let Some(urls) = image_urls.filter(|urls| !urls.is_empty()) {
                    let mut blocks = Vec::<Value>::new();
                    blocks.push(json!({"type": "text", "text": user_prompt}));
                    for url in urls {
                        blocks.push(json!({
                            "type": "image",
                            "source": {
                                "type": "url",
                                "url": url,
                            },
                        }));
                    }
                    json!(blocks)
                } else {
                    json!(user_prompt)
                };
                json!({
                    "model": self.model,
                    "system": system_prompt,
                    "messages": [{
                        "role": "user",
                        "content": user,
                    }],
                    "max_tokens": 3072,
                })
            }
        }
    }

    fn extract_text_from_response(&self, body: &str) -> Result<String, PipelineError> {
        match self.flavor {
            ApiFlavor::OpenAiCompatible => {
                let envelope: OpenAIChatResponse = serde_json::from_str(body)?;
                let mut out = String::new();
                for choice in envelope.choices {
                    if let Some(content) = choice.message.content {
                        out.push_str(&content);
                    }
                }
                Ok(strip_markdown_fence(&out).to_string())
            }
            ApiFlavor::Anthropic => {
                let envelope: AnthropicChatResponse = serde_json::from_str(body)?;
                let mut out = String::new();
                for block in envelope.content {
                    if block.block_type == "text" {
                        if let Some(text) = block.text {
                            out.push_str(&text);
                        }
                    }
                }
                Ok(strip_markdown_fence(&out).to_string())
            }
        }
    }

    async fn request_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        with_json_object: bool,
        image_urls: Option<&[String]>,
    ) -> Result<String, PipelineError> {
        let payload = self.build_payload(system_prompt, user_prompt, with_json_object, image_urls);
        let request = match self.flavor {
            ApiFlavor::OpenAiCompatible => self
                .client
                .post(self.endpoint_url())
                .json(&payload)
                .bearer_auth(&self.api_key)
                .timeout(Duration::from_millis(self.timeout_ms)),
            ApiFlavor::Anthropic => self
                .client
                .post(self.endpoint_url())
                .json(&payload)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header(CONTENT_TYPE, "application/json")
                .timeout(Duration::from_millis(self.timeout_ms)),
        };

        let response = request.send().await.map_err(|err| PipelineError::Http {
            operation: format!("{}_request", self.name),
            details: err.to_string(),
        })?;

        let status = response.status();
        let body = response.text().await.map_err(|err| PipelineError::Http {
            operation: format!("{}_read", self.name),
            details: err.to_string(),
        })?;

        if !status.is_success() {
            return Err(PipelineError::Http {
                operation: format!("{}_status", self.name),
                details: format!("status={status}, body={body}"),
            });
        }

        self.extract_text_from_response(&body)
    }
}

impl LLMProvider for BaseLLMProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn classify<'a>(
        &'a self,
        input: ClassificationInput,
        system_prompt: &'a str,
    ) -> LlmFuture<'a, ClassificationResult> {
        let body = serde_json::to_string(&json!({
            "tweet_id": input.tweet_id,
            "text": input.text,
            "image_urls": input.image_urls,
            "strategy_hint": input.strategy_hint,
        }))
        .unwrap_or_else(|_| "{}".to_string());
        let tweet_id = input.tweet_id.clone();
        let raw_text = input.text.clone();
        let image_urls = input.image_urls.clone();

        Box::pin(async move {
            let text = self
                .request_text(system_prompt, &body, true, None)
                .await
                .map_err(PipelineError::from)?;
            let mut result = parse_with_fallback::<ClassificationResult>(
                &text,
                fallback_classification_result(&text, &raw_text, &tweet_id, &image_urls),
            )
            .map_err(PipelineError::from)?;
            if result.raw_text.is_empty() {
                result.raw_text = raw_text.clone();
            }
            if result.image_urls.is_empty() {
                result.image_urls = image_urls.clone();
            }
            result.classification_source = if result.classification_source.is_empty() {
                "provider".to_string()
            } else {
                result.classification_source
            };
            Ok(result)
        })
    }

    fn analyze_image<'a>(
        &'a self,
        input: ImageAnalysisInput,
        system_prompt: &'a str,
    ) -> LlmFuture<'a, ImageAnalysisOutput> {
        let body = serde_json::to_string(&json!({
            "bookmark_id": input.bookmark_id,
            "image_urls": input.image_urls,
            "context": input.context,
        }))
        .unwrap_or_else(|_| "{}".to_string());
        let image_urls = input.image_urls.clone();

        Box::pin(async move {
            let raw_json = self
                .request_text(system_prompt, &body, true, Some(&image_urls))
                .await
                .map_err(PipelineError::from)?;
            Ok(parse_image_output(&raw_json))
        })
    }

    fn generate_code<'a>(
        &'a self,
        input: CodeGenInput,
        system_prompt: &'a str,
    ) -> LlmFuture<'a, CodeGenOutput> {
        let body = serde_json::to_string(&input.plan).unwrap_or_else(|_| "{}".to_string());
        Box::pin(async move {
            let text = self
                .request_text(system_prompt, &body, false, None)
                .await
                .map_err(PipelineError::from)?;
            Ok(parse_generated_code(&text).unwrap_or_else(|| CodeGenOutput {
                pine_script: text,
                confidence: 0.5,
                notes: vec!["fallback code".to_string()],
            }))
        })
    }

    fn complete_json<'a>(&'a self, system_prompt: &'a str, user_prompt: &'a str) -> LlmFuture<'a, String> {
        Box::pin(async move {
            self.request_text(system_prompt, user_prompt, true, None)
                .await
                .map_err(PipelineError::from)
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
                "https://api.cerebras.ai/v1",
                "chat/completions",
                api_key,
                std::env::var("CEREBRAS_MODEL").unwrap_or_else(|_| "qwen-3-235b-a22b-instruct-2507".to_string()),
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
                "https://api.x.ai/v1",
                "chat/completions",
                api_key,
                std::env::var("XAI_MODEL").unwrap_or_else(|_| "grok-4-0709".to_string()),
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
                "https://api.anthropic.com/v1",
                "messages",
                api_key,
                std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-opus-4-6".to_string()),
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
                "https://api.openai.com/v1",
                "chat/completions",
                api_key,
                std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.4".to_string()),
                ApiFlavor::OpenAiCompatible,
                client,
            ),
        }
    }
}

macro_rules! delegate_provider {
    ($name:ident) => {
        impl LLMProvider for $name {
            fn name(&self) -> &'static str {
                self.inner.name()
            }

            fn classify<'a>(
                &'a self,
                input: ClassificationInput,
                system_prompt: &'a str,
            ) -> LlmFuture<'a, ClassificationResult> {
                self.inner.classify(input, system_prompt)
            }

            fn analyze_image<'a>(
                &'a self,
                input: ImageAnalysisInput,
                system_prompt: &'a str,
            ) -> LlmFuture<'a, ImageAnalysisOutput> {
                self.inner.analyze_image(input, system_prompt)
            }

            fn generate_code<'a>(
                &'a self,
                input: CodeGenInput,
                system_prompt: &'a str,
            ) -> LlmFuture<'a, CodeGenOutput> {
                self.inner.generate_code(input, system_prompt)
            }

            fn complete_json<'a>(
                &'a self,
                system_prompt: &'a str,
                user_prompt: &'a str,
            ) -> LlmFuture<'a, String> {
                self.inner.complete_json(system_prompt, user_prompt)
            }
        }
    };
}

delegate_provider!(CerebrasProvider);
delegate_provider!(XaiProvider);
delegate_provider!(ClaudeProvider);
delegate_provider!(OpenAIProvider);

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = env::var(key).ok();
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
            EnvVarGuard { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.clone() {
                Some(previous) => env::set_var(self.key, previous),
                None => env::remove_var(self.key),
            }
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap()
    }

    fn assert_provider_model_and_endpoint<T: std::fmt::Debug>(provider: T, expected_model: &str, expected_endpoint: &str) {
        let debug = format!("{provider:?}");
        assert!(debug.contains(expected_model), "missing model {expected_model} in {debug}");
        assert!(debug.contains(expected_endpoint), "missing endpoint {expected_endpoint} in {debug}");
    }

    #[test]
    fn cerebras_provider_prefers_env_model_and_fallback_on_missing() {
        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("CEREBRAS_MODEL", Some("qwen-test"));
            let provider = CerebrasProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "qwen-test",
                "https://api.cerebras.ai/v1",
            );
        }

        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("CEREBRAS_MODEL", None);
            let provider = CerebrasProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "qwen-3-235b-a22b-instruct-2507",
                "https://api.cerebras.ai/v1",
            );
        }
    }

    #[test]
    fn xai_provider_prefers_env_model_and_fallback_on_missing() {
        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("XAI_MODEL", Some("grok-test"));
            let provider = XaiProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(provider, "grok-test", "https://api.x.ai/v1");
        }

        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("XAI_MODEL", None);
            let provider = XaiProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "grok-4-0709",
                "https://api.x.ai/v1",
            );
        }
    }

    #[test]
    fn claude_provider_prefers_env_model_and_fallback_on_missing() {
        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("ANTHROPIC_MODEL", Some("claude-test"));
            let provider = ClaudeProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "claude-test",
                "https://api.anthropic.com/v1",
            );
        }

        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("ANTHROPIC_MODEL", None);
            let provider = ClaudeProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "claude-opus-4-6",
                "https://api.anthropic.com/v1",
            );
        }
    }

    #[test]
    fn openai_provider_prefers_env_model_and_fallback_on_missing() {
        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("OPENAI_MODEL", Some("gpt-test"));
            let provider = OpenAIProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "gpt-test",
                "https://api.openai.com/v1",
            );
        }

        {
            let _guard = lock_env();
            let _model = EnvVarGuard::set("OPENAI_MODEL", None);
            let provider = OpenAIProvider::new("k".to_string(), Client::new());
            assert_provider_model_and_endpoint(
                provider,
                "gpt-5.4",
                "https://api.openai.com/v1",
            );
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicChatResponse {
    content: Vec<AnthropicBlock>,
}

fn parse_generated_code(text: &str) -> Option<CodeGenOutput> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(output) = serde_json::from_str::<CodeGenOutput>(trimmed) {
        return Some(output);
    }

    if let Some(json_like) = extract_json_like(trimmed) {
        if let Ok(output) = serde_json::from_str::<CodeGenOutput>(json_like) {
            return Some(output);
        }
    }

    if let Some(code_block) = extract_code_block(trimmed) {
        return Some(CodeGenOutput {
            pine_script: ensure_version_prefix(code_block),
            confidence: 0.5,
            notes: vec!["raw_codeblock".to_string()],
        });
    }

    if trimmed.contains("//@version=6") || trimmed.contains("strategy(") || trimmed.contains("indicator(") {
        Some(CodeGenOutput {
            pine_script: ensure_version_prefix(trimmed),
            confidence: 0.5,
            notes: vec!["raw_code".to_string()],
        })
    } else {
        None
    }
}

fn parse_with_fallback<T>(text: &str, fallback: T) -> Result<T, PipelineError>
where
    T: DeserializeOwned,
{
    if let Ok(v) = serde_json::from_str::<T>(text) {
        return Ok(v);
    }
    if let Some(json_like) = extract_json_like(text) {
        if let Ok(v) = serde_json::from_str::<T>(json_like) {
            return Ok(v);
        }
    }
    Ok(fallback)
}

fn fallback_classification_result(
    provider_response: &str,
    raw_text: &str,
    tweet_id: &str,
    image_urls: &[String],
) -> ClassificationResult {
    if let Some(parsed) = parse_classification_response(provider_response, tweet_id) {
        return parsed;
    }

    let mut keywords = classify_finance_keywords(&provider_response);
    if !keywords {
        keywords = classify_finance_keywords(raw_text);
    }

    if keywords || likely_has_chart_mentions(raw_text) {
        ClassificationResult {
            tweet_id: tweet_id.to_string(),
            is_finance: true,
            confidence: 0.4,
            classification_source: "heuristic".to_string(),
            has_trading_pattern: likely_has_pattern_mentions(raw_text),
            has_visual_data: !image_urls.is_empty(),
            category: "finance".to_string(),
            subcategory: "general".to_string(),
            detected_topic: if keywords { "finance".to_string() } else { String::new() },
            summary: "fallback heuristic".to_string(),
            raw_text: raw_text.to_string(),
            image_urls: image_urls.to_vec(),
        }
    } else {
        ClassificationResult {
            tweet_id: tweet_id.to_string(),
            is_finance: false,
            confidence: 0.0,
            classification_source: "fallback".to_string(),
            has_trading_pattern: false,
            has_visual_data: false,
            category: "other".to_string(),
            subcategory: "general".to_string(),
            detected_topic: String::new(),
            summary: "fallback parse".to_string(),
            raw_text: raw_text.to_string(),
            image_urls: image_urls.to_vec(),
        }
    }
}

fn parse_classification_response(
    response: &str,
    tweet_id: &str,
) -> Option<ClassificationResult> {
    let value = parser::parse_chart_json(Some(response))?;
    let obj = value.as_object()?;

    let is_finance = obj.get("is_finance").and_then(|value| value.as_bool())?;
    let confidence = obj
        .get("confidence")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0);
    let has_trading_pattern = obj
        .get("has_trading_pattern")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let has_visual_data = obj
        .get("has_visual_data")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    Some(ClassificationResult {
        tweet_id: tweet_id.to_string(),
        is_finance,
        confidence,
        classification_source: "provider".to_string(),
        has_trading_pattern,
        has_visual_data,
        category: obj
            .get("category")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("other")
            .to_string(),
        subcategory: obj
            .get("subcategory")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("general")
            .to_string(),
        detected_topic: obj
            .get("detected_topic")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        summary: obj
            .get("summary")
            .and_then(|value| value.as_str())
            .unwrap_or("provider parse fallback")
            .to_string(),
        raw_text: String::new(),
        image_urls: Vec::new(),
    })
}

fn classify_finance_keywords(text: &str) -> bool {
    let normalized = text.to_lowercase();
    let tokens = [
        "btc",
        "eth",
        "crypto",
        "forex",
        "stock",
        "stocks",
        "spx",
        "dow",
        "nasdaq",
        "rsi",
        "macd",
        "sma",
        "ema",
        "candlestick",
        "candles",
        "chart",
        "support",
        "resistance",
        "breakout",
        "fibonacci",
        "options",
        "call",
        "put",
        "ticker",
        "entry",
        "exit",
        "leverage",
        "long",
        "short",
        "fut",
    ];
    if tokens.iter().any(|token| normalized.split_whitespace().any(|word| {
        let normalized_word = word
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_lowercase();
        normalized_word == *token
    })) {
        return true;
    }

    normalized.contains("$")
        && normalized
            .split_whitespace()
            .any(|token| token.starts_with('$') && token.len() > 2)
}

fn likely_has_pattern_mentions(text: &str) -> bool {
    let normalized = text.to_lowercase();
    normalized.contains("support")
        || normalized.contains("resistance")
        || normalized.contains("entry")
        || normalized.contains("exit")
        || normalized.contains("take-profit")
        || normalized.contains("stop loss")
        || normalized.contains("fibonacci")
}

fn likely_has_chart_mentions(text: &str) -> bool {
    let normalized = text.to_lowercase();
    normalized.contains("chart")
        || normalized.contains("candlestick")
        || normalized.contains("indicator")
        || normalized.contains("pattern")
        || normalized.contains("ema")
        || normalized.contains("sma")
        || normalized.contains("rsi")
        || normalized.contains("macd")
}

fn parse_image_output(raw: &str) -> ImageAnalysisOutput {
    let indicators = parse_string_array(raw, "indicators");
    let notes = parse_string_array(raw, "notes");
    let notes = if notes.is_empty() && indicators.is_empty() {
        vec!["No structured output parsed".to_string()]
    } else {
        notes
    };

    ImageAnalysisOutput {
        raw_json: raw.to_string(),
        indicators,
        notes,
    }
}

fn parse_string_array(raw: &str, key: &str) -> Vec<String> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.get(key).and_then(|value| value.as_array()).map(|items| {
            items
                .iter()
                .filter_map(|entry| entry.as_str().map(ToString::to_string))
                .collect()
        }))
        .unwrap_or_default()
}

fn extract_json_like(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then_some(&text[start..=end])
}

fn extract_code_block(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let rest = &text[start + 3..];
    let end = rest.find("```")?;
    let body = &rest[..end];
    Some(body.trim())
}

fn ensure_version_prefix(script: &str) -> String {
    let trimmed = script.trim();
    if trimmed.starts_with("//@version=6") {
        trimmed.to_string()
    } else if trimmed.contains("strategy(") || trimmed.contains("indicator(") {
        format!("//@version=6\n{trimmed}")
    } else {
        format!("//@version=6\nstrategy(\"Generated\", overlay=true)\n{trimmed}")
    }
}

fn strip_markdown_fence(value: &str) -> String {
    let trimmed = value.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut seen_open = false;
    let mut out = String::new();
    for line in trimmed.lines() {
        if line.starts_with("```") {
            if !seen_open {
                seen_open = true;
                continue;
            }
            break;
        }
        if seen_open {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}
