use crate::error::{PipelineError, PipelineResult};
use crate::models::{ClassificationResult, StrategyPlan};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value as JsonValue;
use std::path::Path;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS bookmark_cache (
    tweet_id TEXT PRIMARY KEY,
    classification_json TEXT,
    plan_json TEXT,
    pine_script TEXT,
    validation_passed INTEGER,
    validation_errors TEXT,
    chart_data_json TEXT,
    completed INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);
"#;

const MIGRATIONS: &[&str] = &[
    "ALTER TABLE bookmark_cache ADD COLUMN chart_data_json TEXT;",
    "ALTER TABLE bookmark_cache ADD COLUMN completed INTEGER DEFAULT 0;",
];

#[derive(Debug, Clone)]
pub struct BookmarkCache {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct CacheRow {
    pub tweet_id: String,
    pub classification_json: Option<String>,
    pub plan_json: Option<String>,
    pub pine_script: Option<String>,
    pub validation_passed: Option<i64>,
    pub validation_errors: Option<String>,
    pub chart_data_json: Option<String>,
    pub completed: bool,
}

impl BookmarkCache {
    pub fn new(path: impl AsRef<Path>) -> PipelineResult<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path_ref)?;
        conn.execute_batch(SCHEMA)?;

        for stmt in MIGRATIONS {
            if let Err(err) = conn.execute_batch(stmt) {
                if !err.to_string().contains("duplicate column name") {
                    return Err(PipelineError::Cache {
                        details: err.to_string(),
                    });
                }
            }
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn get(&self, tweet_id: &str) -> PipelineResult<Option<CacheRow>> {
        let key = tweet_id.to_string();
        self.with_conn("get cache", move |conn| {
            conn.query_row(
                r#"
                SELECT tweet_id, classification_json, plan_json, pine_script,
                       validation_passed, validation_errors, chart_data_json, completed
                FROM bookmark_cache WHERE tweet_id = ?1
                "#,
                params![key],
                |row| {
                    Ok(CacheRow {
                        tweet_id: row.get(0)?,
                        classification_json: row.get(1)?,
                        plan_json: row.get(2)?,
                        pine_script: row.get(3)?,
                        validation_passed: row.get(4)?,
                        validation_errors: row.get(5)?,
                        chart_data_json: row.get(6)?,
                        completed: row.get::<_, i64>(7).unwrap_or(0) != 0,
                    })
                },
            )
            .optional()
        })
        .await
    }

    pub async fn get_classification(&self, tweet_id: &str) -> PipelineResult<Option<ClassificationResult>> {
        let row = self.get(tweet_id).await?;
        match row.and_then(|r| r.classification_json) {
            Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            None => Ok(None),
        }
    }

    pub async fn get_plan(&self, tweet_id: &str) -> PipelineResult<Option<StrategyPlan>> {
        let row = self.get(tweet_id).await?;
        match row.and_then(|r| r.plan_json) {
            Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            None => Ok(None),
        }
    }

    pub async fn get_script(&self, tweet_id: &str) -> PipelineResult<Option<String>> {
        let row = self.get(tweet_id).await?;
        Ok(row.and_then(|r| r.pine_script))
    }

    pub async fn get_chart_data(&self, tweet_id: &str) -> PipelineResult<Option<JsonValue>> {
        let row = self.get(tweet_id).await?;
        match row.and_then(|r| r.chart_data_json) {
            Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            None => Ok(None),
        }
    }

    pub async fn has_classification(&self, tweet_id: &str) -> PipelineResult<bool> {
        Ok(self.get(tweet_id).await?.and_then(|r| r.classification_json).is_some())
    }

    pub async fn has_plan(&self, tweet_id: &str) -> PipelineResult<bool> {
        Ok(self.get(tweet_id).await?.and_then(|r| r.plan_json).is_some())
    }

    pub async fn has_script(&self, tweet_id: &str) -> PipelineResult<bool> {
        Ok(self.get(tweet_id).await?.and_then(|r| r.pine_script).is_some())
    }

    pub async fn has_chart_data(&self, tweet_id: &str) -> PipelineResult<bool> {
        Ok(self.get(tweet_id).await?.and_then(|r| r.chart_data_json).is_some())
    }

    pub async fn has_completed(&self, tweet_id: &str) -> PipelineResult<bool> {
        Ok(self
            .get(tweet_id)
            .await?
            .map(|r| r.completed)
            .unwrap_or(false))
    }

    pub async fn save_classification(
        &self,
        tweet_id: &str,
        value: &ClassificationResult,
    ) -> PipelineResult<()> {
        let data = serde_json::to_string(value)?;
        let tweet_id = tweet_id.to_string();
        self.with_conn("save classification", move |conn| {
            conn.execute(
                "INSERT INTO bookmark_cache (tweet_id, classification_json, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(tweet_id) DO UPDATE SET
                   classification_json = excluded.classification_json,
                   updated_at = datetime('now')",
                params![tweet_id, data],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn save_plan(&self, tweet_id: &str, value: &StrategyPlan) -> PipelineResult<()> {
        let data = serde_json::to_string(value)?;
        let tweet_id = tweet_id.to_string();
        self.with_conn("save plan", move |conn| {
            conn.execute(
                "INSERT INTO bookmark_cache (tweet_id, plan_json, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(tweet_id) DO UPDATE SET
                   plan_json = excluded.plan_json,
                   updated_at = datetime('now')",
                params![tweet_id, data],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn save_script(
        &self,
        tweet_id: &str,
        pine_script: &str,
        validation_passed: bool,
        validation_errors: &[String],
    ) -> PipelineResult<()> {
        let errors_json = serde_json::to_string(validation_errors)?;
        let tweet_id = tweet_id.to_string();
        let script = pine_script.to_string();
        let passed = if validation_passed { 1 } else { 0 };

        self.with_conn("save script", move |conn| {
            conn.execute(
                "INSERT INTO bookmark_cache (tweet_id, pine_script, validation_passed, validation_errors, updated_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))
                 ON CONFLICT(tweet_id) DO UPDATE SET
                   pine_script = excluded.pine_script,
                   validation_passed = excluded.validation_passed,
                   validation_errors = excluded.validation_errors,
                   updated_at = datetime('now')",
                params![tweet_id, script, passed, errors_json],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn save_chart_data(&self, tweet_id: &str, value: &JsonValue) -> PipelineResult<()> {
        let data = serde_json::to_string(value)?;
        let tweet_id = tweet_id.to_string();
        self.with_conn("save chart", move |conn| {
            conn.execute(
                "INSERT INTO bookmark_cache (tweet_id, chart_data_json, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(tweet_id) DO UPDATE SET
                   chart_data_json = excluded.chart_data_json,
                   updated_at = datetime('now')",
                params![tweet_id, data],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn mark_completed(&self, tweet_id: &str) -> PipelineResult<()> {
        let tweet_id = tweet_id.to_string();
        self.with_conn("mark completed", move |conn| {
            conn.execute(
                "INSERT INTO bookmark_cache (tweet_id, completed, updated_at)
                 VALUES (?1, 1, datetime('now'))
                 ON CONFLICT(tweet_id) DO UPDATE SET
                   completed = 1,
                   updated_at = datetime('now')",
                params![tweet_id],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn clear(&self) -> PipelineResult<u64> {
        self.with_conn("clear", |conn| {
            let count = conn.execute("DELETE FROM bookmark_cache", [])?;
            Ok(count as u64)
        })
        .await
    }

    pub async fn stats(&self) -> PipelineResult<JsonValue> {
        self.with_conn("stats", |conn| {
            let total: i64 = conn.query_row("SELECT COUNT(*) FROM bookmark_cache", [], |row| row.get(0))?;
            let classified: i64 =
                conn.query_row("SELECT COUNT(*) FROM bookmark_cache WHERE classification_json IS NOT NULL", [], |row| row.get(0))?;
            let planned: i64 =
                conn.query_row("SELECT COUNT(*) FROM bookmark_cache WHERE plan_json IS NOT NULL", [], |row| row.get(0))?;
            let scripted: i64 =
                conn.query_row("SELECT COUNT(*) FROM bookmark_cache WHERE pine_script IS NOT NULL", [], |row| row.get(0))?;
            let valid: i64 =
                conn.query_row("SELECT COUNT(*) FROM bookmark_cache WHERE validation_passed = 1", [], |row| row.get(0))?;
            let completed: i64 =
                conn.query_row("SELECT COUNT(*) FROM bookmark_cache WHERE completed = 1", [], |row| row.get(0))?;

            Ok(serde_json::json!({
                "total": total,
                "classified": classified,
                "planned": planned,
                "scripted": scripted,
                "valid": valid,
                "completed": completed,
            }))
        })
        .await
    }

    async fn with_conn<T, F>(&self, operation: &str, op: F) -> PipelineResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        let op_name = operation.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| PipelineError::CachePoisoned {
                    details: format!("{op_name}: lock poisoned"),
                })?;
            op(&conn).map_err(PipelineError::from)
        })
        .await
        .map_err(|err| PipelineError::TaskJoin {
            details: err.to_string(),
        })?
    }
}
