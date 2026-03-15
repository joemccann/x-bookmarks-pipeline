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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmFuture;
    use crate::models::{CodeGenInput, CodeGenOutput, ImageAnalysisOutput};
    use std::future::ready;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    #[derive(Clone, Default)]
    struct MockProvider {
        classify: Arc<AtomicBool>,
        vision: Arc<AtomicBool>,
        classify_payload: Option<ClassificationResult>,
        vision_payload: Option<ImageAnalysisOutput>,
        complete_payload: Option<String>,
    }

    impl MockProvider {
        fn with_classification(mut self, payload: ClassificationResult) -> Self {
            self.classify_payload = Some(payload);
            self
        }

        fn with_image_analysis(mut self, payload: ImageAnalysisOutput) -> Self {
            self.vision_payload = Some(payload);
            self
        }
    }

    impl LLMProvider for MockProvider {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn classify<'a>(
            &'a self,
            input: ClassificationInput,
            _system_prompt: &'a str,
        ) -> LlmFuture<'a, ClassificationResult> {
            let _ = input;
            self.classify.store(true, Ordering::SeqCst);
            let payload = self.classify_payload.clone().unwrap_or_else(|| ClassificationResult {
                tweet_id: "unknown".to_string(),
                is_finance: false,
                confidence: 0.0,
                classification_source: "mock".to_string(),
                has_trading_pattern: false,
                has_visual_data: false,
                category: "other".to_string(),
                subcategory: "general".to_string(),
                detected_topic: String::new(),
                summary: String::new(),
                raw_text: String::new(),
                image_urls: Vec::new(),
            });
            Box::pin(ready(Ok(payload)))
        }

        fn analyze_image<'a>(
            &'a self,
            input: ImageAnalysisInput,
            _system_prompt: &'a str,
        ) -> LlmFuture<'a, ImageAnalysisOutput> {
            let _ = input;
            self.vision.store(true, Ordering::SeqCst);
            let payload = self.vision_payload.clone().unwrap_or_else(|| ImageAnalysisOutput {
                raw_json: String::new(),
                indicators: Vec::new(),
                notes: Vec::new(),
            });
            Box::pin(ready(Ok(payload)))
        }

        fn generate_code<'a>(
            &'a self,
            _input: CodeGenInput,
            _system_prompt: &'a str,
        ) -> LlmFuture<'a, CodeGenOutput> {
            Box::pin(ready(Ok(CodeGenOutput {
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
            let text = self
                .complete_payload
                .clone()
                .unwrap_or_else(|| "{\"result\":\"noop\"}".to_string());
            Box::pin(ready(Ok(text)))
        }
    }

    #[test]
    fn classify_merges_image_result_when_visual_non_finance_requires_image() {
        let text_client = MockProvider::default().with_classification(ClassificationResult {
            tweet_id: "tweet-1".to_string(),
            is_finance: false,
            confidence: 0.2,
            classification_source: "text".to_string(),
            has_trading_pattern: false,
            has_visual_data: false,
            category: "other".to_string(),
            subcategory: "general".to_string(),
            detected_topic: "social".to_string(),
            summary: "not finance".to_string(),
            raw_text: "something".to_string(),
            image_urls: Vec::new(),
        });

        let called_image = Arc::new(AtomicBool::new(false));
        let mut vision_client = MockProvider::default()
            .with_image_analysis(ImageAnalysisOutput {
                raw_json: r#"{"is_finance":true,"confidence":0.95,"has_trading_pattern":true,"has_visual_data":true,"category":"finance","subcategory":"crypto","detected_topic":"BTC","summary":"detected"}"#.to_string(),
                indicators: Vec::new(),
                notes: Vec::new(),
            });
        vision_client.vision = called_image.clone();

        let classifier = FinanceClassifier::new(Arc::new(text_client), Arc::new(vision_client));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(async move {
            classifier
                .classify(
                    "tweet-1",
                    "sample text",
                    vec!["https://example.com/chart.png".to_string()],
                    None,
                )
                .await
        });

        let classification = result.unwrap();
        assert!(classification.is_finance);
        assert_eq!(classification.classification_source, "image");
        assert_eq!(classification.category, "finance");
        assert!(called_image.load(Ordering::SeqCst));
    }

    #[test]
    fn classify_skips_image_analysis_without_images() {
        let classify_provider = MockProvider::default().with_classification(ClassificationResult {
            tweet_id: "tweet-1".to_string(),
            is_finance: false,
            confidence: 0.2,
            classification_source: "text".to_string(),
            has_trading_pattern: false,
            has_visual_data: false,
            category: "other".to_string(),
            subcategory: "general".to_string(),
            detected_topic: "social".to_string(),
            summary: "not finance".to_string(),
            raw_text: "plain".to_string(),
            image_urls: Vec::new(),
        });

        let image_called = Arc::new(AtomicBool::new(false));
        let mut vision_provider = MockProvider::default()
            .with_image_analysis(ImageAnalysisOutput {
                raw_json: r#"{"is_finance":true}"#.to_string(),
                indicators: Vec::new(),
                notes: Vec::new(),
            });
        vision_provider.vision = image_called.clone();

        let classifier = FinanceClassifier::new(Arc::new(classify_provider), Arc::new(vision_provider));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(async move {
            classifier
                .classify("tweet-1", "text without charts", Vec::new(), None)
                .await
        });

        let classification = result.unwrap();
        assert!(!classification.is_finance);
        assert!(!image_called.load(Ordering::SeqCst));
    }

    #[test]
    fn parse_classification_like_parses_full_payload() {
        let raw = r#"{"is_finance":true,"confidence":0.91,"has_trading_pattern":true,"has_visual_data":true,"category":"finance","subcategory":"crypto","detected_topic":"BTC","summary":"looked chart"}"#;
        let parsed = parse_classification_like(raw).unwrap();
        assert!(parsed.is_finance);
        assert!(parsed.has_trading_pattern);
        assert_eq!(parsed.category, "finance");
        assert_eq!(parsed.subcategory, "crypto");
        assert_eq!(parsed.detected_topic, "BTC");
    }
}
