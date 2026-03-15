use crate::cache::BookmarkCache;
use crate::error::{PipelineError, PipelineResult};
use crate::llm::LLMProvider;
use crate::models::{
    Bookmark, BookmarkMeta, ClassificationInput, CodeGenInput, FinalScript, ImageAnalysisInput,
};
use crate::notify::SmtpNotifier;
use anyhow::Error as AnyhowError;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub type OnMetaSaved = Arc<dyn Fn(&FinalScript) -> Result<(), AnyhowError> + Send + Sync>;

#[derive(Clone)]
pub struct Pipeline {
    pub classifier: Arc<dyn LLMProvider>,
    pub image_analyzer: Arc<dyn LLMProvider>,
    pub code_generator: Arc<dyn LLMProvider>,
    pub cache: BookmarkCache,
    pub notifier: Option<Arc<SmtpNotifier>>,
    pub max_parallel_workers: usize,
}

impl Pipeline {
    pub fn new(
        classifier: Arc<dyn LLMProvider>,
        image_analyzer: Arc<dyn LLMProvider>,
        code_generator: Arc<dyn LLMProvider>,
        cache: BookmarkCache,
        notifier: Option<Arc<SmtpNotifier>>,
        max_parallel_workers: usize,
    ) -> Self {
        Self {
            classifier,
            image_analyzer,
            code_generator,
            cache,
            notifier,
            max_parallel_workers: max_parallel_workers.max(1),
        }
    }

    pub async fn run(
        self: Arc<Self>,
        bookmarks: Vec<Bookmark>,
        on_meta_saved: Option<OnMetaSaved>,
    ) -> anyhow::Result<Vec<FinalScript>> {
        let permits = Arc::new(Semaphore::new(self.max_parallel_workers));
        let mut handles = Vec::with_capacity(bookmarks.len());

        for bookmark in bookmarks {
            let permit = permits.clone().acquire_owned().await?;
            let pipeline = Arc::clone(&self);
            let hook = on_meta_saved.clone();
            let notifier = self.notifier.clone();
            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let script = pipeline.process_single(bookmark).await?;
                if let Some(hook) = &hook {
                    if let Err(err) = hook(&script) {
                        eprintln!(
                            "on_meta_saved hook failed for {}: {}",
                            script.bookmark_id, err
                        );
                    }
                }
                if let Some(notifier) = &notifier {
                    notifier.send_meta_saved(&script).await?;
                }
                Ok::<FinalScript, PipelineError>(script)
            }));
        }

        let mut outputs = Vec::with_capacity(handles.len());
        for handle in handles {
            outputs.push(handle.await??);
        }
        Ok(outputs)
    }

    async fn process_single(&self, bookmark: Bookmark) -> PipelineResult<FinalScript> {
        if let Some(cached) = self.cache.get(&bookmark.id).await? {
            return Ok(cached);
        }

        let classification = self
            .classifier
            .classify(ClassificationInput {
                bookmark: bookmark.clone(),
                strategy_hint: Some("X-bookmark financial signal pipeline".to_string()),
            })
            .await?;

        let bookmark_id = bookmark.id.clone();

        let analysis = match &bookmark.image_url {
            Some(image_url) => self
                .image_analyzer
                .analyze_image(ImageAnalysisInput {
                    bookmark_id: bookmark.id.clone(),
                    image_url: image_url.clone(),
                    context: Some(classification.category.clone()),
                })
                .await?,
            None => crate::models::ImageAnalysisOutput::no_image_fallback(&bookmark.url),
        };

        let generated = self
            .code_generator
            .generate_code(CodeGenInput {
                bookmark: bookmark.clone(),
                classification: classification.clone(),
                analysis: analysis.clone(),
                additional_requirements: None,
            })
            .await?;

        self.validate_pine_script(&generated.pine_script)?;

        let final_script = FinalScript {
            bookmark_id: bookmark_id.clone(),
            meta: BookmarkMeta::new(
                bookmark_id,
                classification,
                analysis,
                self.code_generator.name().to_string(),
            ),
            pine_script: generated.pine_script,
        };

        self.cache.upsert(&final_script).await?;
        Ok(final_script)
    }

    fn validate_pine_script(&self, script: &str) -> PipelineResult<()> {
        if !script.contains("//@version=6") {
            return Err(PipelineError::PineValidation {
                details: "missing // @version=6 directive".to_string(),
            });
        }
        if !script.contains("strategy(")
            && !script.contains("study(")
            && !script.contains("indicator(")
        {
            return Err(PipelineError::PineValidation {
                details: "script has no strategy(), study(), or indicator() declaration".to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        Bookmark, BookmarkMeta, ClassificationOutput, CodeGenOutput, CodeGenInput, ImageAnalysisOutput,
        ImageAnalysisInput,
    };
    use anyhow::anyhow;
    use crate::llm::{LLMProvider, LlmFuture};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone)]
    struct StubProvider {
        classify: ClassificationOutput,
        analysis: ImageAnalysisOutput,
        code: CodeGenOutput,
        name: &'static str,
        classify_calls: Option<Arc<AtomicUsize>>,
        analyze_calls: Option<Arc<AtomicUsize>>,
        generate_calls: Option<Arc<AtomicUsize>>,
    }

    impl StubProvider {
        fn new(name: &'static str, category: &str) -> Self {
            Self {
                name,
                classify: ClassificationOutput {
                    category: category.to_string(),
                    confidence: 0.9,
                    rationale: "stub".to_string(),
                },
                analysis: ImageAnalysisOutput {
                    signal: "up".to_string(),
                    summary: "stub analysis".to_string(),
                    indicators: vec!["ma".to_string()],
                    confidence: 0.8,
                },
                code: CodeGenOutput {
                    pine_script: "//@version=6\nstrategy(\"stub\")".to_string(),
                    confidence: 0.95,
                    notes: vec!["ok".to_string()],
                },
                classify_calls: None,
                analyze_calls: None,
                generate_calls: None,
            }
        }

        fn with_counters(
            name: &'static str,
            category: &str,
            classify_calls: Arc<AtomicUsize>,
            analyze_calls: Arc<AtomicUsize>,
            generate_calls: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                name,
                classify: ClassificationOutput {
                    category: category.to_string(),
                    confidence: 0.9,
                    rationale: "stub".to_string(),
                },
                analysis: ImageAnalysisOutput {
                    signal: "up".to_string(),
                    summary: "stub analysis".to_string(),
                    indicators: vec!["ma".to_string()],
                    confidence: 0.8,
                },
                code: CodeGenOutput {
                    pine_script: "//@version=6\nstrategy(\"stub\")".to_string(),
                    confidence: 0.95,
                    notes: vec!["ok".to_string()],
                },
                classify_calls: Some(classify_calls),
                analyze_calls: Some(analyze_calls),
                generate_calls: Some(generate_calls),
            }
        }
    }

    impl LLMProvider for StubProvider {
        fn name(&self) -> &'static str {
            self.name
        }

        fn classify<'a>(&'a self, _input: ClassificationInput) -> LlmFuture<'a, ClassificationOutput> {
            if let Some(calls) = &self.classify_calls {
                calls.fetch_add(1, Ordering::SeqCst);
            }
            Box::pin(async move { Ok(self.classify.clone()) })
        }

        fn analyze_image<'a>(
            &'a self,
            _input: ImageAnalysisInput,
        ) -> LlmFuture<'a, ImageAnalysisOutput> {
            if let Some(calls) = &self.analyze_calls {
                calls.fetch_add(1, Ordering::SeqCst);
            }
            Box::pin(async move { Ok(self.analysis.clone()) })
        }

        fn generate_code<'a>(&'a self, _input: CodeGenInput) -> LlmFuture<'a, CodeGenOutput> {
            if let Some(calls) = &self.generate_calls {
                calls.fetch_add(1, Ordering::SeqCst);
            }
            Box::pin(async move { Ok(self.code.clone()) })
        }
    }

    fn stub_bookmark(id: &str) -> Bookmark {
        Bookmark {
            id: id.to_string(),
            url: "https://x.com".to_string(),
            title: "sample".to_string(),
            note: None,
            image_url: Some("https://x.com/chart.png".to_string()),
        }
    }

    fn temp_db(name: &str) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("xbp_{name}_{now}.sqlite"))
            .to_string_lossy()
            .into_owned()
    }

    #[tokio::test]
    async fn pipeline_validates_script_and_returns_result() {
        let pipeline = Arc::new(Pipeline::new(
            Arc::new(StubProvider::new("classifier", "momentum")),
            Arc::new(StubProvider::new("image", "momentum")),
            Arc::new(StubProvider::new("code", "momentum")),
            crate::cache::BookmarkCache::new(temp_db("flow")).unwrap(),
            None,
            2,
        ));
        let count = Arc::new(AtomicUsize::new(0));
        let hook_count = Arc::clone(&count);
        let on_meta_saved: OnMetaSaved = Arc::new(move |_| {
            hook_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let outputs = pipeline
            .run(vec![stub_bookmark("run1")], Some(on_meta_saved))
            .await
            .expect("pipeline");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].meta.classification.category, "momentum");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(outputs[0].pine_script.contains("//@version=6"));
    }

    #[tokio::test]
    async fn pipeline_uses_cached_result_without_llm_calls() {
        let cache = crate::cache::BookmarkCache::new(temp_db("cache_hit")).unwrap();
        let cached = FinalScript {
            bookmark_id: "cache-hit-1".to_string(),
            meta: BookmarkMeta::new(
                "cache-hit-1".to_string(),
                ClassificationOutput {
                    category: "finance".to_string(),
                    confidence: 0.99,
                    rationale: "cached".to_string(),
                },
                ImageAnalysisOutput {
                    signal: "flat".to_string(),
                    summary: "cached analysis".to_string(),
                    indicators: vec!["rsi".to_string()],
                    confidence: 0.9,
                },
                "cache-provider".to_string(),
            ),
            pine_script: "//@version=6\nstrategy(\"cached\")".to_string(),
        };
        cache.upsert(&cached).await.unwrap();

        let classify_calls = Arc::new(AtomicUsize::new(0));
        let analyze_calls = Arc::new(AtomicUsize::new(0));
        let generate_calls = Arc::new(AtomicUsize::new(0));
        let classify_for_providers = Arc::clone(&classify_calls);
        let analyze_for_providers = Arc::clone(&analyze_calls);
        let generate_for_providers = Arc::clone(&generate_calls);

        let pipeline = Arc::new(Pipeline::new(
            Arc::new(StubProvider::with_counters(
                "classifier",
                "finance",
                Arc::clone(&classify_for_providers),
                Arc::clone(&analyze_for_providers),
                Arc::clone(&generate_for_providers),
            )),
            Arc::new(StubProvider::with_counters(
                "image",
                "finance",
                Arc::clone(&classify_for_providers),
                Arc::clone(&analyze_for_providers),
                Arc::clone(&generate_for_providers),
            )),
            Arc::new(StubProvider::with_counters(
                "generator",
                "finance",
                classify_for_providers,
                analyze_for_providers,
                generate_for_providers,
            )),
            cache,
            None,
            2,
        ));

        let outputs = pipeline
            .run(vec![stub_bookmark("cache-hit-1")], None)
            .await
            .expect("pipeline");

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].bookmark_id, "cache-hit-1");
        assert_eq!(outputs[0].pine_script, "//@version=6\nstrategy(\"cached\")");
        assert_eq!(classify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(analyze_calls.load(Ordering::SeqCst), 0);
        assert_eq!(generate_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn pipeline_on_meta_saved_errors_do_not_fail_pipeline() {
        let pipeline = Arc::new(Pipeline::new(
            Arc::new(StubProvider::new("classifier", "finance")),
            Arc::new(StubProvider::new("image", "finance")),
            Arc::new(StubProvider::new("generator", "finance")),
            crate::cache::BookmarkCache::new(temp_db("hook_err")).unwrap(),
            None,
            2,
        ));
        let on_meta_saved: OnMetaSaved = Arc::new(|_| Err(anyhow!("boom")));
        let outputs = pipeline
            .run(vec![stub_bookmark("hook-fail")], Some(on_meta_saved))
            .await
            .expect("pipeline");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].bookmark_id, "hook-fail");
    }

    #[tokio::test]
    async fn validate_pine_script_rejects_invalid_payload() {
        let pipeline = Pipeline::new(
            Arc::new(StubProvider::new("c", "x")),
            Arc::new(StubProvider::new("i", "x")),
            Arc::new(StubProvider::new("g", "x")),
            crate::cache::BookmarkCache::new(temp_db("validate")).unwrap(),
            None,
            1,
        );
        assert!(pipeline.validate_pine_script("no version").is_err());
    }
}
