use crate::error::PipelineError;
use crate::llm::LLMProvider;
use crate::models::{ClassificationResult, StrategyPlan};
use crate::prompts::CLAUDE_PLANNING_SYSTEM_PROMPT;
use serde_json::{json, Value};
use std::sync::Arc;

/// Try to extract JSON from a response that may contain markdown code blocks or extra text
fn extract_json_from_response(raw: &str) -> Option<&str> {
    // Try to find JSON in ```json ... ``` blocks
    if let Some(start) = raw.find("```json") {
        let json_start = start + 7;
        if let Some(end) = raw[json_start..].find("```") {
            return Some(raw[json_start..json_start + end].trim());
        }
    }
    
    // Try to find JSON in ``` ... ``` blocks
    if let Some(start) = raw.find("```") {
        let block_start = start + 3;
        // Skip language identifier if present (e.g., ```json\n)
        let content_start = if let Some(newline) = raw[block_start..].find('\n') {
            block_start + newline + 1
        } else {
            block_start
        };
        if let Some(end) = raw[content_start..].find("```") {
            let potential_json = raw[content_start..content_start + end].trim();
            // Verify it looks like JSON
            if potential_json.starts_with('{') || potential_json.starts_with('[') {
                return Some(potential_json);
            }
        }
    }
    
    // Try to find a JSON object directly (starts with { and ends with })
    if let Some(start) = raw.find('{') {
        if let Some(end) = raw.rfind('}') {
            if end > start {
                return Some(&raw[start..=end]);
            }
        }
    }
    
    None
}

#[derive(Clone)]
pub struct StrategyPlanner {
    client: Arc<dyn LLMProvider>,
}

/// Maximum number of retry attempts for transient LLM failures
const MAX_RETRIES: u32 = 2;

impl StrategyPlanner {
    pub fn new(client: Arc<dyn LLMProvider>) -> Self {
        Self { client }
    }

    pub async fn plan(
        &self,
        classification: &ClassificationResult,
        author: &str,
        tweet_date: &str,
        chart_description: &str,
    ) -> Result<StrategyPlan, PipelineError> {
        let payload = json!({
            "tweet_id": classification.tweet_id,
            "classification": {
                "is_finance": classification.is_finance,
                "confidence": classification.confidence,
                "topic": classification.detected_topic,
                "category": classification.category,
                "subcategory": classification.subcategory,
                "has_trading_pattern": classification.has_trading_pattern,
                "has_visual_data": classification.has_visual_data,
                "summary": classification.summary,
                "text": classification.raw_text,
            },
            "author": author,
            "date": tweet_date,
            "chart": chart_description,
            "strategy_hint": classification.subcategory,
        })
        .to_string();

        // Retry logic for transient LLM failures (empty responses, etc.)
        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                eprintln!(
                    "[planner] retry attempt {}/{} for tweet {}",
                    attempt, MAX_RETRIES, classification.tweet_id
                );
                // Brief delay before retry
                tokio::time::sleep(tokio::time::Duration::from_millis(500 * attempt as u64)).await;
            }

            match self.try_plan(&payload, classification, author, tweet_date).await {
                Ok(plan) => return Ok(plan),
                Err(e) => {
                    // Only retry on empty/invalid response errors, not on HTTP errors
                    let is_retryable = matches!(&e, PipelineError::ProviderResponse { details, .. } 
                        if details.contains("empty response") || details.contains("Invalid JSON"));
                    
                    if is_retryable && attempt < MAX_RETRIES {
                        eprintln!("[planner] retryable error: {}", e);
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| PipelineError::ProviderResponse {
            provider: "planner".to_string(),
            details: "max retries exceeded".to_string(),
        }))
    }

    async fn try_plan(
        &self,
        payload: &str,
        classification: &ClassificationResult,
        author: &str,
        tweet_date: &str,
    ) -> Result<StrategyPlan, PipelineError> {
        let raw = self
            .client
            .complete_json(CLAUDE_PLANNING_SYSTEM_PROMPT, payload)
            .await
            .map_err(PipelineError::from)?;

        // Handle empty or whitespace-only responses
        let raw_trimmed = raw.trim();
        if raw_trimmed.is_empty() {
            return Err(PipelineError::ProviderResponse {
                provider: "planner".to_string(),
                details: "LLM returned empty response".to_string(),
            });
        }

        // Try to parse as JSON, with fallback for common LLM response issues
        let parsed = match serde_json::from_str::<Value>(raw_trimmed) {
            Ok(v) => v,
            Err(e) => {
                // Try to extract JSON from markdown code blocks if present
                if let Some(json_str) = extract_json_from_response(raw_trimmed) {
                    serde_json::from_str::<Value>(json_str).map_err(|inner_err| {
                        PipelineError::ProviderResponse {
                            provider: "planner".to_string(),
                            details: format!(
                                "Failed to parse extracted JSON: {}. Original error: {}. Raw response (first 500 chars): {}",
                                inner_err,
                                e,
                                &raw_trimmed[..raw_trimmed.len().min(500)]
                            ),
                        }
                    })?
                } else {
                    return Err(PipelineError::ProviderResponse {
                        provider: "planner".to_string(),
                        details: format!(
                            "Invalid JSON response: {}. Raw response (first 500 chars): {}",
                            e,
                            &raw_trimmed[..raw_trimmed.len().min(500)]
                        ),
                    });
                }
            }
        };

        Ok(build_plan_from_json(classification, author, tweet_date, &parsed))
    }
}

fn build_plan_from_json(
    classification: &ClassificationResult,
    author: &str,
    tweet_date: &str,
    value: &Value,
) -> StrategyPlan {
    StrategyPlan {
        tweet_id: classification.tweet_id.clone(),
        script_type: value
            .get("script_type")
            .and_then(Value::as_str)
            .unwrap_or("strategy")
            .to_string(),
        title: value.get("title").and_then(Value::as_str).unwrap_or("Strategy").to_string(),
        ticker: value.get("ticker").and_then(Value::as_str).unwrap_or("BTCUSDT").to_string(),
        direction: value.get("direction").and_then(Value::as_str).unwrap_or("long").to_string(),
        timeframe: value.get("timeframe").and_then(Value::as_str).unwrap_or("D").to_string(),
        indicators: values_to_vec(value.get("indicators")),
        indicator_params: value
            .get("indicator_params")
            .cloned()
            .unwrap_or_else(|| json!({})),
        entry_conditions: values_to_vec(value.get("entry_conditions")),
        exit_conditions: values_to_vec(value.get("exit_conditions")),
        risk_management: value
            .get("risk_management")
            .cloned()
            .unwrap_or_else(|| json!({})),
        key_levels: value.get("key_levels").cloned().unwrap_or_else(|| json!({})),
        pattern: value.get("pattern").and_then(Value::as_str).map(ToString::to_string),
        visual_signals: values_to_vec(value.get("visual_signals")),
        rationale: value
            .get("rationale")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        author: author.to_string(),
        tweet_date: tweet_date.to_string(),
        raw_tweet_text: classification.raw_text.clone(),
        chart_description: "".to_string(),
    }
}

fn values_to_vec(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|entry| entry.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmFuture;
    use crate::models::ClassificationResult;
    use std::future::ready;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct MockProvider {
        payload: String,
    }

    impl MockProvider {
        fn with_payload(mut self, payload: impl Into<String>) -> Self {
            self.payload = payload.into();
            self
        }
    }

    impl LLMProvider for MockProvider {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn classify<'a>(
            &'a self,
            _input: crate::models::ClassificationInput,
            _system_prompt: &'a str,
        ) -> LlmFuture<'a, crate::models::ClassificationResult> {
            Box::pin(ready(Ok(crate::models::ClassificationResult {
                tweet_id: "t".to_string(),
                is_finance: true,
                confidence: 0.1,
                classification_source: "mock".to_string(),
                has_trading_pattern: false,
                has_visual_data: false,
                category: "other".to_string(),
                subcategory: "general".to_string(),
                detected_topic: String::new(),
                summary: String::new(),
                raw_text: String::new(),
                image_urls: Vec::new(),
            })))
        }

        fn analyze_image<'a>(
            &'a self,
            _input: crate::models::ImageAnalysisInput,
            _system_prompt: &'a str,
        ) -> LlmFuture<'a, crate::models::ImageAnalysisOutput> {
            Box::pin(ready(Ok(crate::models::ImageAnalysisOutput {
                raw_json: String::new(),
                indicators: Vec::new(),
                notes: Vec::new(),
            })))
        }

        fn generate_code<'a>(
            &'a self,
            _input: crate::models::CodeGenInput,
            _system_prompt: &'a str,
        ) -> LlmFuture<'a, crate::models::CodeGenOutput> {
            Box::pin(ready(Ok(crate::models::CodeGenOutput {
                pine_script: String::new(),
                confidence: 0.0,
                notes: Vec::new(),
            })))
        }

        fn complete_json<'a>(
            &'a self,
            _system_prompt: &'a str,
            _user_prompt: &'a str,
        ) -> LlmFuture<'a, String> {
            Box::pin(ready(Ok(self.payload.clone())))
        }
    }

    fn sample_classification() -> ClassificationResult {
        ClassificationResult {
            tweet_id: "t1".to_string(),
            is_finance: true,
            confidence: 0.9,
            classification_source: "mock".to_string(),
            has_trading_pattern: true,
            has_visual_data: false,
            category: "finance".to_string(),
            subcategory: "crypto".to_string(),
            detected_topic: "BTC".to_string(),
            summary: "chart".to_string(),
            raw_text: "text".to_string(),
            image_urls: Vec::new(),
        }
    }

    #[test]
    fn build_plan_from_json_defaults_when_fields_missing() {
        let planner = StrategyPlanner::new(Arc::new(MockProvider::default().with_payload(r#"{}"#)));
        let classification = sample_classification();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let plan = runtime
            .block_on(async { planner.plan(&classification, "alice", "2026-03-14", "{}").await })
            .unwrap();

        assert_eq!(plan.ticker, "BTCUSDT");
        assert_eq!(plan.script_type, "strategy");
        assert_eq!(plan.author, "alice");
        assert_eq!(plan.tweet_date, "2026-03-14");
        assert_eq!(plan.raw_tweet_text, "text");
    }

    #[test]
    fn plan_fails_on_empty_response() {
        let planner = StrategyPlanner::new(Arc::new(MockProvider::default().with_payload("")));
        let classification = sample_classification();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime
            .block_on(async { planner.plan(&classification, "alice", "2026-03-14", "{}").await });

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty response"), "Expected empty response error, got: {}", err);
    }

    #[test]
    fn plan_extracts_json_from_markdown_block() {
        let payload = r#"Here's the plan:
```json
{"ticker": "ETHUSDT", "script_type": "indicator"}
```
"#;
        let planner = StrategyPlanner::new(Arc::new(MockProvider::default().with_payload(payload)));
        let classification = sample_classification();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let plan = runtime
            .block_on(async { planner.plan(&classification, "bob", "2026-03-15", "{}").await })
            .unwrap();

        assert_eq!(plan.ticker, "ETHUSDT");
        assert_eq!(plan.script_type, "indicator");
    }

    #[test]
    fn plan_extracts_json_from_plain_text_with_json() {
        let payload = r#"I'll create a strategy for you: {"ticker": "SOLUSDT", "direction": "short"} That should work."#;
        let planner = StrategyPlanner::new(Arc::new(MockProvider::default().with_payload(payload)));
        let classification = sample_classification();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let plan = runtime
            .block_on(async { planner.plan(&classification, "charlie", "2026-03-16", "{}").await })
            .unwrap();

        assert_eq!(plan.ticker, "SOLUSDT");
        assert_eq!(plan.direction, "short");
    }

    #[test]
    fn extract_json_from_response_finds_json_block() {
        let input = "Here's the plan:\n```json\n{\"test\": 1}\n```\nDone.";
        assert_eq!(extract_json_from_response(input), Some("{\"test\": 1}"));
    }

    #[test]
    fn extract_json_from_response_finds_raw_json() {
        let input = "Starting with {\"key\": \"value\"} and more text";
        assert_eq!(extract_json_from_response(input), Some("{\"key\": \"value\"}"));
    }

    #[test]
    fn extract_json_from_response_returns_none_for_no_json() {
        let input = "This is just plain text with no JSON";
        assert_eq!(extract_json_from_response(input), None);
    }
}
