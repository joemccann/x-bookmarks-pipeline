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
