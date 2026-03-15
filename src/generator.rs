use crate::error::PipelineError;
use crate::llm::LLMProvider;
use crate::models::{Bookmark, CodeGenInput, CodeGenOutput, StrategyPlan};
use crate::prompts::GROK_PINESCRIPT_SYSTEM_PROMPT;
use serde_json::json;

#[derive(Clone)]
pub struct PineScriptGenerator {
    client: std::sync::Arc<dyn LLMProvider>,
}

impl PineScriptGenerator {
    pub fn new(client: std::sync::Arc<dyn LLMProvider>) -> Self {
        Self { client }
    }

    pub async fn generate(
        &self,
        bookmark: &Bookmark,
        plan: &StrategyPlan,
    ) -> Result<String, PipelineError> {
        let plan_payload = build_plan_payload(plan);
        let context = json!({
            "bookmark_id": bookmark.id,
            "author": bookmark.author,
            "date": bookmark.date,
            "text": bookmark.text,
            "image_urls": bookmark.image_urls,
            "chart_description": bookmark.chart_description,
        });

        let system = format!(
            "{GROK_PINESCRIPT_SYSTEM_PROMPT}\n\nBookmark:\n{}\n\nPlan:\n{}",
            serde_json::to_string_pretty(&context).unwrap_or_default(),
            plan_payload,
        );

        let output: CodeGenOutput = self
            .client
            .generate_code(
                CodeGenInput {
                    bookmark: bookmark.clone(),
                    plan: plan.clone(),
                },
                &system,
            )
            .await
            .map_err(PipelineError::from)?;

        Ok(normalize_script(&output.pine_script))
    }
}

fn build_plan_payload(plan: &StrategyPlan) -> String {
    let mut payload = serde_json::json!({
        "type": plan.script_type,
        "title": plan.title,
        "author": plan.author,
        "date": plan.tweet_date,
        "ticker": plan.ticker,
        "direction": plan.direction,
        "timeframe": plan.timeframe,
        "indicators": plan.indicators,
        "entry_conditions": plan.entry_conditions,
        "exit_conditions": plan.exit_conditions,
        "risk_management": plan.risk_management,
        "key_levels": plan.key_levels,
        "pattern": plan.pattern,
        "visual_signals": plan.visual_signals,
        "rationale": plan.rationale,
    });
    if let Some(obj) = plan.indicator_params.as_object() {
        payload["indicator_params"] = serde_json::Value::Object(obj.clone());
    }
    payload.to_string()
}

fn normalize_script(script: &str) -> String {
    let trimmed = script.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("//@version=6") {
        trimmed.to_string()
    } else if trimmed.contains("strategy(") || trimmed.contains("indicator(") {
        format!("//@version=6\n{trimmed}")
    } else {
        format!("//@version=6\nstrategy(\"Generated\", overlay=true)\n{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmFuture;
    use std::future::ready;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct MockProvider {
        output: String,
    }

    impl MockProvider {
        fn with_script(mut self, script: impl Into<String>) -> Self {
            self.output = script.into();
            self
        }
    }

    impl crate::llm::LLMProvider for MockProvider {
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
                pine_script: self.output.clone(),
                confidence: 0.75,
                notes: vec!["ok".to_string()],
            })))
        }

        fn complete_json<'a>(
            &'a self,
            _system_prompt: &'a str,
            _user_prompt: &'a str,
        ) -> LlmFuture<'a, String> {
            Box::pin(ready(Ok("{}".to_string())))
        }
    }

    #[tokio::test]
    async fn generator_normalizes_pine_script_missing_version() {
        let generator = PineScriptGenerator::new(Arc::new(
            MockProvider::default().with_script("strategy(\"Demo\", overlay=true)\nplot(1)"),
        ));

        let bookmark = crate::models::Bookmark {
            id: "1".to_string(),
            text: "test".to_string(),
            author: "alice".to_string(),
            date: "2026-03-14".to_string(),
            image_urls: Vec::new(),
            tweet_url: "https://x.com/test".to_string(),
            chart_description: String::new(),
        };
        let plan = crate::models::StrategyPlan {
            tweet_id: "1".to_string(),
            script_type: "strategy".to_string(),
            title: "Demo".to_string(),
            ticker: "BTCUSDT".to_string(),
            direction: "long".to_string(),
            timeframe: "D".to_string(),
            indicators: Vec::new(),
            indicator_params: serde_json::json!({}),
            entry_conditions: Vec::new(),
            exit_conditions: Vec::new(),
            risk_management: serde_json::json!({}),
            key_levels: serde_json::json!({}),
            pattern: None,
            visual_signals: Vec::new(),
            rationale: "r".to_string(),
            author: "alice".to_string(),
            tweet_date: "2026-03-14".to_string(),
            raw_tweet_text: "test".to_string(),
            chart_description: String::new(),
        };

        let script = generator.generate(&bookmark, &plan).await.unwrap();
        assert!(script.starts_with("//@version=6"));
        assert!(script.contains("strategy(\"Demo\""));
    }

    #[test]
    fn build_plan_payload_contains_plan_fields() {
        let plan = crate::models::StrategyPlan {
            tweet_id: "1".to_string(),
            script_type: "strategy".to_string(),
            title: "Demo".to_string(),
            ticker: "BTCUSDT".to_string(),
            direction: "long".to_string(),
            timeframe: "D".to_string(),
            indicators: vec!["ema".to_string()],
            indicator_params: serde_json::json!({"ema":9}),
            entry_conditions: vec!["entry>0".to_string()],
            exit_conditions: vec!["exit<0".to_string()],
            risk_management: serde_json::json!({"stop_loss":1.0}),
            key_levels: serde_json::json!({"levels":[1]}),
            pattern: Some("breakout".to_string()),
            visual_signals: vec!["v".to_string()],
            rationale: "r".to_string(),
            author: "alice".to_string(),
            tweet_date: "2026-03-14".to_string(),
            raw_tweet_text: "text".to_string(),
            chart_description: String::new(),
        };
        let payload: serde_json::Value = serde_json::from_str(&build_plan_payload(&plan)).unwrap();
        assert_eq!(payload["type"], "strategy");
        assert_eq!(payload["ticker"], "BTCUSDT");
        assert_eq!(payload["pattern"], "breakout");
        assert_eq!(payload["indicators"][0], "ema");
    }
}
