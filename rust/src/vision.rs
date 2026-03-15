use crate::error::PipelineError;
use crate::llm::LLMProvider;
use crate::models::{ImageAnalysisInput, ImageAnalysisOutput};
use crate::prompts::VISION_ANALYSIS_PROMPT;

#[derive(Clone)]
pub struct VisionAnalyzer {
    client: std::sync::Arc<dyn LLMProvider>,
}

impl VisionAnalyzer {
    pub fn new(client: std::sync::Arc<dyn LLMProvider>) -> Self {
        Self { client }
    }

    pub async fn analyze(
        &self,
        image_urls: &[String],
    ) -> Result<String, PipelineError> {
        if image_urls.is_empty() {
            return Ok(String::new());
        }

        let out = self
            .client
            .analyze_image(
                ImageAnalysisInput {
                    bookmark_id: "vision".to_string(),
                    image_urls: image_urls.to_vec(),
                    context: Some(VISION_ANALYSIS_PROMPT.to_string()),
                },
                "Vision analysis for chart image(s)",
            )
            .await
            .map_err(PipelineError::from)?;

        Ok(out.raw_json)
    }

    pub async fn analyze_all(&self, image_urls: &[String]) -> Result<String, PipelineError> {
        if image_urls.is_empty() {
            return Ok(String::new());
        }

        let mut chunks: Vec<String> = Vec::new();
        for (idx, url) in image_urls.iter().enumerate() {
            if url.trim().is_empty() {
                continue;
            }
            let block = self
                .client
                .analyze_image(
                    ImageAnalysisInput {
                        bookmark_id: format!("vision-{idx}"),
                        image_urls: vec![url.clone()],
                        context: Some(VISION_ANALYSIS_PROMPT.to_string()),
                    },
                    "Vision analysis for chart image(s)",
                )
                .await
                .map_err(PipelineError::from)?;

            if !block.raw_json.is_empty() {
                chunks.push(block.raw_json);
            }
        }
        Ok(chunks.join("\n"))
    }
}

#[allow(dead_code)]
pub fn _unused_image_output_fields(_: &ImageAnalysisOutput) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmFuture;
    use std::future::ready;

    #[derive(Clone)]
    struct MockProvider {
        response: String,
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
                raw_json: self.response.clone(),
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
            Box::pin(ready(Ok("{}".to_string())))
        }
    }

    #[tokio::test]
    async fn analyze_returns_empty_for_no_images() {
        let analyzer = VisionAnalyzer::new(std::sync::Arc::new(MockProvider {
            response: r#"{"trend":"up"}"#.to_string(),
        }));
        let result = analyzer.analyze(&[]).await.unwrap();
        assert_eq!(result, String::new());
    }

    #[tokio::test]
    async fn analyze_all_skips_empty_entries() {
        let analyzer = VisionAnalyzer::new(std::sync::Arc::new(MockProvider {
            response: r#"{"trend":"up"}"#.to_string(),
        }));
        let result = analyzer
            .analyze_all(&["https://x.com/a.png".to_string(), "".to_string()])
            .await
            .unwrap();
        assert_eq!(result, r#"{"trend":"up"}"#.to_string());
    }
}
