use reqwest::Error as ReqwestError;
use rusqlite::Error as RusqliteError;
use serde_json::Error as SerdeJsonError;
use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("request to {provider} exceeded timeout of {timeout_ms}ms")]
    ApiTimeout {
        provider: String,
        timeout_ms: u64,
    },

    #[error("sqlite lock while {operation} after {attempts} attempts")]
    SqliteLock {
        operation: String,
        attempts: u32,
    },

    #[error("cache failure during {details}")]
    Cache {
        details: String,
    },

    #[error("sqlite lock was poisoned: {details}")]
    CachePoisoned {
        details: String,
    },

    #[error("validation rejected Pine Script: {details}")]
    PineValidation {
        details: String,
    },

    #[error("http request failed for {operation}: {details}")]
    Http {
        operation: String,
        details: String,
    },

    #[error("provider {provider} returned invalid payload: {details}")]
    ProviderResponse {
        provider: String,
        details: String,
    },

    #[error("email notification failed: {details}")]
    Email {
        details: String,
    },

    #[error("task join failed: {details}")]
    TaskJoin {
        details: String,
    },

    #[error("file write failed: {details}")]
    Io {
        details: String,
    },
}

pub type PipelineResult<T> = Result<T, PipelineError>;

impl From<RusqliteError> for PipelineError {
    fn from(err: RusqliteError) -> Self {
        let text = err.to_string();
        if text.contains("database is locked") || text.contains("database is busy") {
            PipelineError::SqliteLock {
                operation: "sqlite operation".to_string(),
                attempts: 1,
            }
        } else {
            PipelineError::Cache { details: text }
        }
    }
}

impl From<ReqwestError> for PipelineError {
    fn from(err: ReqwestError) -> Self {
        PipelineError::Http {
            operation: "request".to_string(),
            details: err.to_string(),
        }
    }
}

impl From<SerdeJsonError> for PipelineError {
    fn from(err: SerdeJsonError) -> Self {
        PipelineError::ProviderResponse {
            provider: "provider".to_string(),
            details: err.to_string(),
        }
    }
}

impl From<io::Error> for PipelineError {
    fn from(err: io::Error) -> Self {
        PipelineError::Io {
            details: err.to_string(),
        }
    }
}
