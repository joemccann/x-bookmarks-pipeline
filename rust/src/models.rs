use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub note: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationInput {
    pub bookmark: Bookmark,
    #[serde(default)]
    pub strategy_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationOutput {
    pub category: String,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisInput {
    pub bookmark_id: String,
    pub image_url: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisOutput {
    pub signal: String,
    pub summary: String,
    pub indicators: Vec<String>,
    pub confidence: f64,
}

impl ImageAnalysisOutput {
    pub fn no_image_fallback(url: &str) -> Self {
        Self {
            signal: "no_image".to_string(),
            summary: format!("No chart image supplied for {url}. Defaulting to text-only analysis."),
            indicators: vec!["manual_review".to_string()],
            confidence: 0.2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenInput {
    pub bookmark: Bookmark,
    pub classification: ClassificationOutput,
    pub analysis: ImageAnalysisOutput,
    pub additional_requirements: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenOutput {
    pub pine_script: String,
    pub confidence: f64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkMeta {
    pub bookmark_id: String,
    pub classification: ClassificationOutput,
    pub analysis: ImageAnalysisOutput,
    pub generated_at: i64,
    pub source_provider: String,
}

impl BookmarkMeta {
    pub fn new(
        bookmark_id: String,
        classification: ClassificationOutput,
        analysis: ImageAnalysisOutput,
        source_provider: String,
    ) -> Self {
        Self {
            bookmark_id,
            classification,
            analysis,
            generated_at: current_unix_timestamp(),
            source_provider,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalScript {
    pub bookmark_id: String,
    pub meta: BookmarkMeta,
    pub pine_script: String,
}

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
