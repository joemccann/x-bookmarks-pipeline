use crate::cache::BookmarkCache;
use crate::classifier::FinanceClassifier;
use crate::config::AppConfig;
use crate::error::{PipelineError, PipelineResult};
use crate::generator::PineScriptGenerator;
use crate::llm::LLMProvider;
use crate::models::{
    Bookmark, ClassificationResult, PipelineResult as PipelineRunResult, StrategyPlan, ValidationResult,
};
use crate::notify::SmtpNotifier;
use crate::parser::{parse_chart_json, sanitize_path};
use crate::planner::StrategyPlanner;
use crate::validator::PineScriptValidator;
use crate::vision::VisionAnalyzer;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{fs, sync::Semaphore};

pub type OnMetaSaved = Arc<dyn Fn(&str) -> Result<(), PipelineError> + Send + Sync>;

#[derive(Clone)]
pub struct Pipeline {
    classifier: FinanceClassifier,
    planner: StrategyPlanner,
    generator: PineScriptGenerator,
    vision: VisionAnalyzer,
    validator: PineScriptValidator,
    cache: Option<BookmarkCache>,
    notifier: Option<Arc<SmtpNotifier>>,
    output_dir: String,
    cache_enabled: bool,
    vision_enabled: bool,
    max_parallel_workers: usize,
    on_meta_saved: Option<OnMetaSaved>,
}

impl Pipeline {
    pub fn new(
        classifier: Arc<dyn LLMProvider>,
        vision: Arc<dyn LLMProvider>,
        planner_client: Arc<dyn LLMProvider>,
        generator_client: Arc<dyn LLMProvider>,
        cache: Option<BookmarkCache>,
        notifier: Option<Arc<SmtpNotifier>>,
        config: &AppConfig,
    ) -> Self {
        Self {
            classifier: FinanceClassifier::new(classifier, vision.clone()),
            planner: StrategyPlanner::new(planner_client),
            generator: PineScriptGenerator::new(generator_client),
            vision: VisionAnalyzer::new(vision),
            validator: PineScriptValidator::new(),
            cache,
            notifier,
            output_dir: config.output_dir.clone(),
            cache_enabled: true,
            vision_enabled: true,
            max_parallel_workers: config.max_workers.max(1),
            on_meta_saved: None,
        }
    }

    pub fn with_cache(mut self, enabled: bool) -> Self {
        self.cache_enabled = enabled;
        self
    }

    pub fn with_vision(mut self, enabled: bool) -> Self {
        self.vision_enabled = enabled;
        self
    }

    pub fn with_on_meta_saved(mut self, hook: OnMetaSaved) -> Self {
        self.on_meta_saved = Some(hook);
        self
    }

    pub async fn run_batch(self: Arc<Self>, bookmarks: Vec<Bookmark>, save: bool) -> Vec<PipelineRunResult> {
        let semaphore = Arc::new(Semaphore::new(self.max_parallel_workers));
        let mut handles = Vec::with_capacity(bookmarks.len());

        for bookmark in bookmarks {
            let pipeline = Arc::clone(&self);
            let permit = semaphore.clone().acquire_owned().await;
            let handle = match permit {
                Ok(permit) => tokio::spawn(async move {
                    let _permit = permit;
                    pipeline.run(bookmark, save).await
                }),
                Err(err) => {
                    tokio::spawn(async move {
                        let mut failed = PipelineRunResult::new("unknown");
                        failed.error = format!("Failed acquiring worker slot: {err}");
                        failed
                    })
                }
            };
            handles.push(handle);
        }

        let mut out = Vec::with_capacity(handles.len());
        for handle in handles {
            if let Ok(result) = handle.await {
                out.push(result);
            }
        }
        out
    }

    pub async fn run(&self, mut bookmark: Bookmark, save: bool) -> PipelineRunResult {
        let mut result = PipelineRunResult::new(&bookmark.id);

        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                if let Ok(true) = cache.has_completed(&bookmark.id).await {
                    if let Ok(cached) = self.load_from_cache(&bookmark.id).await {
                        return cached;
                    }
                }
            }
        }

        let classification = match self
            .cached_or_classify(&bookmark.id, &mut bookmark.text, bookmark.image_urls.clone())
            .await
        {
            Ok(value) => value,
            Err(err) => {
                let mut validation = ValidationResult::new();
                validation.fail(format!("Classification failed: {err}"));
                result.validation = Some(validation);
                result.error = format!("Classification failed: {err}");
                return self.finalize(bookmark, result, save).await;
            }
        };

        let chart_data = self
            .maybe_run_vision(&bookmark, &classification)
            .await
            .unwrap_or(None);

        result.classification = Some(classification.clone());
        result.chart_data = chart_data.clone();

        if classification.is_finance {
            let plan = match self.plan_stage(&bookmark, &classification, chart_data.as_ref()).await {
                Ok(value) => value,
                Err(err) => {
                    let mut validation = ValidationResult::new();
                    validation.fail(format!("Planning failed: {err}"));
                    result.validation = Some(validation);
                    result.error = format!("Planning failed: {err}");
                    if save {
                        let _ = self.save_meta(&bookmark, &classification, chart_data.as_ref(), None).await;
                    }
                    return self.finalize(bookmark, result, save).await;
                }
            };

            let (script, validation) = match self.script_stage(&bookmark, &plan).await {
                Ok(value) => value,
                Err(err) => {
                    let mut validation = ValidationResult::new();
                    validation.fail(format!("Generation failed: {err}"));
                    result.validation = Some(validation);
                    result.error = format!("Generation failed: {err}");
                    if save {
                        let _ = self.save_meta(&bookmark, &classification, chart_data.as_ref(), Some(&plan)).await;
                    }
                    return self.finalize(bookmark, result, save).await;
                }
            };

            result.plan = Some(plan.clone());
            result.pine_script = script.clone();
            result.validation = Some(validation.clone());

            if save {
                let saved = self
                    .save_finance(&bookmark, &classification, &plan, &script, validation)
                    .await;
                result.output_path = saved.0;
                result.meta_path = saved.1;
            }
        } else if save {
            let _ = self.save_meta(&bookmark, &classification, chart_data.as_ref(), None).await;
            result.meta_path = Some(self.meta_path_for_bookmark(&bookmark, &classification));
        }

        self.finalize(bookmark, result, save).await
    }

    async fn cached_or_classify(
        &self,
        tweet_id: &str,
        text: &mut String,
        image_urls: Vec<String>,
    ) -> PipelineResult<ClassificationResult> {
        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                if let Ok(Some(classification)) = cache.get_classification(tweet_id).await {
                    return Ok(classification);
                }
            }
        }

        let classification = self
            .classifier
            .classify(tweet_id.to_string(), text.clone(), image_urls, None)
            .await?;

        *text = classification.raw_text.clone();

        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                let _ = cache.save_classification(tweet_id, &classification).await;
            }
        }

        Ok(classification)
    }

    async fn maybe_run_vision(
        &self,
        bookmark: &Bookmark,
        classification: &ClassificationResult,
    ) -> PipelineResult<Option<Value>> {
        if !self.vision_enabled {
            if !bookmark.chart_description.trim().is_empty() {
                return Ok(parse_chart_json(Some(&bookmark.chart_description)));
            }
            return Ok(None);
        }

        let should_analyze = (classification.is_finance || classification.has_visual_data)
            && !bookmark.image_urls.is_empty()
            && bookmark.chart_description.trim().is_empty();

        if !should_analyze {
            if !bookmark.chart_description.trim().is_empty() {
                return Ok(parse_chart_json(Some(&bookmark.chart_description)));
            }
            return Ok(None);
        }

        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                if let Ok(Some(chart_data)) = cache.get_chart_data(&bookmark.id).await {
                    return Ok(Some(chart_data));
                }
            }
        }

        let raw = self.vision.analyze(&bookmark.image_urls).await?;
        let chart_data = parse_chart_json(Some(&raw));

        if let Some(chart) = &chart_data {
            if self.cache_enabled {
                if let Some(cache) = &self.cache {
                    let _ = cache.save_chart_data(&bookmark.id, chart).await;
                }
            }
        }

        Ok(chart_data)
    }

    async fn plan_stage(
        &self,
        bookmark: &Bookmark,
        classification: &ClassificationResult,
        chart_data: Option<&Value>,
    ) -> PipelineResult<StrategyPlan> {
        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                if let Ok(Some(plan)) = cache.get_plan(&bookmark.id).await {
                    return Ok(plan);
                }
            }
        }

        let chart = chart_data
            .and_then(|value| serde_json::to_string(value).ok())
            .unwrap_or_default();
        let plan = self
            .planner
            .plan(classification, &bookmark.author, &bookmark.date, &chart)
            .await?;

        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                let _ = cache.save_plan(&bookmark.id, &plan).await;
            }
        }

        Ok(plan)
    }

    async fn script_stage(&self, bookmark: &Bookmark, plan: &StrategyPlan) -> PipelineResult<(String, ValidationResult)> {
        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                if let Ok(Some(script)) = cache.get_script(&bookmark.id).await {
                    let validation = self.load_validation_from_cache(&bookmark.id).await?;
                    return Ok((script, validation));
                }
            }
        }

        let code = self.generator.generate(bookmark, plan).await?;
        let validation = self.validator.validate(&code, &plan.script_type);

        if self.cache_enabled {
            if let Some(cache) = &self.cache {
                let _ = cache
                    .save_script(&bookmark.id, &code, validation.valid, &validation.errors)
                    .await;
            }
        }

        Ok((code, validation))
    }

    async fn load_validation_from_cache(&self, tweet_id: &str) -> PipelineResult<ValidationResult> {
        if !self.cache_enabled {
            return Ok(ValidationResult::new());
        }

        let cache = match &self.cache {
            Some(cache) => cache,
            None => return Ok(ValidationResult::new()),
        };

        let row = cache.get(tweet_id).await?;
        if let Some(raw_json) = row.as_ref().and_then(|r| r.validation_errors.clone()) {
            let errors: Vec<String> = serde_json::from_str(&raw_json).unwrap_or_default();
            let passed = row.and_then(|r| r.validation_passed).unwrap_or(1) != 0;
            let mut validation = ValidationResult::new();
            validation.valid = passed;
            validation.errors = errors;
            Ok(validation)
        } else {
            Ok(ValidationResult::new())
        }
    }

    async fn load_from_cache(&self, tweet_id: &str) -> PipelineResult<PipelineRunResult> {
        let cache = match &self.cache {
            Some(cache) => cache,
            None => {
                return Err(PipelineError::Cache {
                    details: "cache missing".to_string(),
                })
            }
        };

        let mut result = PipelineRunResult::new(tweet_id);
        result.cached = true;
        result.classification = cache.get_classification(tweet_id).await?;
        result.plan = cache.get_plan(tweet_id).await?;
        result.pine_script = cache.get_script(tweet_id).await?.unwrap_or_default();
        result.chart_data = cache.get_chart_data(tweet_id).await?;
        result.validation = Some(self.load_validation_from_cache(tweet_id).await.unwrap_or_default());
        result.meta_path = Some(self.meta_path_for_cached(tweet_id));
        result.output_path = Some(self.output_path_for_cached(tweet_id));
        Ok(result)
    }

    async fn finalize(
        &self,
        bookmark: Bookmark,
        result: PipelineRunResult,
        save: bool,
    ) -> PipelineRunResult {
        if save && result.error.is_empty() {
            if let Some(meta_path) = result.meta_path.as_deref() {
                if let Some(hook) = &self.on_meta_saved {
                    let _ = hook(meta_path);
                }
                if let Some(notifier) = &self.notifier {
                    let _ = notifier.send_meta_saved(meta_path).await;
                }
                if let Some(cache) = &self.cache {
                    if self.cache_enabled {
                        let _ = cache.mark_completed(&bookmark.id).await;
                    }
                }
            }
        }
        result
    }

    async fn save_meta(
        &self,
        bookmark: &Bookmark,
        classification: &ClassificationResult,
        chart_data: Option<&Value>,
        plan: Option<&StrategyPlan>,
    ) -> PipelineResult<()> {
        let out_dir = self.output_directory(classification);
        fs::create_dir_all(&out_dir).await?;

        let meta = serde_json::json!({
            "tweet_id": bookmark.id,
            "tweet_url": bookmark.tweet_url,
            "category": classification.category,
            "subcategory": classification.subcategory,
            "is_finance": classification.is_finance,
            "confidence": classification.confidence,
            "has_visual_data": classification.has_visual_data,
            "detected_topic": classification.detected_topic,
            "summary": classification.summary,
            "author": bookmark.author,
            "date": bookmark.date,
            "image_urls": bookmark.image_urls,
            "chart_data": chart_data,
            "script_type": plan.map(|p| p.script_type.clone()),
            "ticker": plan.map(|p| p.ticker.clone()),
            "direction": plan.map(|p| p.direction.clone()),
            "timeframe": plan.map(|p| p.timeframe.clone()),
            "indicators": plan.map(|p| p.indicators.clone()),
            "pattern": plan.and_then(|p| p.pattern.clone()),
            "key_levels": plan.map(|p| p.key_levels.clone()),
            "rationale": plan.map(|p| p.rationale.clone()),
        });

        let path = self.meta_path_for_bookmark(bookmark, classification);
        fs::write(&path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    async fn save_finance(
        &self,
        bookmark: &Bookmark,
        classification: &ClassificationResult,
        plan: &StrategyPlan,
        pine_script: &str,
        validation: ValidationResult,
    ) -> (Option<String>, Option<String>) {
        let out_dir = self.output_directory(classification);
        if fs::create_dir_all(&out_dir).await.is_err() {
            return (None, None);
        }

        let safe_author = sanitize_path(&plan.author);
        let safe_ticker = sanitize_path(&plan.ticker);
        let date = if bookmark.date.is_empty() {
            "undated"
        } else {
            &bookmark.date
        };
        let stem = format!("{safe_author}_{safe_ticker}_{date}");

        let mut pine_path = out_dir.clone();
        pine_path.push(format!("{stem}.pine"));

        if !pine_script.trim().is_empty() {
            if fs::write(&pine_path, pine_script).await.is_err() {
                return (None, None);
            }
        }

        let mut meta_path = pine_path.clone();
        meta_path.set_extension("meta.json");

        let meta = serde_json::json!({
            "tweet_id": bookmark.id,
            "tweet_url": bookmark.tweet_url,
            "category": classification.category,
            "subcategory": classification.subcategory,
            "is_finance": true,
            "script_type": plan.script_type,
            "author": plan.author,
            "date": bookmark.date,
            "ticker": plan.ticker,
            "direction": plan.direction,
            "timeframe": plan.timeframe,
            "indicators": plan.indicators,
            "pattern": plan.pattern,
            "key_levels": plan.key_levels,
            "rationale": plan.rationale,
            "validation_passed": validation.valid,
            "validation_errors": validation.errors,
            "validation_warnings": validation.warnings,
        });

        if fs::write(
            &meta_path,
            serde_json::to_string_pretty(&meta).unwrap_or_else(|_| "{}".to_string()),
        )
        .await
        .is_err()
        {
            return (
                Some(pine_path.to_string_lossy().to_string()),
                None,
            );
        }

        (
            Some(pine_path.to_string_lossy().to_string()),
            Some(meta_path.to_string_lossy().to_string()),
        )
    }

    fn output_directory(&self, classification: &ClassificationResult) -> PathBuf {
        let category = sanitize_path(&classification.category);
        let subcategory = sanitize_path(&classification.subcategory);
        PathBuf::from(&self.output_dir).join(category).join(subcategory)
    }

    fn output_path_for_cached(&self, tweet_id: &str) -> String {
        format!("{tweet_id}.pine")
    }

    fn meta_path_for_cached(&self, tweet_id: &str) -> String {
        format!("{tweet_id}.meta.json")
    }

    fn meta_path_for_bookmark(&self, bookmark: &Bookmark, classification: &ClassificationResult) -> String {
        let dir = self.output_directory(classification);
        let safe_author = sanitize_path(&bookmark.author);
        let date = if bookmark.date.is_empty() {
            "undated"
        } else {
            &bookmark.date
        };
        let stem = format!("{}_{}_{}", safe_author, date, &bookmark.id[..bookmark.id.len().min(8)]);
        dir.join(format!("{stem}.meta.json")).to_string_lossy().to_string()
    }
}
