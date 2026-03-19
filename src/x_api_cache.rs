//! X API request caching and budgeting to minimize API costs.
//!
//! This module provides:
//! - Username → user_id resolution caching (persisted to SQLite)
//! - Token validation state caching (skip redundant /users/me calls)
//! - Request counting and budgeting (hard limits per cycle/day)
//!
//! X API pricing (Pay-as-you-go, 2024):
//! - GET /2/users/:id/bookmarks: $0.05 per 100 tweets
//! - GET /2/users/me: $0.01 per request
//! - GET /2/users/by/username/:username: $0.01 per request

use crate::error::{PipelineError, PipelineResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// X API cost estimates (USD) for budgeting
pub mod costs {
    /// Cost per 100 bookmarks fetched
    pub const BOOKMARKS_PER_100: f64 = 0.05;
    /// Cost per /users/me validation call
    pub const USER_ME: f64 = 0.01;
    /// Cost per username lookup
    pub const USERNAME_LOOKUP: f64 = 0.01;
    /// OAuth token operations are free
    pub const TOKEN_REFRESH: f64 = 0.0;
}

const X_API_CACHE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS x_api_cache (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    expires_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS x_api_stats (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    requests_today INTEGER DEFAULT 0,
    requests_this_cycle INTEGER DEFAULT 0,
    estimated_cost_today REAL DEFAULT 0.0,
    last_reset_day INTEGER,
    last_cycle_start INTEGER
);

INSERT OR IGNORE INTO x_api_stats (id, requests_today, requests_this_cycle, estimated_cost_today, last_reset_day, last_cycle_start)
VALUES (1, 0, 0, 0.0, 0, 0);
"#;

/// Cached username → user_id mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedUserId {
    pub username: String,
    pub user_id: String,
    pub cached_at: u64,
}

/// Token validation state - avoid redundant /users/me calls
#[derive(Debug, Clone)]
pub struct TokenValidationState {
    /// Hash of the token that was validated
    token_hash: u64,
    /// When the validation was performed
    validated_at: Instant,
    /// Whether the token was valid
    valid: bool,
}

impl TokenValidationState {
    pub fn new(token: &str, valid: bool) -> Self {
        Self {
            token_hash: simple_hash(token),
            validated_at: Instant::now(),
            valid,
        }
    }

    /// Check if this validation is still usable for the given token
    pub fn is_valid_for(&self, token: &str, max_age: Duration) -> Option<bool> {
        if simple_hash(token) != self.token_hash {
            return None; // Different token
        }
        if self.validated_at.elapsed() > max_age {
            return None; // Expired
        }
        Some(self.valid)
    }
}

/// Simple non-cryptographic hash for token comparison
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    hash
}

/// Request budget configuration
#[derive(Debug, Clone)]
pub struct RequestBudget {
    /// Maximum requests per daemon cycle (0 = unlimited)
    pub max_per_cycle: u32,
    /// Maximum requests per day (0 = unlimited)
    pub max_per_day: u32,
    /// Maximum estimated cost per day in USD (0.0 = unlimited)
    pub max_cost_per_day: f64,
}

impl Default for RequestBudget {
    fn default() -> Self {
        Self {
            max_per_cycle: 0,    // unlimited by default
            max_per_day: 0,      // unlimited by default
            max_cost_per_day: 0.0, // unlimited by default
        }
    }
}

/// X API cache for reducing redundant API calls
#[derive(Clone)]
pub struct XApiCache {
    conn: Arc<Mutex<Connection>>,
    /// In-memory token validation state (not persisted)
    token_state: Arc<Mutex<Option<TokenValidationState>>>,
    /// Request budget configuration
    budget: RequestBudget,
}

impl XApiCache {
    /// Create a new X API cache at the given path
    pub fn new(path: impl AsRef<Path>) -> PipelineResult<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path_ref)?;
        conn.execute_batch(X_API_CACHE_SCHEMA)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            token_state: Arc::new(Mutex::new(None)),
            budget: RequestBudget::default(),
        })
    }

    /// Create cache with a specific budget
    pub fn with_budget(mut self, budget: RequestBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Get cached user_id for a username (if not expired)
    pub fn get_user_id(&self, username: &str) -> PipelineResult<Option<String>> {
        let key = format!("user_id:{}", username.to_lowercase());
        let conn = self.conn.lock().map_err(|_| PipelineError::CachePoisoned {
            details: "x_api_cache lock poisoned".to_string(),
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let result: Option<String> = conn
            .query_row(
                "SELECT value FROM x_api_cache WHERE key = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
                params![key, now],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(json) = result {
            if let Ok(cached) = serde_json::from_str::<CachedUserId>(&json) {
                return Ok(Some(cached.user_id));
            }
        }

        Ok(None)
    }

    /// Cache a username → user_id mapping (default: 30 days expiry)
    pub fn set_user_id(&self, username: &str, user_id: &str) -> PipelineResult<()> {
        self.set_user_id_with_ttl(username, user_id, Duration::from_secs(30 * 24 * 60 * 60))
    }

    /// Cache a username → user_id mapping with custom TTL
    pub fn set_user_id_with_ttl(
        &self,
        username: &str,
        user_id: &str,
        ttl: Duration,
    ) -> PipelineResult<()> {
        let key = format!("user_id:{}", username.to_lowercase());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cached = CachedUserId {
            username: username.to_lowercase(),
            user_id: user_id.to_string(),
            cached_at: now,
        };
        let value = serde_json::to_string(&cached)?;
        let expires_at = now + ttl.as_secs();

        let conn = self.conn.lock().map_err(|_| PipelineError::CachePoisoned {
            details: "x_api_cache lock poisoned".to_string(),
        })?;

        conn.execute(
            "INSERT INTO x_api_cache (key, value, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET
               value = excluded.value,
               expires_at = excluded.expires_at",
            params![key, value, expires_at as i64, now as i64],
        )?;

        Ok(())
    }

    /// Check if token validation can be skipped (recently validated same token)
    pub fn check_token_validation_cache(&self, token: &str, max_age: Duration) -> Option<bool> {
        let state = self.token_state.lock().ok()?;
        state.as_ref()?.is_valid_for(token, max_age)
    }

    /// Update token validation cache
    pub fn set_token_validation(&self, token: &str, valid: bool) {
        if let Ok(mut state) = self.token_state.lock() {
            *state = Some(TokenValidationState::new(token, valid));
        }
    }

    /// Clear token validation cache (e.g., after token refresh)
    pub fn clear_token_validation(&self) {
        if let Ok(mut state) = self.token_state.lock() {
            *state = None;
        }
    }

    /// Record an API request and check budget
    pub fn record_request(&self, estimated_cost: f64) -> PipelineResult<bool> {
        let conn = self.conn.lock().map_err(|_| PipelineError::CachePoisoned {
            details: "x_api_cache lock poisoned".to_string(),
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Get current day (UTC)
        let today = now / 86400;

        // Check if we need to reset daily counters
        let last_reset_day: i64 = conn
            .query_row("SELECT last_reset_day FROM x_api_stats WHERE id = 1", [], |row| row.get(0))
            .unwrap_or(0);

        if today > last_reset_day {
            conn.execute(
                "UPDATE x_api_stats SET requests_today = 0, estimated_cost_today = 0.0, last_reset_day = ?1 WHERE id = 1",
                params![today],
            )?;
        }

        // Get current stats
        let (requests_today, requests_cycle, cost_today): (i64, i64, f64) = conn
            .query_row(
                "SELECT requests_today, requests_this_cycle, estimated_cost_today FROM x_api_stats WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0, 0.0));

        // Check budget limits
        if self.budget.max_per_cycle > 0 && requests_cycle >= self.budget.max_per_cycle as i64 {
            return Ok(false); // Cycle budget exceeded
        }
        if self.budget.max_per_day > 0 && requests_today >= self.budget.max_per_day as i64 {
            return Ok(false); // Daily budget exceeded
        }
        if self.budget.max_cost_per_day > 0.0 && cost_today >= self.budget.max_cost_per_day {
            return Ok(false); // Cost budget exceeded
        }

        // Increment counters
        conn.execute(
            "UPDATE x_api_stats SET 
               requests_today = requests_today + 1,
               requests_this_cycle = requests_this_cycle + 1,
               estimated_cost_today = estimated_cost_today + ?1
             WHERE id = 1",
            params![estimated_cost],
        )?;

        Ok(true)
    }

    /// Reset cycle counter (call at start of each daemon cycle)
    pub fn reset_cycle(&self) -> PipelineResult<()> {
        let conn = self.conn.lock().map_err(|_| PipelineError::CachePoisoned {
            details: "x_api_cache lock poisoned".to_string(),
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "UPDATE x_api_stats SET requests_this_cycle = 0, last_cycle_start = ?1 WHERE id = 1",
            params![now],
        )?;

        Ok(())
    }

    /// Get current request statistics
    pub fn get_stats(&self) -> PipelineResult<RequestStats> {
        let conn = self.conn.lock().map_err(|_| PipelineError::CachePoisoned {
            details: "x_api_cache lock poisoned".to_string(),
        })?;

        let (requests_today, requests_cycle, cost_today): (i64, i64, f64) = conn
            .query_row(
                "SELECT requests_today, requests_this_cycle, estimated_cost_today FROM x_api_stats WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((0, 0, 0.0));

        Ok(RequestStats {
            requests_today: requests_today as u32,
            requests_this_cycle: requests_cycle as u32,
            estimated_cost_today: cost_today,
        })
    }

    /// Clean up expired entries
    pub fn cleanup_expired(&self) -> PipelineResult<u64> {
        let conn = self.conn.lock().map_err(|_| PipelineError::CachePoisoned {
            details: "x_api_cache lock poisoned".to_string(),
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let count = conn.execute(
            "DELETE FROM x_api_cache WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            params![now],
        )?;

        Ok(count as u64)
    }
}

/// Current request statistics
#[derive(Debug, Clone)]
pub struct RequestStats {
    pub requests_today: u32,
    pub requests_this_cycle: u32,
    pub estimated_cost_today: f64,
}

impl std::fmt::Display for RequestStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "today: {} reqs (${:.4}), cycle: {} reqs",
            self.requests_today, self.estimated_cost_today, self.requests_this_cycle
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn temp_path(prefix: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        path.push(format!("xbp-xapi-{prefix}-{nanos}.db"));
        path
    }

    #[test]
    fn user_id_cache_roundtrip() {
        let path = temp_path("user-id");
        let _ = std::fs::remove_file(&path);

        let cache = XApiCache::new(&path).unwrap();

        // Initially empty
        assert!(cache.get_user_id("alice").unwrap().is_none());

        // Set and retrieve
        cache.set_user_id("alice", "12345").unwrap();
        assert_eq!(cache.get_user_id("alice").unwrap(), Some("12345".to_string()));

        // Case insensitive
        assert_eq!(cache.get_user_id("ALICE").unwrap(), Some("12345".to_string()));
    }

    #[test]
    fn token_validation_cache() {
        let path = temp_path("token-val");
        let _ = std::fs::remove_file(&path);

        let cache = XApiCache::new(&path).unwrap();

        // Initially empty
        assert!(cache.check_token_validation_cache("token123", Duration::from_secs(60)).is_none());

        // Set and check
        cache.set_token_validation("token123", true);
        assert_eq!(
            cache.check_token_validation_cache("token123", Duration::from_secs(60)),
            Some(true)
        );

        // Different token returns None
        assert!(cache.check_token_validation_cache("token456", Duration::from_secs(60)).is_none());

        // Clear
        cache.clear_token_validation();
        assert!(cache.check_token_validation_cache("token123", Duration::from_secs(60)).is_none());
    }

    #[test]
    fn request_budget_enforcement() {
        let path = temp_path("budget");
        let _ = std::fs::remove_file(&path);

        let cache = XApiCache::new(&path)
            .unwrap()
            .with_budget(RequestBudget {
                max_per_cycle: 3,
                max_per_day: 0,
                max_cost_per_day: 0.0,
            });

        cache.reset_cycle().unwrap();

        // First 3 requests should succeed
        assert!(cache.record_request(0.01).unwrap());
        assert!(cache.record_request(0.01).unwrap());
        assert!(cache.record_request(0.01).unwrap());

        // 4th should fail
        assert!(!cache.record_request(0.01).unwrap());

        // After reset, should work again
        cache.reset_cycle().unwrap();
        assert!(cache.record_request(0.01).unwrap());
    }

    #[test]
    fn request_stats() {
        let path = temp_path("stats");
        let _ = std::fs::remove_file(&path);

        let cache = XApiCache::new(&path).unwrap();
        cache.reset_cycle().unwrap();

        cache.record_request(0.05).unwrap();
        cache.record_request(0.01).unwrap();

        let stats = cache.get_stats().unwrap();
        assert_eq!(stats.requests_this_cycle, 2);
        assert!((stats.estimated_cost_today - 0.06).abs() < 0.001);
    }
}
