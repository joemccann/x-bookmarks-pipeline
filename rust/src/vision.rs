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
