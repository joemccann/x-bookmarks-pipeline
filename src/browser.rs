use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::{sleep, Instant};
use tokio_tungstenite::tungstenite::Message;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration for the CDP auto-consent task.
pub struct AutoConsentConfig {
    /// The OAuth authorization URL that was opened in the browser.
    /// Used to match the correct tab (by URL prefix or `state` parameter).
    pub expected_auth_url: String,
    /// Path to the Chrome user-data-dir whose `DevToolsActivePort` we read.
    /// If not set, defaults to the standard macOS/Linux/Windows Chrome profile.
    pub chrome_user_data_dir: PathBuf,
    /// Maximum time to keep trying before giving up.
    pub overall_timeout: Duration,
    /// Delay between target-discovery polls.
    pub poll_interval: Duration,
}

/// Returns the default Chrome user-data-dir for the current platform.
pub fn default_chrome_user_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs_home().map(|h| h.join("Library/Application Support/Google/Chrome"))
    } else if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA")
            .ok()
            .map(|d| PathBuf::from(d).join("Google/Chrome/User Data"))
    } else {
        dirs_home().map(|h| h.join(".config/google-chrome"))
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Structured outcome — better than a bool for logs and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoConsentOutcome {
    /// The consent button was clicked automatically.
    Clicked { strategy: String },
    /// Auto-consent is not available; user must click manually.
    ManualFallback(&'static str),
    /// Polling expired without finding or clicking the consent button.
    TimedOut,
}

// ---------------------------------------------------------------------------
// Target classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Consent,
    Login,
    Callback,
    Other,
}

pub fn classify_target(url: &str) -> TargetKind {
    let lower = url.to_lowercase();
    if lower.contains("/i/oauth2/authorize") {
        TargetKind::Consent
    } else if lower.contains("/i/flow/login")
        || lower.contains("/login")
            && (lower.contains("x.com") || lower.contains("twitter.com"))
    {
        TargetKind::Login
    } else if lower.contains("localhost") && lower.contains("callback") {
        TargetKind::Callback
    } else {
        TargetKind::Other
    }
}

// ---------------------------------------------------------------------------
// DevToolsActivePort parsing
// ---------------------------------------------------------------------------

pub fn devtools_active_port_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("DevToolsActivePort")
}

/// Parse the two-line DevToolsActivePort file.
/// Line 1: port number (e.g. "9222")
/// Line 2: browser websocket path (e.g. "/devtools/browser/abc-123")
pub fn parse_devtools_active_port(contents: &str) -> Result<(u16, String)> {
    let mut lines = contents.lines();
    let port_line = lines
        .next()
        .filter(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("DevToolsActivePort: missing port line"))?;
    let port: u16 = port_line
        .trim()
        .parse()
        .context("DevToolsActivePort: invalid port number")?;
    let ws_path = lines
        .next()
        .filter(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("DevToolsActivePort: missing websocket path line"))?
        .trim()
        .to_string();
    Ok((port, ws_path))
}

pub fn build_ws_url(port: u16, browser_ws_path: &str) -> String {
    format!("ws://127.0.0.1:{port}{browser_ws_path}")
}

// ---------------------------------------------------------------------------
// Target discovery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TargetInfo {
    pub target_id: String,
    pub url: String,
    pub kind: TargetKind,
}

/// Find the OAuth consent page target from `Target.getTargets` response.
/// Requires `type == "page"` and URL matching the expected auth URL.
/// Prefers exact URL prefix match; falls back to domain + path match.
pub fn find_oauth_target(targets: &[Value], expected_auth_url: &str) -> Option<TargetInfo> {
    // Extract the state param from expected URL for precise matching
    let expected_state = extract_query_param(expected_auth_url, "state");

    let mut best: Option<TargetInfo> = None;

    for target in targets {
        let target_type = target["type"].as_str().unwrap_or("");
        if target_type != "page" {
            continue;
        }
        let url = target["url"].as_str().unwrap_or("");
        let target_id = target["targetId"].as_str().unwrap_or("");
        let kind = classify_target(url);

        if kind != TargetKind::Consent {
            continue;
        }

        let info = TargetInfo {
            target_id: target_id.to_string(),
            url: url.to_string(),
            kind,
        };

        // Prefer exact state match if we have one
        if let Some(ref expected) = expected_state {
            if let Some(actual) = extract_query_param(url, "state") {
                if &actual == expected {
                    return Some(info);
                }
            }
        }

        // Otherwise keep the first consent-page match
        if best.is_none() {
            best = Some(info);
        }
    }

    best
}

fn extract_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Click JavaScript
// ---------------------------------------------------------------------------

/// Returns an async IIFE that waits up to 5 seconds for the consent button,
/// tries multiple selector strategies, and returns structured JSON.
pub fn build_click_js() -> &'static str {
    r#"(async function() {
    function tryClick() {
        // Strategy 1: data-testid (X's convention)
        var btn = document.querySelector('[data-testid="OAuth_Consent_Button"]');
        if (btn) { btn.click(); return {clicked:true, strategy:"data-testid", label:btn.textContent.trim().substring(0,80)}; }

        // Strategy 2: button with "Authorize app" text
        var buttons = Array.from(document.querySelectorAll('button'));
        btn = buttons.find(function(b) { return /authorize\s*app/i.test(b.textContent); });
        if (btn) { btn.click(); return {clicked:true, strategy:"text-authorize-app", label:btn.textContent.trim().substring(0,80)}; }

        // Strategy 3: button with "Allow" text
        btn = buttons.find(function(b) { return /^allow$/i.test(b.textContent.trim()); });
        if (btn) { btn.click(); return {clicked:true, strategy:"text-allow", label:btn.textContent.trim().substring(0,80)}; }

        return {clicked:false, strategy:"none", label:""};
    }

    // Poll up to 5 seconds for the button to appear (React may still be rendering)
    for (var i = 0; i < 25; i++) {
        var result = tryClick();
        if (result.clicked) return JSON.stringify(result);
        await new Promise(function(r) { setTimeout(r, 200); });
    }
    return JSON.stringify({clicked:false, strategy:"none", label:""});
})()"#
}

/// Parse the JSON string returned by the click JS.
#[derive(Debug, Clone)]
pub struct ClickResult {
    pub clicked: bool,
    pub strategy: String,
    pub label: String,
}

pub fn parse_click_result(value: &str) -> ClickResult {
    if let Ok(v) = serde_json::from_str::<Value>(value) {
        ClickResult {
            clicked: v["clicked"].as_bool().unwrap_or(false),
            strategy: v["strategy"].as_str().unwrap_or("unknown").to_string(),
            label: v["label"].as_str().unwrap_or("").to_string(),
        }
    } else {
        ClickResult {
            clicked: false,
            strategy: "parse-error".to_string(),
            label: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// CDP WebSocket client
// ---------------------------------------------------------------------------

pub struct CdpClient {
    sink: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    stream: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    next_id: AtomicU64,
    cmd_timeout: Duration,
}

impl CdpClient {
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws, _response) = tokio_tungstenite::connect_async(url)
            .await
            .context("CDP WebSocket connect failed")?;
        let (sink, stream) = ws.split();
        Ok(Self {
            sink,
            stream,
            next_id: AtomicU64::new(1),
            cmd_timeout: Duration::from_secs(5),
        })
    }

    /// Send a CDP command (browser-level, no session).
    pub async fn send(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({ "id": id, "method": method, "params": params });
        self.sink
            .send(Message::Text(msg.to_string().into()))
            .await
            .context("CDP send failed")?;
        self.read_response(id).await
    }

    /// Send a CDP command scoped to a flat session.
    pub async fn send_to_session(
        &mut self,
        session_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "id": id,
            "method": method,
            "params": params,
            "sessionId": session_id,
        });
        self.sink
            .send(Message::Text(msg.to_string().into()))
            .await
            .context("CDP send_to_session failed")?;
        self.read_response(id).await
    }

    /// Read messages until we find the response matching `expected_id`.
    /// Discards CDP event messages (no `id` field) along the way.
    async fn read_response(&mut self, expected_id: u64) -> Result<Value> {
        let deadline = Instant::now() + self.cmd_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow::anyhow!(
                    "CDP response timeout for message id {expected_id}"
                ));
            }
            let msg = tokio::time::timeout(remaining, self.stream.next())
                .await
                .map_err(|_| anyhow::anyhow!("CDP response timeout for message id {expected_id}"))?
                .ok_or_else(|| anyhow::anyhow!("CDP WebSocket stream ended"))?
                .context("CDP WebSocket read error")?;

            if let Message::Text(text) = msg {
                let value: Value =
                    serde_json::from_str(&text).context("CDP response is not valid JSON")?;
                // Check if this is a response (has `id`) matching ours
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    if id == expected_id {
                        if let Some(error) = value.get("error") {
                            return Err(anyhow::anyhow!("CDP error: {}", error));
                        }
                        return Ok(value);
                    }
                }
                // Otherwise it's an event or someone else's response — discard
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP-based Chrome discovery (fallback when DevToolsActivePort is missing)
// ---------------------------------------------------------------------------

/// Query Chrome's HTTP discovery endpoint to get the browser WebSocket URL.
/// Chrome with `--remote-debugging-port=PORT` exposes `/json/version` which
/// returns `{ "webSocketDebuggerUrl": "ws://..." }`.
async fn discover_ws_url_via_http(port: u16) -> Option<String> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    let body: Value = resp.json().await.ok()?;
    body.get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(String::from)
}

/// Close Chrome tabs whose URL contains the given substring.
/// Uses Chrome's `/json/close/{id}` HTTP endpoint — no WebSocket needed.
pub async fn close_tabs_matching(url_substring: &str) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let resp = match client.get("http://127.0.0.1:9222/json").send().await {
        Ok(r) => r,
        Err(_) => return,
    };
    let tabs: Vec<Value> = resp.json().await.unwrap_or_default();

    for tab in &tabs {
        let tab_url = tab["url"].as_str().unwrap_or("");
        if tab_url.contains(url_substring) {
            if let Some(id) = tab["id"].as_str() {
                let close_url = format!("http://127.0.0.1:9222/json/close/{id}");
                let _ = client.get(&close_url).send().await;
                eprintln!("[cdp] closed tab: {}", &tab_url[..tab_url.len().min(80)]);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CDP auto-consent — connects to the user's existing Chrome
// ---------------------------------------------------------------------------

/// Attempt to auto-click the OAuth consent button in Chrome via CDP.
///
/// This function is designed to run concurrently with `wait_for_oauth_code`.
/// It connects to Chrome's DevTools WebSocket, finds the OAuth consent page,
/// and clicks the "Authorize app" button. If anything fails, it returns
/// a structured outcome explaining why, never panicking or blocking.
pub async fn auto_click_oauth_consent(cfg: AutoConsentConfig) -> Result<AutoConsentOutcome> {
    let port_path = devtools_active_port_path(&cfg.chrome_user_data_dir);
    let deadline = Instant::now() + cfg.overall_timeout;

    // Phase 1: Discover the Chrome DevTools WebSocket URL.
    // Try two methods:
    //   A) Read DevToolsActivePort file from the configured user-data-dir
    //   B) Query the HTTP discovery endpoint at well-known port (9222)
    let ws_url = loop {
        if Instant::now() >= deadline {
            return Ok(AutoConsentOutcome::ManualFallback(
                "Could not discover Chrome DevTools endpoint before deadline",
            ));
        }

        // Method A: DevToolsActivePort file
        if let Ok(contents) = tokio::fs::read_to_string(&port_path).await {
            if let Ok((port, ws_path)) = parse_devtools_active_port(&contents) {
                break build_ws_url(port, &ws_path);
            }
        }

        // Method B: HTTP discovery at well-known port 9222
        if let Some(url) = discover_ws_url_via_http(9222).await {
            eprintln!("[cdp] discovered Chrome via HTTP endpoint on port 9222");
            break url;
        }

        sleep(cfg.poll_interval).await;
    };

    let mut client = match CdpClient::connect(&ws_url).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(AutoConsentOutcome::ManualFallback(
                if e.to_string().contains("Connection refused") {
                    "Chrome remote debugging not enabled (connection refused)"
                } else {
                    "CDP WebSocket connection failed"
                },
            ));
        }
    };

    // Phase 2: Poll for the OAuth consent tab
    let mut retry_count = 0u32;
    let mut saw_login = false;

    loop {
        if Instant::now() >= deadline {
            return Ok(AutoConsentOutcome::TimedOut);
        }
        retry_count += 1;

        let targets_resp = match client.send("Target.getTargets", json!({})).await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("[cdp] Target.getTargets failed: {e}");
                sleep(cfg.poll_interval).await;
                continue;
            }
        };

        let targets = targets_resp
            .get("result")
            .and_then(|r| r.get("targetInfos"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Check for login pages (log once, keep polling)
        if !saw_login {
            for t in &targets {
                let url = t["url"].as_str().unwrap_or("");
                if classify_target(url) == TargetKind::Login {
                    eprintln!("[cdp] X login page detected — waiting for user to log in");
                    saw_login = true;
                    break;
                }
            }
        }

        // Look for the consent page
        if let Some(target) = find_oauth_target(&targets, &cfg.expected_auth_url) {
            // Optionally activate the tab
            let _ = client
                .send(
                    "Target.activateTarget",
                    json!({ "targetId": target.target_id }),
                )
                .await;

            // Attach to the target with flat session
            let attach_resp = match client
                .send(
                    "Target.attachToTarget",
                    json!({ "targetId": target.target_id, "flatten": true }),
                )
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("[cdp] attachToTarget failed: {e} (retry {retry_count})");
                    sleep(cfg.poll_interval).await;
                    continue;
                }
            };

            let session_id = match attach_resp
                .get("result")
                .and_then(|r| r.get("sessionId"))
                .and_then(Value::as_str)
            {
                Some(sid) => sid.to_string(),
                None => {
                    eprintln!("[cdp] attachToTarget returned no sessionId (retry {retry_count})");
                    sleep(cfg.poll_interval).await;
                    continue;
                }
            };

            // Execute the click JS with userGesture and awaitPromise
            let eval_resp = match client
                .send_to_session(
                    &session_id,
                    "Runtime.evaluate",
                    json!({
                        "expression": build_click_js(),
                        "returnByValue": true,
                        "awaitPromise": true,
                        "userGesture": true,
                        "timeout": 5000,
                    }),
                )
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("[cdp] Runtime.evaluate failed: {e} (retry {retry_count})");
                    // Detach session and retry
                    sleep(cfg.poll_interval).await;
                    continue;
                }
            };

            // Parse the result
            let result_str = eval_resp
                .get("result")
                .and_then(|r| r.get("result"))
                .and_then(|r| r.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("");

            let click_result = parse_click_result(result_str);

            if click_result.clicked {
                eprintln!(
                    "[cdp] consent button clicked via {} ({}) after {} polls",
                    click_result.strategy,
                    click_result.label,
                    retry_count,
                );
                return Ok(AutoConsentOutcome::Clicked {
                    strategy: click_result.strategy,
                });
            }

            // Button not found yet — page may still be rendering
            eprintln!("[cdp] consent button not found yet (retry {retry_count})");
        }

        sleep(cfg.poll_interval).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Bind a tokio TcpListener on an OS-assigned port and return (listener, port).
    async fn bind_test_server() -> (tokio::net::TcpListener, u16) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    // ── Unit tests: DevToolsActivePort parsing ──

    #[test]
    fn parse_devtools_active_port_valid_returns_port_and_path() {
        let input = "9222\n/devtools/browser/abc-def-123\n";
        let (port, path) = parse_devtools_active_port(input).unwrap();
        assert_eq!(port, 9222);
        assert_eq!(path, "/devtools/browser/abc-def-123");
    }

    #[test]
    fn parse_devtools_active_port_empty_errors() {
        assert!(parse_devtools_active_port("").is_err());
    }

    #[test]
    fn parse_devtools_active_port_missing_second_line_errors() {
        assert!(parse_devtools_active_port("9222\n").is_err());
    }

    #[test]
    fn parse_devtools_active_port_non_numeric_port_errors() {
        assert!(parse_devtools_active_port("notaport\n/devtools/browser/abc\n").is_err());
    }

    #[test]
    fn parse_devtools_active_port_whitespace_trimmed() {
        let input = " 9222 \n /devtools/browser/xyz \n";
        let (port, path) = parse_devtools_active_port(input).unwrap();
        assert_eq!(port, 9222);
        assert_eq!(path, "/devtools/browser/xyz");
    }

    // ── Unit tests: WebSocket URL construction ──

    #[test]
    fn build_ws_url_formats_correctly() {
        assert_eq!(
            build_ws_url(9222, "/devtools/browser/abc-123"),
            "ws://127.0.0.1:9222/devtools/browser/abc-123"
        );
    }

    #[test]
    fn build_ws_url_different_port() {
        assert_eq!(
            build_ws_url(41323, "/devtools/browser/xyz"),
            "ws://127.0.0.1:41323/devtools/browser/xyz"
        );
    }

    // ── Unit tests: DevToolsActivePort path derivation ──

    #[test]
    fn devtools_active_port_path_derives_from_user_data_dir() {
        let dir = Path::new("/tmp/chrome-oauth-profile");
        let path = devtools_active_port_path(dir);
        assert_eq!(path, PathBuf::from("/tmp/chrome-oauth-profile/DevToolsActivePort"));
    }

    // ── Unit tests: target classification ──

    #[test]
    fn classify_target_detects_consent_urls() {
        assert_eq!(
            classify_target("https://x.com/i/oauth2/authorize?client_id=abc&state=xyz"),
            TargetKind::Consent
        );
        assert_eq!(
            classify_target("https://twitter.com/i/oauth2/authorize?foo=bar"),
            TargetKind::Consent
        );
    }

    #[test]
    fn classify_target_detects_login_urls() {
        assert_eq!(
            classify_target("https://x.com/i/flow/login?redirect_after_login=..."),
            TargetKind::Login
        );
        assert_eq!(
            classify_target("https://x.com/login"),
            TargetKind::Login
        );
    }

    #[test]
    fn classify_target_detects_callback_urls() {
        assert_eq!(
            classify_target("http://localhost:8080/callback?code=abc&state=xyz"),
            TargetKind::Callback
        );
    }

    #[test]
    fn classify_target_returns_other_for_normal_pages() {
        assert_eq!(
            classify_target("https://x.com/home"),
            TargetKind::Other
        );
        assert_eq!(
            classify_target("https://google.com"),
            TargetKind::Other
        );
    }

    // ── Unit tests: target discovery ──

    #[test]
    fn find_oauth_target_matches_exact_authorize_url() {
        let targets = vec![
            json!({"type": "page", "targetId": "t1", "url": "https://x.com/home"}),
            json!({"type": "page", "targetId": "t2", "url": "https://x.com/i/oauth2/authorize?client_id=abc&state=expected123"}),
        ];
        let found = find_oauth_target(
            &targets,
            "https://x.com/i/oauth2/authorize?client_id=abc&state=expected123",
        );
        assert!(found.is_some());
        let info = found.unwrap();
        assert_eq!(info.target_id, "t2");
        assert_eq!(info.kind, TargetKind::Consent);
    }

    #[test]
    fn find_oauth_target_prefers_state_match() {
        let targets = vec![
            json!({"type": "page", "targetId": "old", "url": "https://x.com/i/oauth2/authorize?state=stale"}),
            json!({"type": "page", "targetId": "current", "url": "https://x.com/i/oauth2/authorize?state=fresh"}),
        ];
        let found = find_oauth_target(
            &targets,
            "https://x.com/i/oauth2/authorize?state=fresh",
        );
        assert_eq!(found.unwrap().target_id, "current");
    }

    #[test]
    fn find_oauth_target_ignores_non_page_targets() {
        let targets = vec![
            json!({"type": "background_page", "targetId": "t1", "url": "https://x.com/i/oauth2/authorize?state=abc"}),
            json!({"type": "service_worker", "targetId": "t2", "url": "https://x.com/i/oauth2/authorize?state=abc"}),
        ];
        assert!(find_oauth_target(&targets, "https://x.com/i/oauth2/authorize?state=abc").is_none());
    }

    #[test]
    fn find_oauth_target_returns_none_when_absent() {
        let targets = vec![
            json!({"type": "page", "targetId": "t1", "url": "https://x.com/home"}),
            json!({"type": "page", "targetId": "t2", "url": "https://google.com"}),
        ];
        assert!(find_oauth_target(&targets, "https://x.com/i/oauth2/authorize?state=abc").is_none());
    }

    // ── Unit tests: click JS ──

    #[test]
    fn build_click_js_contains_wait_logic_and_structured_return() {
        let js = build_click_js();
        // Must be an async IIFE
        assert!(js.contains("async function()"));
        // Must wait for DOM with setTimeout polling
        assert!(js.contains("setTimeout"));
        // Must try data-testid strategy
        assert!(js.contains("OAuth_Consent_Button"));
        // Must try text strategies
        assert!(js.contains("authorize"));
        assert!(js.contains("allow"));
        // Must return structured JSON
        assert!(js.contains("JSON.stringify"));
        assert!(js.contains("clicked"));
        assert!(js.contains("strategy"));
    }

    // ── Unit tests: click result parsing ──

    #[test]
    fn parse_click_result_valid_clicked() {
        let result = parse_click_result(r#"{"clicked":true,"strategy":"data-testid","label":"Authorize app"}"#);
        assert!(result.clicked);
        assert_eq!(result.strategy, "data-testid");
        assert_eq!(result.label, "Authorize app");
    }

    #[test]
    fn parse_click_result_not_found() {
        let result = parse_click_result(r#"{"clicked":false,"strategy":"none","label":""}"#);
        assert!(!result.clicked);
        assert_eq!(result.strategy, "none");
    }

    #[test]
    fn parse_click_result_invalid_json() {
        let result = parse_click_result("not json at all");
        assert!(!result.clicked);
        assert_eq!(result.strategy, "parse-error");
    }

    // ── Unit tests: query param extraction ──

    #[test]
    fn extract_query_param_finds_state() {
        let url = "https://x.com/i/oauth2/authorize?client_id=abc&state=xyz123&scope=read";
        assert_eq!(extract_query_param(url, "state"), Some("xyz123".to_string()));
        assert_eq!(extract_query_param(url, "client_id"), Some("abc".to_string()));
        assert_eq!(extract_query_param(url, "missing"), None);
    }

    // ── Integration tests: CDP message correlation ──

    #[tokio::test]
    async fn cdp_client_correlates_response_ids_while_ignoring_events() {
        let (listener, port) = bind_test_server().await;
        let ws_url = format!("ws://127.0.0.1:{port}");

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            // Read the client's request
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            let req_id = req["id"].as_u64().unwrap();

            // Send an unsolicited event first (should be ignored by client)
            let event = json!({"method": "Target.targetCreated", "params": {"targetInfo": {}}});
            ws.send(Message::Text(event.to_string().into())).await.unwrap();

            // Then send the actual response
            let resp = json!({"id": req_id, "result": {"targetInfos": []}});
            ws.send(Message::Text(resp.to_string().into())).await.unwrap();
        });

        // Give server a moment to bind
        sleep(Duration::from_millis(50)).await;

        let mut client = CdpClient::connect(&ws_url).await.unwrap();
        let resp = client.send("Target.getTargets", json!({})).await.unwrap();

        // Should have received the response, ignoring the event
        assert!(resp["result"]["targetInfos"].is_array());

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn send_to_session_includes_session_id() {
        let (listener, port) = bind_test_server().await;
        let ws_url = format!("ws://127.0.0.1:{port}");

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();

            // Verify the sessionId was included in the request
            assert_eq!(req["sessionId"].as_str().unwrap(), "session-abc-123");
            assert_eq!(req["method"].as_str().unwrap(), "Runtime.evaluate");

            let resp = json!({"id": req["id"].as_u64().unwrap(), "result": {"result": {"value": "ok"}}});
            ws.send(Message::Text(resp.to_string().into())).await.unwrap();
        });

        sleep(Duration::from_millis(50)).await;

        let mut client = CdpClient::connect(&ws_url).await.unwrap();
        let resp = client
            .send_to_session("session-abc-123", "Runtime.evaluate", json!({"expression": "1+1"}))
            .await
            .unwrap();
        assert!(resp.get("result").is_some());

        server_handle.await.unwrap();
    }

    // ── Integration tests: full auto-consent flow with fake CDP server ──

    #[tokio::test]
    async fn happy_path_get_targets_attach_evaluate_clicked() {
        let (listener, port) = bind_test_server().await;

        let tmp_dir = std::env::temp_dir().join(format!("cdp-test-happy-{}", port));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;
        let port_file = tmp_dir.join("DevToolsActivePort");
        tokio::fs::write(&port_file, format!("{port}\n/devtools/browser/fake\n"))
            .await
            .unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            // 1. Target.getTargets
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            let resp = json!({
                "id": req["id"],
                "result": {
                    "targetInfos": [{
                        "type": "page",
                        "targetId": "target-1",
                        "url": "https://x.com/i/oauth2/authorize?state=test123"
                    }]
                }
            });
            ws.send(Message::Text(resp.to_string().into())).await.unwrap();

            // 2. Target.activateTarget
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({"id": req["id"], "result": {}}).to_string().into()))
                .await
                .unwrap();

            // 3. Target.attachToTarget
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            assert_eq!(req["params"]["flatten"], true);
            let resp = json!({"id": req["id"], "result": {"sessionId": "sess-1"}});
            ws.send(Message::Text(resp.to_string().into())).await.unwrap();

            // 4. Runtime.evaluate
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            assert_eq!(req["sessionId"], "sess-1");
            assert_eq!(req["params"]["userGesture"], true);
            assert_eq!(req["params"]["awaitPromise"], true);
            let resp = json!({
                "id": req["id"],
                "result": {
                    "result": {
                        "value": r#"{"clicked":true,"strategy":"data-testid","label":"Authorize app"}"#
                    }
                }
            });
            ws.send(Message::Text(resp.to_string().into())).await.unwrap();
        });

        let cfg = AutoConsentConfig {
            expected_auth_url: "https://x.com/i/oauth2/authorize?state=test123".to_string(),
            chrome_user_data_dir: tmp_dir.clone(),
            overall_timeout: Duration::from_secs(10),
            poll_interval: Duration::from_millis(100),
        };

        let outcome = auto_click_oauth_consent(cfg).await.unwrap();
        assert_eq!(
            outcome,
            AutoConsentOutcome::Clicked {
                strategy: "data-testid".to_string()
            }
        );

        server_handle.await.unwrap();
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn login_page_first_then_consent_page_later() {
        let (listener, port) = bind_test_server().await;

        let tmp_dir = std::env::temp_dir().join(format!("cdp-test-login-{}", port));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;
        tokio::fs::write(
            tmp_dir.join("DevToolsActivePort"),
            format!("{port}\n/devtools/browser/fake\n"),
        )
        .await
        .unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            // Poll 1: login page only
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({
                "id": req["id"],
                "result": {"targetInfos": [
                    {"type": "page", "targetId": "t1", "url": "https://x.com/i/flow/login"}
                ]}
            }).to_string().into())).await.unwrap();

            // Poll 2: consent page appears
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({
                "id": req["id"],
                "result": {"targetInfos": [
                    {"type": "page", "targetId": "t2", "url": "https://x.com/i/oauth2/authorize?state=abc"}
                ]}
            }).to_string().into())).await.unwrap();

            // activateTarget
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({"id": req["id"], "result": {}}).to_string().into()))
                .await.unwrap();

            // attachToTarget
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({"id": req["id"], "result": {"sessionId": "s1"}}).to_string().into()))
                .await.unwrap();

            // Runtime.evaluate
            let msg = ws.next().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({
                "id": req["id"],
                "result": {"result": {"value": r#"{"clicked":true,"strategy":"text-authorize-app","label":"Authorize app"}"#}}
            }).to_string().into())).await.unwrap();
        });

        let cfg = AutoConsentConfig {
            expected_auth_url: "https://x.com/i/oauth2/authorize?state=abc".to_string(),
            chrome_user_data_dir: tmp_dir.clone(),
            overall_timeout: Duration::from_secs(10),
            poll_interval: Duration::from_millis(100),
        };

        let outcome = auto_click_oauth_consent(cfg).await.unwrap();
        assert!(matches!(outcome, AutoConsentOutcome::Clicked { .. }));

        server_handle.await.unwrap();
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    #[cfg(not(feature = "cdp_live_test"))]
    async fn devtools_active_port_missing_returns_manual_fallback() {
        // This test verifies that when DevToolsActivePort is missing AND no
        // Chrome is listening on port 9222, the outcome is ManualFallback or
        // TimedOut. On machines with Chrome Debug running (port 9222 open),
        // the HTTP discovery succeeds and the test would connect to real Chrome,
        // so we gate it behind a feature flag for CI environments.
        let tmp_dir = std::env::temp_dir().join("cdp-test-missing-port");
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        let cfg = AutoConsentConfig {
            expected_auth_url: "https://x.com/i/oauth2/authorize?state=abc".to_string(),
            chrome_user_data_dir: tmp_dir.clone(),
            overall_timeout: Duration::from_millis(500),
            poll_interval: Duration::from_millis(100),
        };

        let outcome = auto_click_oauth_consent(cfg).await.unwrap();
        // Either ManualFallback (no Chrome) or TimedOut (Chrome found but no matching tab)
        assert!(
            matches!(outcome, AutoConsentOutcome::ManualFallback(_) | AutoConsentOutcome::TimedOut),
            "expected ManualFallback or TimedOut, got {:?}",
            outcome,
        );

        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn no_matching_target_before_deadline_returns_timeout() {
        let (listener, port) = bind_test_server().await;

        let tmp_dir = std::env::temp_dir().join(format!("cdp-test-timeout-{}", port));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;
        tokio::fs::write(
            tmp_dir.join("DevToolsActivePort"),
            format!("{port}\n/devtools/browser/fake\n"),
        )
        .await
        .unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            // Keep returning empty targets until client gives up
            loop {
                let msg = ws.next().await;
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let req: Value = serde_json::from_str(&text).unwrap();
                        ws.send(Message::Text(json!({
                            "id": req["id"],
                            "result": {"targetInfos": [
                                {"type": "page", "targetId": "t1", "url": "https://x.com/home"}
                            ]}
                        }).to_string().into())).await.unwrap();
                    }
                    _ => break,
                }
            }
        });

        let cfg = AutoConsentConfig {
            expected_auth_url: "https://x.com/i/oauth2/authorize?state=abc".to_string(),
            chrome_user_data_dir: tmp_dir.clone(),
            overall_timeout: Duration::from_millis(800),
            poll_interval: Duration::from_millis(100),
        };

        let outcome = auto_click_oauth_consent(cfg).await.unwrap();
        assert_eq!(outcome, AutoConsentOutcome::TimedOut);

        server_handle.abort();
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn cancellation_after_callback_task_exits_cleanly() {
        let (listener, port) = bind_test_server().await;

        let tmp_dir = std::env::temp_dir().join(format!("cdp-test-cancel-{}", port));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;
        tokio::fs::write(
            tmp_dir.join("DevToolsActivePort"),
            format!("{port}\n/devtools/browser/fake\n"),
        )
        .await
        .unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            // Respond slowly — keep returning empty targets
            loop {
                let msg = ws.next().await;
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let req: Value = serde_json::from_str(&text).unwrap();
                        ws.send(Message::Text(json!({
                            "id": req["id"],
                            "result": {"targetInfos": []}
                        }).to_string().into())).await.unwrap();
                    }
                    _ => break,
                }
            }
        });

        let cfg = AutoConsentConfig {
            expected_auth_url: "https://x.com/i/oauth2/authorize?state=abc".to_string(),
            chrome_user_data_dir: tmp_dir.clone(),
            overall_timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(100),
        };

        // Spawn the auto-consent task
        let handle = tokio::spawn(auto_click_oauth_consent(cfg));

        // Simulate: callback arrives after 300ms, abort the CDP task
        sleep(Duration::from_millis(300)).await;
        handle.abort();

        // The abort should not panic
        let result = handle.await;
        assert!(result.is_err()); // JoinError::Cancelled

        server_handle.abort();
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }
}
