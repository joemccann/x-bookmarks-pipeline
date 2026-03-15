use crate::error::PipelineError;
use crate::llm::LLMProvider;
use crate::models::{ClassificationInput, ClassificationResult, ImageAnalysisInput};
use crate::prompts::{FINANCE_IMAGE_CLASSIFICATION_PROMPT, FINANCE_TEXT_CLASSIFICATION_PROMPT};
use crate::parser;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct FinanceClassifier {
    text_client: Arc<dyn LLMProvider>,
    vision_client: Arc<dyn LLMProvider>,
}

impl FinanceClassifier {
    pub fn new(text_client: Arc<dyn LLMProvider>, vision_client: Arc<dyn LLMProvider>) -> Self {
        Self {
            text_client,
            vision_client,
        }
    }

    pub async fn classify(
        &self,
        tweet_id: impl Into<String>,
        text: impl Into<String>,
        image_urls: Vec<String>,
        strategy_hint: Option<String>,
    ) -> Result<ClassificationResult, PipelineError> {
        let tweet_id = tweet_id.into();
        let text = text.into();

        let mut classified = self
            .text_client
            .classify(
                ClassificationInput {
                    tweet_id: tweet_id.clone(),
                    text: text.clone(),
                    image_urls: image_urls.clone(),
                    strategy_hint,
                },
                FINANCE_TEXT_CLASSIFICATION_PROMPT,
            )
            .await
            .map_err(PipelineError::from)?;

        classified.tweet_id = tweet_id.clone();
        classified.raw_text = text.clone();
        classified.image_urls = image_urls.clone();
        classified.classification_source = if classified.is_finance {
            "text".to_string()
        } else {
            classified.classification_source.clone()
        };
        if classified.classification_source.is_empty() {
            classified.classification_source = "text".to_string();
        }

        if classified.is_finance || image_urls.is_empty() {
            return Ok(classified);
        }

        let vision_output = self
            .vision_client
            .analyze_image(
                ImageAnalysisInput {
                    bookmark_id: tweet_id.clone(),
                    image_urls: image_urls.clone(),
                    context: Some(FINANCE_IMAGE_CLASSIFICATION_PROMPT.to_string()),
                },
                "image classification",
            )
            .await
            .map_err(PipelineError::from)?;

        if vision_output.raw_json.is_empty() {
            return Ok(classified);
        }

        if let Ok(vision_result) = parse_classification_like(&vision_output.raw_json) {
            let mut merged = classified;
            merged.is_finance = vision_result.is_finance;
            merged.confidence = vision_result.confidence;
            merged.has_trading_pattern = vision_result.has_trading_pattern;
            merged.has_visual_data = vision_result.has_visual_data;
            merged.category = vision_result.category;
            merged.subcategory = vision_result.subcategory;
            merged.detected_topic = vision_result.detected_topic;
            merged.summary = vision_result.summary;
            merged.classification_source = "image".to_string();
            return Ok(merged);
        }

        Ok(classified)
    }
}

fn parse_classification_like(raw: &str) -> Result<ClassificationResult, PipelineError> {
    let json_value: Value = parser::parse_chart_json(Some(raw)).ok_or_else(|| PipelineError::ProviderResponse {
        provider: "vision".to_string(),
        details: "invalid json".to_string(),
    })?;

    if !json_value.is_object() {
        return Err(PipelineError::ProviderResponse {
            provider: "vision".to_string(),
            details: "non-object payload".to_string(),
        });
    }

    let obj = json_value.as_object().cloned().unwrap_or_default();
    let is_finance = obj
        .get("is_finance")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let confidence = obj.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let has_trading_pattern = obj
        .get("has_trading_pattern")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_visual_data = obj.get("has_visual_data").and_then(|v| v.as_bool()).unwrap_or(false);

    let category = obj
        .get("category")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "other".to_string());
    let subcategory = obj
        .get("subcategory")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "general".to_string());
    let detected_topic = obj
        .get("detected_topic")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_default();
    let summary = obj
        .get("summary")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_default();

    Ok(ClassificationResult {
        tweet_id: String::new(),
        is_finance,
        confidence,
        classification_source: "image".to_string(),
        has_trading_pattern,
        has_visual_data,
        category,
        subcategory,
        detected_topic,
        summary,
        raw_text: String::new(),
        image_urls: Vec::new(),
    })
}
