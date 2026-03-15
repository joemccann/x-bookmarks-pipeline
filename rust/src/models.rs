use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub text: String,
    pub author: String,
    pub date: String,
    pub image_urls: Vec<String>,
    pub tweet_url: String,
    pub chart_description: String,
}

impl Bookmark {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            author: String::new(),
            date: String::new(),
            image_urls: Vec::new(),
            tweet_url: String::new(),
            chart_description: String::new(),
        }
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    pub fn with_date(mut self, date: impl Into<String>) -> Self {
        self.date = date.into();
        self
    }

    pub fn with_images(mut self, image_urls: Vec<String>) -> Self {
        self.image_urls = image_urls;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationInput {
    pub tweet_id: String,
    pub text: String,
    pub image_urls: Vec<String>,
    pub strategy_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    pub tweet_id: String,
    pub is_finance: bool,
    pub confidence: f64,
    pub classification_source: String,
    pub has_trading_pattern: bool,
    pub has_visual_data: bool,
    pub category: String,
    pub subcategory: String,
    pub detected_topic: String,
    pub summary: String,
    pub raw_text: String,
    pub image_urls: Vec<String>,
}

impl ClassificationResult {
    pub fn new(tweet_id: impl Into<String>) -> Self {
        Self {
            tweet_id: tweet_id.into(),
            is_finance: false,
            confidence: 0.0,
            classification_source: "none".to_string(),
            has_trading_pattern: false,
            has_visual_data: false,
            category: "other".to_string(),
            subcategory: "general".to_string(),
            detected_topic: String::new(),
            summary: "".to_string(),
            raw_text: String::new(),
            image_urls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisInput {
    pub bookmark_id: String,
    pub image_urls: Vec<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysisOutput {
    pub raw_json: String,
    pub indicators: Vec<String>,
    pub notes: Vec<String>,
}

impl ImageAnalysisOutput {
    pub fn no_image_fallback(_url: &str) -> Self {
        Self {
            raw_json: String::new(),
            indicators: vec!["manual_review".to_string()],
            notes: vec!["No chart data available".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyPlan {
    pub tweet_id: String,
    pub script_type: String,
    pub title: String,
    pub ticker: String,
    pub direction: String,
    pub timeframe: String,
    pub indicators: Vec<String>,
    pub indicator_params: Value,
    pub entry_conditions: Vec<String>,
    pub exit_conditions: Vec<String>,
    pub risk_management: Value,
    pub key_levels: Value,
    pub pattern: Option<String>,
    pub visual_signals: Vec<String>,
    pub rationale: String,
    pub author: String,
    pub tweet_date: String,
    pub raw_tweet_text: String,
    pub chart_description: String,
}

impl StrategyPlan {
    pub fn fallback(tweet_id: impl Into<String>) -> Self {
        Self {
            tweet_id: tweet_id.into(),
            script_type: "strategy".to_string(),
            title: "Strategy".to_string(),
            ticker: "BTCUSDT".to_string(),
            direction: "long".to_string(),
            timeframe: "D".to_string(),
            indicators: Vec::new(),
            indicator_params: Value::Object(Default::default()),
            entry_conditions: Vec::new(),
            exit_conditions: Vec::new(),
            risk_management: Value::Object(Default::default()),
            key_levels: Value::Object(Default::default()),
            pattern: None,
            visual_signals: Vec::new(),
            rationale: "Fallback plan".to_string(),
            author: String::new(),
            tweet_date: String::new(),
            raw_tweet_text: String::new(),
            chart_description: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenInput {
    pub bookmark: Bookmark,
    pub plan: StrategyPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenOutput {
    pub pine_script: String,
    pub confidence: f64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn fail(&mut self, msg: impl Into<String>) {
        self.valid = false;
        self.errors.push(msg.into());
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalScript {
    pub bookmark_id: String,
    pub pine_script: String,
    pub meta_path: Option<String>,
    pub output_path: Option<String>,
    pub classification: ClassificationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub tweet_id: String,
    pub classification: Option<ClassificationResult>,
    pub plan: Option<StrategyPlan>,
    pub pine_script: String,
    pub validation: Option<ValidationResult>,
    pub output_path: Option<String>,
    pub meta_path: Option<String>,
    pub chart_data: Option<Value>,
    pub cached: bool,
    pub error: String,
}

impl PipelineResult {
    pub fn new(tweet_id: impl Into<String>) -> Self {
        Self {
            tweet_id: tweet_id.into(),
            classification: None,
            plan: None,
            pine_script: String::new(),
            validation: None,
            output_path: None,
            meta_path: None,
            chart_data: None,
            cached: false,
            error: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XBookmark {
    pub tweet_id: String,
    pub text: String,
    pub author: String,
    pub date: String,
    pub image_urls: Vec<String>,
    pub is_article: bool,
    pub tweet_url: String,
}

impl XBookmark {
    pub fn to_bookmark(&self) -> Bookmark {
        Bookmark {
            id: self.tweet_id.clone(),
            text: self.text.clone(),
            author: self.author.clone(),
            date: self.date.clone(),
            image_urls: self.image_urls.clone(),
            tweet_url: self.tweet_url.clone(),
            chart_description: String::new(),
        }
    }
}
