use crate::error::{PipelineError, PipelineResult};
use crate::models::Bookmark;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Callback to check if a bookmark ID is already cached/completed.
/// Returns true if the bookmark should be skipped (already processed).
pub type CacheCheckFn = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Debug, Clone)]
pub struct XBookmarkFetcher {
    endpoint: String,
    token: Arc<Mutex<String>>,
    page_limit: usize,
    total_limit: usize,
    max_pages: usize,
    client: Client,
    /// Stop fetching additional pages when this many consecutive cached bookmarks are found
    early_stop_threshold: usize,
}

#[derive(Debug, Clone)]
pub struct XBookmarkPage {
    pub bookmarks: Vec<Bookmark>,
    pub next_page_token: Option<String>,
}

/// Statistics from a fetch operation
#[derive(Debug, Clone, Default)]
pub struct FetchStats {
    /// Number of API requests made
    pub api_requests: u32,
    /// Total bookmarks fetched from API
    pub total_fetched: usize,
    /// Bookmarks that were new (not in cache)
    pub new_bookmarks: usize,
    /// Whether early termination was triggered
    pub early_stopped: bool,
    /// Estimated API cost in USD
    pub estimated_cost: f64,
}

impl XBookmarkFetcher {
    pub fn new(
        endpoint: impl Into<String>,
        token: impl Into<String>,
        page_limit: usize,
        total_limit: usize,
        max_pages: usize,
        client: Client,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            token: Arc::new(Mutex::new(token.into())),
            page_limit: page_limit.max(1),
            total_limit: total_limit.max(1),
            max_pages: max_pages.max(1),
            client,
            early_stop_threshold: 10, // Stop if 10+ consecutive cached bookmarks
        }
    }

    /// Set the early stop threshold (consecutive cached bookmarks before stopping)
    pub fn with_early_stop_threshold(mut self, threshold: usize) -> Self {
        self.early_stop_threshold = threshold;
        self
    }

    /// Standard fetch - returns all bookmarks up to limit
    pub async fn fetch(&self) -> PipelineResult<Vec<Bookmark>> {
        let (bookmarks, _stats) = self.fetch_with_stats(None).await?;
        Ok(bookmarks)
    }

    /// Fetch with early termination when hitting cached bookmarks.
    /// 
    /// This is the cost-optimized fetch path:
    /// - Stops fetching additional pages when consecutive cached bookmarks exceed threshold
    /// - Returns only NEW bookmarks (not in cache)
    /// - Tracks statistics for cost monitoring
    pub async fn fetch_incremental(
        &self,
        cached_ids: &HashSet<String>,
    ) -> PipelineResult<(Vec<Bookmark>, FetchStats)> {
        let cache_check: CacheCheckFn = {
            let cached = cached_ids.clone();
            Arc::new(move |id: &str| cached.contains(id))
        };
        self.fetch_with_stats(Some(cache_check)).await
    }

    /// Internal fetch with optional cache checking and statistics
    async fn fetch_with_stats(
        &self,
        cache_check: Option<CacheCheckFn>,
    ) -> PipelineResult<(Vec<Bookmark>, FetchStats)> {
        let mut all_bookmarks = Vec::new();
        let mut next_token: Option<String> = None;
        let mut pages = 0usize;
        let mut stats = FetchStats::default();
        let mut consecutive_cached = 0usize;

        while pages < self.max_pages {
            pages += 1;
            stats.api_requests += 1;

            let page = self.fetch_page(next_token.as_deref()).await?;
            let has_entries = !page.bookmarks.is_empty();
            let next = page.next_page_token;

            stats.total_fetched += page.bookmarks.len();

            // Process bookmarks with optional cache filtering
            for bookmark in page.bookmarks {
                if let Some(ref check) = cache_check {
                    if check(&bookmark.id) {
                        consecutive_cached += 1;
                        // Early termination: if we hit N consecutive cached bookmarks,
                        // assume we've seen all new content (X returns newest first)
                        if consecutive_cached >= self.early_stop_threshold {
                            stats.early_stopped = true;
                            break;
                        }
                        continue; // Skip cached bookmark
                    }
                    consecutive_cached = 0; // Reset on new bookmark
                }
                stats.new_bookmarks += 1;
                all_bookmarks.push(bookmark);
            }

            if stats.early_stopped {
                break;
            }

            if next.is_none() || all_bookmarks.len() >= self.total_limit {
                break;
            }

            next_token = next;
            if !has_entries && next_token.is_none() {
                break;
            }
        }

        all_bookmarks.truncate(self.total_limit);

        // Calculate estimated cost: $0.05 per 100 bookmarks fetched
        stats.estimated_cost = (stats.total_fetched as f64 / 100.0) * 0.05;

        Ok((all_bookmarks, stats))
    }

    pub async fn set_access_token(&self, token: impl Into<String>) {
        let mut lock = self.token.lock().await;
        *lock = token.into();
    }

    pub async fn get_access_token(&self) -> String {
        self.token.lock().await.clone()
    }

    pub(crate) async fn fetch_page(&self, page_token: Option<&str>) -> PipelineResult<XBookmarkPage> {
        let token = self.get_access_token().await;
        let mut request = self.client.get(&self.endpoint).bearer_auth(token);
        if let Some(token) = page_token {
            request = request.query(&[("pagination_token", token)]);
        }
        request = request
            .query(&[("max_results", self.page_limit.to_string())])
            .query(&[("tweet.fields", "created_at,author_id,attachments")])
            .query(&[("expansions", "author_id,attachments.media_keys")])
            .query(&[("user.fields", "username,name")])
            .query(&[("media.fields", "url,type")]);

        let response = request.send().await.map_err(|err| PipelineError::Http {
            operation: "x_fetch".to_string(),
            details: err.to_string(),
        })?;

        let status = response.status();
        let raw = response.text().await.map_err(|err| PipelineError::Http {
            operation: "x_fetch_body".to_string(),
            details: err.to_string(),
        })?;

        if is_token_expired(status, &raw) {
            return Err(PipelineError::TokenExpired {
                details: sanitize_token_error(&raw).unwrap_or_else(|| format!("status={status}, body={raw}")),
            });
        }

        if !status.is_success() {
            return Err(PipelineError::Http {
                operation: "x_fetch".to_string(),
                details: format!("status={status}, body={raw}"),
            });
        }

        parse_bookmarks_response(&raw)
    }
}

pub(crate) fn parse_bookmarks_response(payload: &str) -> PipelineResult<XBookmarkPage> {
    let root: Value = serde_json::from_str(payload)?;
    if let Some(errors) = root.get("errors").and_then(Value::as_array) {
        if let Some(message) = errors
            .iter()
            .find_map(|error| error.get("message").and_then(Value::as_str))
        {
            return Err(PipelineError::ProviderResponse {
                provider: "x_fetch".to_string(),
                details: message.to_string(),
            });
        }
    }

    let items = root
        .get("data")
        .and_then(Value::as_array)
        .map_or(&[][..], |items| items);
    let mut media_index = HashMap::new();
    let mut user_index: HashMap<String, (String, String)> = HashMap::new(); // id -> (username, name)

    if let Some(includes) = root.get("includes").and_then(Value::as_object) {
        if let Some(media) = includes.get("media").and_then(Value::as_array) {
            for entry in media {
                if let Some(id) = entry.get("media_key").and_then(Value::as_str) {
                    if let Some(url) = entry.get("url").and_then(Value::as_str) {
                        media_index.insert(id.to_string(), url.to_string());
                    }
                }
            }
        }
        if let Some(users) = includes.get("users").and_then(Value::as_array) {
            for user in users {
                if let Some(id) = user.get("id").and_then(Value::as_str) {
                    let username = user.get("username").and_then(Value::as_str).unwrap_or("").to_string();
                    let name = user.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    user_index.insert(id.to_string(), (username, name));
                }
            }
        }
    }

    let mut bookmarks = Vec::with_capacity(items.len());
    for entry in items {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let text = entry
            .get("text")
            .or_else(|| entry.get("full_text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let author_id = entry
            .get("author_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let author = if let Some((username, _name)) = user_index.get(author_id) {
            if username.is_empty() { author_id.to_string() } else { username.clone() }
        } else if author_id.is_empty() {
            "unknown".to_string()
        } else {
            author_id.to_string()
        };
        let date = entry
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut image_urls = Vec::new();
        if let Some(attachments) = entry.get("attachments").and_then(Value::as_object) {
            if let Some(keys) = attachments.get("media_keys").and_then(Value::as_array) {
                for key in keys {
                    if let Some(media_key) = key.as_str() {
                        if let Some(url) = media_index.get(media_key) {
                            image_urls.push(url.clone());
                        }
                    }
                }
            }
        }

        let tweet_url = if id.is_empty() {
            String::from("https://x.com/i/web")
        } else {
            format!("https://x.com/{author}/status/{id}")
        };

        bookmarks.push(Bookmark {
            id: if id.is_empty() {
                deterministic_id(&text)
            } else {
                id
            },
            text,
            author: author,
            date,
            image_urls,
            tweet_url,
            chart_description: String::new(),
        });
    }

    let next_page_token = root
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("next_token").and_then(Value::as_str))
        .map(|token| token.to_string());

    Ok(XBookmarkPage {
        bookmarks,
        next_page_token,
    })
}

fn is_token_expired(status: StatusCode, payload: &str) -> bool {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return true;
    }
    if status == StatusCode::BAD_REQUEST && payload.to_lowercase().contains("expired") {
        return true;
    }
    false
}

fn sanitize_token_error(payload: &str) -> Option<String> {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            serde_json::from_str::<Value>(payload)
                .ok()
                .and_then(|value| {
                    value
                        .get("title")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
        })
        .or_else(|| payload.lines().next().map(ToString::to_string))
}

fn deterministic_id(seed: &str) -> String {
    let mut digest = Vec::new();
    for byte in seed.bytes() {
        digest.push(byte);
    }
    let hash = digest
        .iter()
        .fold(0u64, |acc, byte| acc.wrapping_mul(31).wrapping_add(*byte as u64));
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bookmarks_response_handles_pagination() {
        let payload = r#"{
            "data": [
                {"id": "1", "text": "BTC uptrend", "author_id": "111", "created_at": "2026-03-14T10:00:00.000Z"},
                {"id": "2", "text": "ETH range", "author_id": "222", "created_at": "2026-03-15T12:30:00.000Z"}
            ],
            "includes": {
                "users": [
                    {"id": "111", "username": "alice", "name": "Alice A"},
                    {"id": "222", "username": "bob", "name": "Bob B"}
                ]
            },
            "meta": {"next_token":"abc"}
        }"#;

        let page = parse_bookmarks_response(payload).unwrap();
        assert_eq!(page.bookmarks.len(), 2);
        assert_eq!(page.next_page_token, Some("abc".to_string()));
        assert_eq!(page.bookmarks[0].id, "1");
        assert_eq!(page.bookmarks[0].author, "alice");
        assert_eq!(page.bookmarks[0].date, "2026-03-14T10:00:00.000Z");
        assert_eq!(page.bookmarks[0].tweet_url, "https://x.com/alice/status/1");
        assert_eq!(page.bookmarks[1].author, "bob");
        assert_eq!(page.bookmarks[1].date, "2026-03-15T12:30:00.000Z");
    }

    #[test]
    fn parse_bookmarks_response_falls_back_without_includes() {
        let payload = r#"{
            "data": [
                {"id": "1", "text": "no includes", "author_id": "999"}
            ]
        }"#;

        let page = parse_bookmarks_response(payload).unwrap();
        assert_eq!(page.bookmarks[0].author, "999");
        assert_eq!(page.bookmarks[0].date, "");
    }

    #[test]
    fn token_expired_detected_from_status() {
        assert!(is_token_expired(StatusCode::UNAUTHORIZED, "{}"));
    }
}
