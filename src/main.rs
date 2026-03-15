use std::{env, fs, path::Path, sync::Arc, time::Duration, time::SystemTime};
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use base64::engine::{general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use x_bookmarks_pipeline_rust::{
    cache::BookmarkCache,
    cli::{self, CliArgs},
    config::AppConfig,
    models::Bookmark,
    fetcher::XBookmarkFetcher,
    llm::{CerebrasProvider, ClaudeProvider, LLMProvider, OpenAIProvider, XaiProvider},
    models::PipelineResult as PipelineResultModel,
    notify::{EmailConfig, SmtpNotifier},
    orchestrator::{OnMetaSaved, Pipeline},
};
use x_bookmarks_pipeline_rust::error::PipelineError;

#[derive(Clone, Debug)]
struct XRefreshConfig {
    client_id: String,
    client_secret: Option<String>,
    refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OAuthState {
    code_verifier: String,
    state: String,
    redirect_uri: String,
}

const OAUTH_STATE_FILE: &str = ".x-bookmarks-oauth.json";
const OAUTH_AUTH_URL: &str = "https://x.com/i/oauth2/authorize";

fn env_any(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
        .map(|value| value.trim().to_string())
}

fn env_flag(names: &[&str]) -> bool {
    match env_any(names).as_deref() {
        Some(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => false,
    }
}

fn env_u64(names: &[&str], default: u64) -> u64 {
    env_any(names)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize(names: &[&str], default: usize) -> usize {
    env_any(names)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn require_env(name: &str, aliases: &[&str]) -> anyhow::Result<String> {
    env_any(&std::iter::once(name).chain(aliases.iter().copied()).collect::<Vec<_>>())
        .with_context(|| format!("missing required env var {name}"))
}

fn load_refresh_config() -> Option<XRefreshConfig> {
    let refresh_token = env_any(&["X_REFRESH_TOKEN", "XPB_X_REFRESH_TOKEN"])?;
    let client_id = env_any(&["X_CLIENT_ID", "XPB_X_CLIENT_ID"])?;
    let client_secret = env_any(&["X_CLIENT_SECRET", "XPB_X_CLIENT_SECRET"]);
    Some(XRefreshConfig {
        client_id,
        client_secret,
        refresh_token,
    })
}

fn generate_nonce(len: usize) -> String {
    let seed = {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!(
            "{}-{}-{:?}",
            now.as_nanos(),
            std::process::id(),
            std::thread::current().id()
        )
    };
    let hash = Sha256::digest(seed.as_bytes());
    let mut nonce = String::with_capacity(hash.len() * 2);
    for byte in hash {
        nonce.push_str(&format!("{:02x}", byte));
    }
    if len <= nonce.len() {
        nonce.truncate(len);
    } else {
        while nonce.len() < len {
            nonce.push('a');
        }
    }
    nonce
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let is_unreserved = matches!(
            byte,
            b'a'..=b'z'
                | b'A'..=b'Z'
                | b'0'..=b'9'
                | b'-'
                | b'.'
                | b'_'
                | b'~'
        );
        if is_unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn oauth_scope() -> String {
    env_any(&["X_OAUTH_SCOPE", "XPB_X_OAUTH_SCOPE"])
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "tweet.read users.read bookmark.read offline.access".to_string())
}

fn load_oauth_client(
    redirect_uri_override: Option<&str>,
) -> anyhow::Result<(String, Option<String>, String)> {
    let client_id = env_any(&["X_CLIENT_ID", "XPB_X_CLIENT_ID"])
        .ok_or_else(|| anyhow::anyhow!("missing X_CLIENT_ID/XPB_X_CLIENT_ID for OAuth"))?;
    let client_secret = env_any(&["X_CLIENT_SECRET", "XPB_X_CLIENT_SECRET"]);
    let redirect_uri = redirect_uri_override
        .map(ToString::to_string)
        .or_else(|| {
            env_any(&[
        "X_REDIRECT_URI",
        "XPB_X_REDIRECT_URI",
        "X_OAUTH_REDIRECT_URI",
        "XPB_X_OAUTH_REDIRECT_URI",
            ])
            .or_else(|| Some("http://localhost:8765/callback".to_string()))
        })
        .ok_or_else(|| anyhow::anyhow!("missing redirect uri for OAuth flow"))?;
    Ok((client_id, client_secret, redirect_uri))
}

fn load_oauth_state() -> anyhow::Result<OAuthState> {
    let path = Path::new(OAUTH_STATE_FILE);
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str::<OAuthState>(&raw)?)
}

fn write_oauth_state(state: &OAuthState) -> anyhow::Result<()> {
    let path = Path::new(OAUTH_STATE_FILE);
    fs::write(path, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

fn clear_oauth_state() {
    let path = Path::new(OAUTH_STATE_FILE);
    let _ = fs::remove_file(path);
}

fn build_oauth_authorization_url(
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
) -> (String, OAuthState) {
    let code_verifier = generate_nonce(64);
    let state = generate_nonce(24);
    let code_challenge = URL_SAFE_NO_PAD.encode(
        Sha256::digest(code_verifier.as_bytes())
            .as_slice(),
    );

    let url = format!(
        "{OAUTH_AUTH_URL}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        client_id,
        url_encode(redirect_uri),
        url_encode(scope),
        state,
        code_challenge
    );

    (
        url,
        OAuthState {
            code_verifier,
            state,
            redirect_uri: redirect_uri.to_string(),
        },
    )
}

fn open_in_browser(url: &str) -> bool {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(url)
            .status()
    } else {
        Command::new("xdg-open").arg(url).status()
    };

    status.map(|status| status.success()).unwrap_or(false)
}

#[derive(Debug)]
struct LoopbackRedirect {
    host: String,
    port: u16,
    path: String,
}

fn parse_local_redirect(redirect_uri: &str) -> Option<LoopbackRedirect> {
    let (_scheme, rest) = redirect_uri.split_once("://")?;
    let is_localhost = redirect_uri.starts_with("http://localhost")
        || redirect_uri.starts_with("https://localhost")
        || redirect_uri.starts_with("http://127.0.0.1")
        || redirect_uri.starts_with("https://127.0.0.1");
    if is_localhost && redirect_uri.starts_with("https://") {
        eprintln!("Localhost OAuth callback uses HTTPS in redirect_uri; automatic callback capture will be unavailable.");
    }
    if is_localhost {
        let (host_port, path) = if let Some((head, tail)) = rest.split_once('/') {
            (head, format!("/{}", tail))
        } else {
            (rest, "/".to_string())
        };
        let default_port = if redirect_uri.starts_with("https://") { 443 } else { 80 };
        let (host, port) = if let Some((host, port_text)) = host_port.split_once(':') {
            (host.to_string(), port_text.parse::<u16>().unwrap_or(8765))
        } else {
            (host_port.to_string(), default_port)
        };
        Some(LoopbackRedirect { host, port, path })
    } else {
        None
    }
}

fn query_param(path: &str, key: &str) -> Option<String> {
    for pair in path.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(v.to_string());
        }
    }
    None
}

async fn wait_for_oauth_code(
    redirect_uri: &str,
    expected_state: &str,
) -> anyhow::Result<Option<String>> {
    let callback = match parse_local_redirect(redirect_uri) {
        Some(callback) => callback,
        None => return Ok(None),
    };

    let listener = TcpListener::bind((callback.host.as_str(), callback.port)).await?;
    let auth_result = tokio::time::timeout(Duration::from_secs(120), async move {
        loop {
            let (mut stream, _addr) = listener.accept().await?;
            let mut buffer = [0u8; 4096];
            let n = stream.read(&mut buffer).await?;
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let request_target = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");
            let (request_path, request_query) = request_target.split_once('?').map_or(
                (request_target, ""),
                |(path, query)| (path, query),
            );
            if request_path == callback.path {
                let state = query_param(request_query, "state");
                if state.as_deref() != Some(expected_state) {
                    let body = "<html><body>OAuth state mismatch. Try re-running with --auth-url.</body></html>";
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    return Err(anyhow::anyhow!("OAuth state mismatch"));
                }
                if let Some(error) = query_param(request_query, "error") {
                    return Err(anyhow::anyhow!("OAuth returned error: {error}"));
                }
                if let Some(code) = query_param(request_query, "code") {
                    let body = "<html><body>Authorization code received. You can return to the terminal.</body></html>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    return Ok(Some(code.to_string()));
                }
            }
            let body = "<html><body>Waiting for OAuth redirect.</body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    })
    .await;

    match auth_result {
        Ok(result) => result,
        Err(_) => Ok(None),
    }
}

fn emit_auth_flow_instructions(
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    verifier: &str,
    url: &str,
) {
    println!("Open this URL in a browser to reauthenticate:");
    let opened = open_in_browser(url);
    if opened {
        println!("(browser launch attempted)");
    } else {
        println!("(automatic browser launch unavailable)");
    }
    println!("{url}");
    println!();
    println!("After authorization, copy the returned ?code= value and run:");
    println!("cargo run -- --auth-code '<code>' --auth-code-verifier {verifier}");
    println!("Using client_id={client_id}");
    println!("Using redirect_uri={redirect_uri}");
    println!("Using scope={scope}");
    println!(
        "Or rerun with: cargo run -- --auth-url --auth-redirect-uri '{redirect_uri}'"
    );
}

async fn start_interactive_reauth_flow(
    args: &CliArgs,
    http: &Client,
) -> anyhow::Result<bool> {
    let (client_id, client_secret, redirect_uri) = load_oauth_client(args.auth_redirect_uri.as_deref())?;
    let scope = oauth_scope();
    let (url, state) = build_oauth_authorization_url(&client_id, &redirect_uri, &scope);
    write_oauth_state(&state)?;
    emit_auth_flow_instructions(&client_id, &redirect_uri, &scope, &state.code_verifier, &url);

    let code = wait_for_oauth_code(&redirect_uri, &state.state).await?;
    let Some(code) = code else {
        return Ok(false);
    };

    let (access_token, refresh_token) = exchange_authorization_code(
        http,
        &code,
        &state.code_verifier,
        &redirect_uri,
        &client_id,
        client_secret,
    )
    .await?;

    let _ = persist_refreshed_access_token(&access_token, refresh_token.as_deref());
    env::set_var("X_BEARER_TOKEN", &access_token);
    env::set_var("X_ACCESS_TOKEN", &access_token);
    env::set_var("X_USER_ACCESS_TOKEN", &access_token);
    if let Some(refresh_token) = refresh_token.as_deref() {
        env::set_var("X_REFRESH_TOKEN", refresh_token);
        env::set_var("XPB_X_REFRESH_TOKEN", refresh_token);
    }
    clear_oauth_state();
    println!("OAuth exchange succeeded and token updated.");
    Ok(true)
}

fn normalize_x_username(raw: &str) -> String {
    raw.trim().trim_start_matches('@').trim().to_string()
}

async fn resolve_fetch_user_id(
    client: &Client,
    token: &str,
    username: &str,
) -> anyhow::Result<String> {
    let response = client
        .get(format!("https://api.x.com/2/users/by/username/{username}"))
        .bearer_auth(token)
        .send()
        .await?;

    let status = response.status();
    let payload = response.text().await?;

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "failed to resolve username {username} ({status}): {payload}"
        ));
    }

    let body: Value = serde_json::from_str(&payload)?;
    if let Some(id) = body
        .get("data")
        .and_then(|data| data.get("id"))
        .and_then(Value::as_str)
    {
        return Ok(id.to_string());
    }

    if let Some(message) = body
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("message").and_then(Value::as_str))
    {
        return Err(anyhow::anyhow!("failed to resolve username {username}: {message}"));
    }

    Err(anyhow::anyhow!("failed to resolve username {username}: no user id returned"))
}

fn is_auth_expired_error(err: &anyhow::Error) -> bool {
    let error = err.to_string().to_lowercase();
    error.contains("authentication") || error.contains("unsupported authentication") || error.contains("expired") || error.contains("forbidden") || error.contains("unauthorized")
}

async fn refresh_x_access_token(
    client: &Client,
    refresh_config: &mut XRefreshConfig,
) -> anyhow::Result<String> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_config.refresh_token.as_str()),
        ("client_id", refresh_config.client_id.as_str()),
    ];
    if let Some(secret) = refresh_config.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }

    let response = client
        .post("https://api.x.com/2/oauth2/token")
        .form(&form)
        .send()
        .await?;

    let status = response.status();
    let payload = response.text().await?;

    if status != StatusCode::OK {
        return Err(anyhow::anyhow!(
            "token refresh failed ({status}): {payload}"
        ));
    }

    let body: Value = serde_json::from_str(&payload)?;

    if let Some(error) = body.get("error").and_then(Value::as_str) {
        let details = body
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(error);
        return Err(anyhow::anyhow!("token refresh rejected: {details}"));
    }

    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("token refresh response missing access_token"))?;

    if let Some(new_refresh) = body
        .get("refresh_token")
        .and_then(Value::as_str)
    {
        refresh_config.refresh_token = new_refresh.to_string();
    }

    Ok(access_token)
}

async fn exchange_authorization_code(
    client: &Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: Option<String>,
) -> anyhow::Result<(String, Option<String>)> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }

    let response = client
        .post("https://api.x.com/2/oauth2/token")
        .form(&form)
        .send()
        .await?;

    let status = response.status();
    let payload = response.text().await?;

    if status != StatusCode::OK {
        return Err(anyhow::anyhow!("token exchange failed ({status}): {payload}"));
    }

    let body: Value = serde_json::from_str(&payload)?;
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        let details = body
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(error);
        return Err(anyhow::anyhow!("token exchange rejected: {details}"));
    }

    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("token exchange response missing access_token"))?;

    let refresh_token = body.get("refresh_token").and_then(Value::as_str).map(ToString::to_string);

    Ok((access_token, refresh_token))
}

async fn acquire_access_token(
    client: &Client,
    refresh_config: &mut Option<XRefreshConfig>,
    configured_token: Option<String>,
    force_refresh: bool,
) -> anyhow::Result<(String, bool)> {
    let token = if let Some(token) = configured_token {
        token
    } else if let Some(cfg) = refresh_config {
        return Ok((refresh_x_access_token(client, cfg).await?, true));
    } else {
        return Err(anyhow::anyhow!(
            "missing required X API bearer token (set X_BEARER_TOKEN/X_ACCESS_TOKEN/X_USER_ACCESS_TOKEN, or configure X_REFRESH_TOKEN + X_CLIENT_ID)"
        ));
    };

    if force_refresh {
        let cfg = refresh_config.as_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "reauth requested but refresh credentials are unavailable (set X_REFRESH_TOKEN/X_CLIENT_ID)"
            )
        })?;
        return Ok((refresh_x_access_token(client, cfg).await?, true));
    }

    if let Some(cfg) = refresh_config {
        match refresh_x_access_token(client, cfg).await {
            Ok(fresh_token) => return Ok((fresh_token, true)),
            Err(refresh_error) => {
                eprintln!(
                    "token refresh failed during preflight ({refresh_error}); using existing configured token"
                );
            }
        }
    }

    Ok((token, false))
}

fn persist_refreshed_access_token(
    access_token: &str,
    refresh_token: Option<&str>,
) -> anyhow::Result<bool> {
    let path = Path::new(".env");
    if !path.exists() {
        return Ok(false);
    }

    let raw = fs::read_to_string(path)?;
    let access_keys = [
        "X_BEARER_TOKEN",
        "X_ACCESS_TOKEN",
        "X_USER_ACCESS_TOKEN",
        "XPB_X_BEARER_TOKEN",
        "XPB_X_ACCESS_TOKEN",
        "XPB_X_USER_ACCESS_TOKEN",
    ];
    let refresh_keys = [
        "X_REFRESH_TOKEN",
        "XPB_X_REFRESH_TOKEN",
    ];

    let mut lines: Vec<String> = Vec::new();
    let mut access_touched = false;
    let mut refresh_touched = false;
    for line in raw.lines() {
        let mut replaced = false;
        if let Some((raw_key, _)) = line.split_once('=') {
            let key = raw_key.trim();
            if access_keys.contains(&key) {
                lines.push(format!("{key}={access_token}"));
                access_touched = true;
                replaced = true;
            }
            if let Some(refresh_token) = refresh_token {
                if refresh_keys.contains(&key) {
                    lines.push(format!("{key}={refresh_token}"));
                    refresh_touched = true;
                    replaced = true;
                }
            }
        }
        if !replaced {
            lines.push(line.to_string());
        }
    }

    if !access_touched {
        lines.push(format!("X_BEARER_TOKEN={access_token}"));
    }

    if let Some(refresh_token) = refresh_token {
        if !refresh_touched {
            lines.push(format!("X_REFRESH_TOKEN={refresh_token}"));
        }
    }

    fs::write(path, format!("{}\n", lines.join("\n")))?;
    env::set_var("X_BEARER_TOKEN", access_token);
    env::set_var("X_ACCESS_TOKEN", access_token);
    env::set_var("X_USER_ACCESS_TOKEN", access_token);
    if let Some(refresh_token) = refresh_token {
        env::set_var("X_REFRESH_TOKEN", refresh_token);
        env::set_var("XPB_X_REFRESH_TOKEN", refresh_token);
    }
    Ok(true)
}

async fn fetch_bookmarks_with_refresh(
    fetcher: &XBookmarkFetcher,
    fetch_client: &Client,
    refresh_config: &mut Option<XRefreshConfig>,
    args: &CliArgs,
) -> anyhow::Result<Vec<Bookmark>> {
    match fetcher.fetch().await {
        Ok(bookmarks) => Ok(bookmarks),
        Err(PipelineError::TokenExpired { .. }) if refresh_config.is_some() => {
            if let Some(cfg) = refresh_config {
                match refresh_x_access_token(fetch_client, cfg).await {
                    Ok(token) => {
                        fetcher.set_access_token(token).await;
                        return fetcher.fetch().await.map_err(Into::into);
                    }
                    Err(refresh_error) => {
                        eprintln!(
                            "token refresh failed while retrying expired token ({refresh_error}); launching browser login"
                        );
                        if start_interactive_reauth_flow(args, fetch_client).await? {
                            if let Some(token) = env_any(&[
                                "X_BEARER_TOKEN",
                                "X_ACCESS_TOKEN",
                                "X_USER_ACCESS_TOKEN",
                                "XPB_X_BEARER_TOKEN",
                                "XPB_X_USER_ACCESS_TOKEN",
                            ]) {
                                fetcher.set_access_token(token).await;
                            }
                            return fetcher.fetch().await.map_err(Into::into);
                        }
                        return Err(anyhow::anyhow!(
                            "token refresh failed while retrying expired token ({refresh_error})"
                        ));
                    }
                }
            }
            if start_interactive_reauth_flow(args, fetch_client).await? {
                if let Some(token) = env_any(&[
                    "X_BEARER_TOKEN",
                    "X_ACCESS_TOKEN",
                    "X_USER_ACCESS_TOKEN",
                    "XPB_X_BEARER_TOKEN",
                    "XPB_X_USER_ACCESS_TOKEN",
                ]) {
                    fetcher.set_access_token(token).await;
                }
                return fetcher.fetch().await.map_err(Into::into);
            }
            Err(PipelineError::TokenExpired {
                details: "authentication token expired".to_string(),
            }
            .into())
        }
        Err(err) => Err(err.into()),
    }
}

async fn preflight_refresh_fetcher_token(
    fetcher: &XBookmarkFetcher,
    fetch_client: &Client,
    refresh_config: &mut Option<XRefreshConfig>,
    require_fresh: bool,
    args: &CliArgs,
) -> anyhow::Result<bool> {
    let current_token = fetcher.get_access_token().await;
    if current_token.is_empty() {
        return Err(anyhow::anyhow!(
            "fetcher is missing access token and token refresh flow could not be initialized"
        ));
    }

    if is_access_token_valid(fetch_client, &current_token).await? {
        return Ok(false);
    }

    let cfg = refresh_config
        .as_mut()
        .ok_or_else(|| {
            if require_fresh {
                anyhow::anyhow!(
                    "reauth requested but token is invalid/expired and refresh credentials were not provided"
                )
            } else {
                anyhow::anyhow!(
                    "existing token is invalid or expired and no refresh credentials were provided"
                )
            }
        })?;

    let new_token = match refresh_x_access_token(fetch_client, cfg).await {
        Ok(token) => token,
        Err(err) => {
            if args.reauth {
                eprintln!(
                    "reauth required before processing, but refresh failed ({err}); launching browser login"
                );
                if start_interactive_reauth_flow(args, fetch_client).await? {
                    return Ok(false);
                }
                return Err(anyhow::anyhow!(
                    "reauth required before processing, but refresh failed ({err})"
                ));
            }
            eprintln!("preflight token refresh failed ({err}); launching browser login");
            if start_interactive_reauth_flow(args, fetch_client).await? {
                return Ok(false);
            }
            return Err(anyhow::anyhow!(
                "authentication check failed for fetcher token ({err})"
            ));
        }
    };
    fetcher.set_access_token(new_token.clone()).await;
    if is_access_token_valid(fetch_client, &new_token).await.is_err() {
        return Err(anyhow::anyhow!(
            "fresh token refresh failed validation; token may be missing required bookmark scope"
        ));
    }

    if require_fresh {
        let _ = persist_refreshed_access_token(&new_token, None);
    }

    Ok(false)
}

async fn ensure_cli_authentication(
    args: &CliArgs,
    http: &Client,
    refresh_config: &mut Option<XRefreshConfig>,
) -> anyhow::Result<()> {
    if args.auth_url || args.auth_code.is_some() {
        return Ok(());
    }

    let token = env_any(&[
        "X_BEARER_TOKEN",
        "X_ACCESS_TOKEN",
        "X_USER_ACCESS_TOKEN",
        "XPB_X_BEARER_TOKEN",
        "XPB_X_ACCESS_TOKEN",
        "XPB_X_USER_ACCESS_TOKEN",
    ]);

    let authenticated = if let Some(token) = token {
        match is_access_token_valid(http, &token).await {
            Ok(true) => true,
            Ok(false) => false,
            Err(err) if is_auth_expired_error(&err) => false,
            Err(err) => return Err(err),
        }
    } else {
        false
    };

    if authenticated {
        return Ok(());
    }

    if let Some(cfg) = refresh_config.as_mut() {
        match refresh_x_access_token(http, cfg).await {
            Ok(fresh_token) => {
                let _ = persist_refreshed_access_token(&fresh_token, Some(&cfg.refresh_token));
                println!("refreshed token during auth gate");
                return Ok(());
            }
            Err(err) => {
                eprintln!("token refresh failed during auth gate ({err}); launching browser reauth");
            }
        }
    }

    eprintln!("authentication required but token is missing/expired; launching browser reauth");
    if start_interactive_reauth_flow(args, http).await? {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "authentication required; use the printed browser auth URL and rerun with --auth-code"
    ))
}

async fn is_access_token_valid(client: &Client, token: &str) -> anyhow::Result<bool> {
    let response = client
        .get("https://api.x.com/2/users/me")
        .bearer_auth(token)
        .send()
        .await?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Ok(false);
    }
    if !status.is_success() {
        let payload = response.text().await?;
        return Err(anyhow::anyhow!("token validation failed ({status}): {payload}"));
    }
    Ok(true)
}

async fn resolve_fetch_user_id_with_refresh(
    client: &Client,
    username: &str,
    token: &mut String,
    refresh_config: &mut Option<XRefreshConfig>,
) -> anyhow::Result<String> {
    match resolve_fetch_user_id(client, token, username).await {
        Ok(user_id) => Ok(user_id),
        Err(err) => {
            if refresh_config.is_some() && is_auth_expired_error(&err) {
                if let Some(config) = refresh_config {
                    return match refresh_x_access_token(client, config).await {
                        Ok(new_token) => {
                            *token = new_token;
                            resolve_fetch_user_id(client, token, username).await
                        }
                        Err(refresh_error) => {
                            eprintln!("token refresh failed: {refresh_error}; trying existing token");
                            resolve_fetch_user_id(client, token, username)
                                .await
                                .with_context(|| {
                                    format!("token refresh failed and existing token fallback failed: {refresh_error}")
                                })
                        }
                    };
                }
                return Err(err);
            }
            Err(err)
        }
    }
}

fn build_notifier() -> Option<Arc<SmtpNotifier>> {
    match (
        env_any(&["SMTP_HOST", "XPB_SMTP_HOST"]),
        env_any(&["SMTP_USER", "XPB_SMTP_USER"]),
        env_any(&["SMTP_PASSWORD", "SMTP_PASS", "XPB_SMTP_PASSWORD"]),
        env_any(&["SMTP_FROM", "EMAIL_FROM", "XPB_SMTP_FROM"]),
        env_any(&["SMTP_TO", "EMAIL_TO", "XPB_SMTP_TO"]),
    ) {
        (Some(host), Some(user), Some(password), Some(from), Some(to)) => Some(Arc::new(SmtpNotifier::new(EmailConfig {
            smtp_host: host,
            smtp_user: user,
            smtp_password: password,
            from,
            to,
        }))),
        _ => None,
    }
}

async fn build_fetcher(
    args: &CliArgs,
    cfg: &AppConfig,
    refresh_config: &mut Option<XRefreshConfig>,
) -> anyhow::Result<Option<XBookmarkFetcher>> {
    if args.fetch || env_flag(&["XPB_FETCH_LOOP", "DAEMON_FETCH"]) {
        let token = env_any(&[
            "X_BEARER_TOKEN",
            "X_ACCESS_TOKEN",
            "X_USER_ACCESS_TOKEN",
            "XPB_X_BEARER_TOKEN",
            "XPB_X_USER_ACCESS_TOKEN",
        ]);

        let client = Client::builder()
            .timeout(Duration::from_secs(cfg.fetch_timeout.round() as u64))
            .build()?;

        let (mut access_token, refreshed_from_reauth) =
            match acquire_access_token(&client, refresh_config, token, args.reauth).await {
                Ok(result) => result,
                Err(err) if args.reauth => {
                    eprintln!("reauth token refresh failed ({err}); launching browser login");
                    if start_interactive_reauth_flow(args, &client).await? {
                        (
                            env_any(&[
                                "X_BEARER_TOKEN",
                                "X_ACCESS_TOKEN",
                                "X_USER_ACCESS_TOKEN",
                                "XPB_X_BEARER_TOKEN",
                                "XPB_X_USER_ACCESS_TOKEN",
                            ])
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "browser reauth completed, but no access token is now configured"
                                )
                            })?,
                            true,
                        );
                    }
                    return Err(err);
                }
                Err(err) => return Err(err),
            };
        if args.reauth {
            match persist_refreshed_access_token(
                &access_token,
                refresh_config.as_ref().map(|cfg| cfg.refresh_token.as_str()),
            ) {
                Ok(true) => println!("reauth completed and token persisted to .env"),
                Ok(false) => {
                    if refreshed_from_reauth {
                        println!("reauth completed for this process (no .env file found to update)");
                    }
                }
                Err(err) => {
                    eprintln!("reauth succeeded but .env write failed ({err})");
                }
            }
        }
        let explicit_user_id = args
            .fetch_user_id
            .clone()
            .or_else(|| env_any(&["X_FETCH_USER_ID", "XPB_X_FETCH_USER_ID"]));

        let user_id = if let Some(user_id) = explicit_user_id {
            Some(user_id)
        } else if let Some(username) = args
            .fetch_username
            .clone()
            .or_else(|| env_any(&["X_FETCH_USERNAME", "XPB_X_FETCH_USERNAME"]))
        {
            let username = normalize_x_username(&username);
            Some(resolve_fetch_user_id_with_refresh(
                &client,
                &username,
                &mut access_token,
                refresh_config,
            )
            .await?)
        } else {
            None
        };

        let endpoint = args.fetch_endpoint.clone().or_else(|| {
            user_id.map(|resolved_user| {
                format!("https://api.x.com/2/users/{resolved_user}/bookmarks")
            })
        }).ok_or_else(|| {
            anyhow::anyhow!(
                "--fetch requires --fetch-user-id/--fetch-username or X_FETCH_USER_ID/X_FETCH_USERNAME when --fetch-endpoint is not set"
            )
        })?;

        Ok(Some(XBookmarkFetcher::new(
            endpoint,
            access_token,
            args.fetch_limit.min(100),
            args.fetch_limit,
            args.fetch_pages,
            client,
        )))
    } else {
        Ok(None)
    }
}

async fn run_cycle(
    pipeline: &Arc<Pipeline>,
    fetcher: &mut Option<XBookmarkFetcher>,
    fetch_client: &Client,
    refresh_config: &mut Option<XRefreshConfig>,
    args: &CliArgs,
) -> Result<Vec<PipelineResultModel>, anyhow::Error> {
    let bookmarks = if let Some(fetcher) = fetcher {
        let suppress_retry = preflight_refresh_fetcher_token(
            fetcher,
            fetch_client,
            refresh_config,
            args.reauth,
            args,
        )
        .await?;
        if suppress_retry {
            fetcher.fetch().await.map_err(anyhow::Error::from)?
        } else {
            fetch_bookmarks_with_refresh(fetcher, fetch_client, refresh_config, args).await?
        }
    } else {
        cli::load_bookmarks(args)?
    };

    println!("loaded {} bookmarks", bookmarks.len());
    if bookmarks.is_empty() {
        println!("no bookmarks to process");
        return Ok(Vec::new());
    }

    let results = pipeline.clone().run_batch(bookmarks, args.should_save()).await;
    println!("processed {} bookmarks", results.len());
    for result in &results {
        println!(
            "{} => cached={}, has_script={}, error={}",
            result.tweet_id,
            result.cached,
            !result.pine_script.is_empty(),
            if result.error.is_empty() {
                "none"
            } else {
                &result.error
            }
        );
    }

    Ok(results)
}

fn pipeline_providers(
    shared_http: Client,
) -> anyhow::Result<(
    Arc<dyn LLMProvider>,
    Arc<dyn LLMProvider>,
    Arc<dyn LLMProvider>,
    Arc<dyn LLMProvider>,
)> {
    Ok((
        Arc::new(CerebrasProvider::new(
            require_env("CEREBRAS_API_KEY", &[])?,
            shared_http.clone(),
        )),
        Arc::new(XaiProvider::new(
            require_env("XAI_API_KEY", &[])?,
            shared_http.clone(),
        )),
        Arc::new(ClaudeProvider::new(
            require_env("ANTHROPIC_API_KEY", &[])?,
            shared_http.clone(),
        )),
        Arc::new(OpenAIProvider::new(
            require_env("OPENAI_API_KEY", &[])?,
            shared_http,
        )),
    ))
}

fn has_processing_inputs(args: &CliArgs) -> bool {
    args.fetch || args.file.is_some() || !args.text.is_empty()
}

async fn handle_oauth_commands(
    args: &CliArgs,
    http: &Client,
) -> anyhow::Result<bool> {
    if args.auth_url {
        let (client_id, _client_secret, redirect_uri) = load_oauth_client(args.auth_redirect_uri.as_deref())?;
        let scope = oauth_scope();
        let (url, state) = build_oauth_authorization_url(&client_id, &redirect_uri, &scope);
        write_oauth_state(&state)?;
        emit_auth_flow_instructions(&client_id, &redirect_uri, &scope, &state.code_verifier, &url);
        return Ok(true);
    }

    if let Some(code) = args.auth_code.as_deref() {
        let (client_id, client_secret, redirect_uri) = load_oauth_client(args.auth_redirect_uri.as_deref())?;
        let verifier = if let Some(verifier) = args.auth_code_verifier.as_deref() {
            verifier.to_string()
        } else {
            let state = load_oauth_state()?;
            state.code_verifier
        };

        let (access_token, refresh_token) = exchange_authorization_code(
            http,
            code,
            &verifier,
            &redirect_uri,
            &client_id,
            client_secret,
        )
        .await?;
        let wrote = persist_refreshed_access_token(
            &access_token,
            refresh_token.as_deref(),
        )?;
        env::set_var("X_BEARER_TOKEN", &access_token);
        env::set_var("X_ACCESS_TOKEN", &access_token);
        env::set_var("X_USER_ACCESS_TOKEN", &access_token);
        println!("OAuth exchange succeeded.");
        if let Some(refresh_token) = refresh_token.as_deref() {
            println!("Refresh token received and stored.");
            env::set_var("X_REFRESH_TOKEN", refresh_token);
            env::set_var("XPB_X_REFRESH_TOKEN", refresh_token);
        }
        if wrote {
            println!("Updated .env with fresh X credentials.");
        } else {
            println!("Refreshed tokens available for this process; .env not updated.");
        }
        clear_oauth_state();
        if has_processing_inputs(args) {
            return Ok(false);
        }
        return Ok(true);
    }

    Ok(false)
}

#[tokio::main]
async fn main() -> Result<()> {
    if dotenvy::from_filename(".env").is_err() {
        let _ = dotenvy::dotenv();
    }

    let args = CliArgs::parse();
    let mut cfg = AppConfig::from_env();

    if let Some(output_dir) = args.output_dir.as_ref() {
        cfg.output_dir = output_dir.clone();
    }
    if let Some(cache_path) = args.cache_path.as_ref() {
        cfg.cache_path = cache_path.clone();
    }
    if let Some(workers) = args.workers {
        cfg.max_workers = workers.max(1);
    }

    let cache: Option<BookmarkCache> = if args.no_cache {
        None
    } else {
        Some(BookmarkCache::new(&cfg.cache_path).with_context(|| format!("opening cache {}", cfg.cache_path))?)
    };

    let shared_http = Client::builder()
        .timeout(Duration::from_secs(cfg.api_timeout.round() as u64))
        .build()?;

    if handle_oauth_commands(&args, &shared_http).await? && !has_processing_inputs(&args) {
        return Ok(());
    }

    let mut refresh_config = load_refresh_config();
    if args.clear_cache {
        if let Some(cache) = &cache {
            let removed = cache.clear().await?;
            println!("cache cleared: {removed}");
        } else {
            println!("cache disabled");
        }
        return Ok(());
    }

    ensure_cli_authentication(&args, &shared_http, &mut refresh_config).await?;

    if args.cache_stats {
        if let Some(cache) = &cache {
            let stats = cache.stats().await?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
        } else {
            println!("cache disabled");
        }
        return Ok(());
    }

    let providers = pipeline_providers(shared_http.clone())?;
    let mut fetcher = build_fetcher(&args, &cfg, &mut refresh_config).await?;
    let notifier = build_notifier();
    if notifier.is_some() {
        println!("notifications enabled");
    } else {
        println!("notifications disabled (set SMTP_HOST/SMTP_USER/SMTP_PASS/SMTP_FROM/SMTP_TO)");
    }

    let hook: OnMetaSaved = Arc::new(|meta_path: &str| {
        println!("meta saved: {meta_path}");
        Ok(())
    });

    let pipeline = Arc::new(
        Pipeline::new(
            Arc::clone(&providers.0),
            Arc::clone(&providers.1),
            Arc::clone(&providers.2),
            Arc::clone(&providers.3),
            cache,
            notifier.clone(),
            &cfg,
        )
        .with_cache(!args.no_cache)
        .with_vision(!args.no_vision)
        .with_verbose(args.verbose)
        .with_on_meta_saved(hook),
    );

    let daemon_mode = args.daemon || env_flag(&["XPB_DAEMON", "DAEMON_MODE"]);
    let daemon_interval = if args.daemon {
        args.daemon_interval
    } else {
        env_u64(
            &["DAEMON_INTERVAL_SECONDS", "XPB_DAEMON_INTERVAL_SECONDS"],
            args.daemon_interval,
        )
    };
    let max_cycles = if let Some(max_cycles) = args.max_cycles {
        Some(max_cycles)
    } else {
        let configured = env_usize(&["DAEMON_MAX_CYCLES", "XPB_DAEMON_MAX_CYCLES"], usize::MAX);
        if configured == usize::MAX {
            None
        } else {
            Some(configured)
        }
    };

    if !daemon_mode {
        let _ = run_cycle(&pipeline, &mut fetcher, &shared_http, &mut refresh_config, &args).await?;
        return Ok(());
    }

    let mut cycle = 0usize;
    let mut fail_streak = 0u32;
    loop {
        cycle += 1;
        match run_cycle(
            &pipeline,
            &mut fetcher,
            &shared_http,
            &mut refresh_config,
            &args,
        )
        .await
        {
            Ok(results) => {
                fail_streak = 0;
                if let Some(notifier) = &notifier {
                    let total = results.len();
                    let completed = results.iter().filter(|result| result.error.is_empty()).count();
                    let cached = results.iter().filter(|result| result.cached).count();
                    let failed = results.iter().filter(|result| !result.error.is_empty()).count();
                    if total > 0 {
                        let _ = notifier
                            .send_cycle_summary(total, completed, cached, failed)
                            .await;
                    }
                }
            }
            Err(err) => {
                fail_streak += 1;
                eprintln!("cycle {cycle} failed: {err}");
                if fail_streak >= 10 {
                    if let Some(notifier) = &notifier {
                        let _ = notifier
                            .send_text(
                                format!("X Bookmarks daemon cycle failed (cycle {cycle})"),
                                format!(
                                    "Daemon cycle {cycle} failed after {fail_streak} consecutive failures: {err}"
                                ),
                            )
                            .await;
                    }
                }
            }
        }

        if let Some(limit) = max_cycles {
            if cycle >= limit {
                break;
            }
        }

        tokio::time::sleep(Duration::from_secs(daemon_interval.max(1))).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = env::var(key).ok();
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
            EnvVarGuard { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.clone() {
                Some(previous) => env::set_var(self.key, previous),
                None => env::remove_var(self.key),
            }
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap()
    }

    #[test]
    fn require_env_prefers_primary_key_before_aliases() {
        let _lock = lock_env();
        let _primary = EnvVarGuard::set("XPB_ENV_TEST_PRIMARY", Some("primary-key"));
        let _alias = EnvVarGuard::set("XPB_ENV_TEST_ALIAS", Some("alias-key"));
        let value = require_env("XPB_ENV_TEST_PRIMARY", &["XPB_ENV_TEST_ALIAS"]).unwrap();
        assert_eq!(value, "primary-key");
    }

    #[test]
    fn require_env_falls_back_to_alias_when_primary_missing() {
        let _lock = lock_env();
        let _primary = EnvVarGuard::set("XPB_ENV_TEST_PRIMARY", None);
        let _alias = EnvVarGuard::set("XPB_ENV_TEST_ALIAS", Some("alias-key"));
        let value = require_env("XPB_ENV_TEST_PRIMARY", &["XPB_ENV_TEST_ALIAS"]).unwrap();
        assert_eq!(value, "alias-key");
    }

    #[test]
    fn require_env_errors_when_required_keys_missing() {
        let _lock = lock_env();
        let _primary = EnvVarGuard::set("XPB_ENV_TEST_PRIMARY", None);
        let _alias = EnvVarGuard::set("XPB_ENV_TEST_ALIAS", None);
        let err = require_env("XPB_ENV_TEST_PRIMARY", &["XPB_ENV_TEST_ALIAS"]).unwrap_err();
        assert!(err.to_string().contains("missing required env var"));
    }

    #[test]
    fn env_any_prefers_first_non_empty() {
        let _lock = lock_env();
        let _first = EnvVarGuard::set("XPB_ENV_TEST_ANY_FIRST", Some(""));
        let _second = EnvVarGuard::set("XPB_ENV_TEST_ANY_SECOND", Some("winner"));
        let value = env_any(&["XPB_ENV_TEST_ANY_FIRST", "XPB_ENV_TEST_ANY_SECOND"]).unwrap();
        assert_eq!(value, "winner");
    }
}
