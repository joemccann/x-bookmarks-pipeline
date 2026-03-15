use crate::error::{PipelineError, PipelineResult};
use crate::models::{FinalScript, BookmarkMeta};
use rusqlite::{params, Connection};
use serde_json;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct BookmarkCache {
    conn: Arc<Mutex<Connection>>,
}

impl BookmarkCache {
    pub fn new(path: impl AsRef<Path>) -> PipelineResult<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path_ref)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS bookmark_cache (
                bookmark_id TEXT PRIMARY KEY,
                meta_json TEXT NOT NULL,
                script TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn get(&self, bookmark_id: &str) -> PipelineResult<Option<FinalScript>> {
        let key = bookmark_id.to_string();
        self.with_conn("get cache", move |conn| {
            let mut stmt = conn.prepare(
                "SELECT bookmark_id, meta_json, script FROM bookmark_cache WHERE bookmark_id = ?1",
            )?;
            match stmt.query_row(params![key], |row| {
                Ok((row.get::<usize, String>(0)?, row.get::<usize, String>(1)?, row.get::<usize, String>(2)?))
            }) {
                Ok((id, meta_json, script)) => {
                    let meta: BookmarkMeta = serde_json::from_str(&meta_json).map_err(PipelineError::from)?;
                    Ok(Some(FinalScript {
                        bookmark_id: id,
                        meta,
                        pine_script: script,
                    }))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(err) => Err(PipelineError::from(err)),
            }
        })
        .await
    }

    pub async fn upsert(&self, script: &FinalScript) -> PipelineResult<()> {
        let bookmark_id = script.bookmark_id.clone();
        let pine_script = script.pine_script.clone();
        let meta_json = serde_json::to_string(&script.meta)?;
        let created_at = script.meta.generated_at;

        self.with_conn("upsert cache", move |conn| {
            conn
                .execute(
                    "INSERT INTO bookmark_cache (bookmark_id, meta_json, script, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(bookmark_id) DO UPDATE SET
                 meta_json = excluded.meta_json,
                 script = excluded.script,
                 created_at = excluded.created_at;",
                    params![bookmark_id, meta_json, pine_script, created_at],
                )
                .map_err(PipelineError::from)?;
            Ok(())
        })
        .await
    }

    async fn with_conn<T, F>(&self, operation: &'static str, op: F) -> PipelineResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> PipelineResult<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| PipelineError::CachePoisoned { details: format!("{operation}: lock poisoned") })?;
            op(&conn)
        })
        .await
        .map_err(|err| PipelineError::TaskJoin {
            details: err.to_string(),
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BookmarkMeta, ClassificationOutput, ImageAnalysisOutput, FinalScript};

    fn test_cache_path() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("xbp_cache_{now}.sqlite"))
            .to_string_lossy()
            .into_owned()
    }

    #[tokio::test]
    async fn cache_upsert_then_read() {
        let path = test_cache_path();
        let cache = BookmarkCache::new(&path).expect("cache init");
        let meta = BookmarkMeta::new(
            "b1".to_string(),
            ClassificationOutput {
                category: "momentum".to_string(),
                confidence: 0.95,
                rationale: "seed".to_string(),
            },
            ImageAnalysisOutput::no_image_fallback("https://x.com"),
            "openai".to_string(),
        );
        let expected = FinalScript {
            bookmark_id: "b1".to_string(),
            meta: meta.clone(),
            pine_script: "//@version=6\nstrategy(\"demo\")".to_string(),
        };

        cache.upsert(&expected).await.expect("insert");
        let restored = cache.get("b1").await.expect("read").expect("exists");

        assert_eq!(restored.bookmark_id, expected.bookmark_id);
        assert_eq!(restored.meta.classification.category, expected.meta.classification.category);
        assert_eq!(restored.pine_script, expected.pine_script);
    }

    #[tokio::test]
    async fn cache_missing_key_returns_none() {
        let path = test_cache_path();
        let cache = BookmarkCache::new(&path).expect("cache init");
        let entry = cache.get("not-present").await.expect("read");
        assert!(entry.is_none());
    }
}
