use crate::error::PipelineError;
use crate::llm::LLMProvider;
use crate::models::{ClassificationResult, StrategyPlan};
use crate::prompts::CLAUDE_PLANNING_SYSTEM_PROMPT;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct StrategyPlanner {
    client: Arc<dyn LLMProvider>,
}

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

        let raw = self
            .client
            .complete_json(CLAUDE_PLANNING_SYSTEM_PROMPT, &payload)
            .await
            .map_err(PipelineError::from)?;

        let parsed = serde_json::from_str::<Value>(&raw).map_err(|err| PipelineError::ProviderResponse {
            provider: "planner".to_string(),
            details: err.to_string(),
        })?;

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
