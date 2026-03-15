use std::{env, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Client;
use reqwest::StatusCode;
use serde_json::Value;
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

fn env_any(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
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

async fn fetch_bookmarks_with_refresh(
    fetcher: &XBookmarkFetcher,
    fetch_client: &Client,
    refresh_config: &mut Option<XRefreshConfig>,
) -> anyhow::Result<Vec<Bookmark>> {
    match fetcher.fetch().await {
        Ok(bookmarks) => Ok(bookmarks),
        Err(PipelineError::TokenExpired { .. }) if refresh_config.is_some() => {
            let token = match refresh_config {
                Some(cfg) => refresh_x_access_token(fetch_client, cfg).await?,
                None => return Err(PipelineError::TokenExpired {
                    details: "authentication token expired".to_string(),
                }
                .into()),
            };
            fetcher.set_access_token(token).await;
            fetcher.fetch().await.map_err(Into::into)
        }
        Err(err) => Err(err.into()),
    }
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
                let mut config = refresh_config
                    .take()
                    .with_context(|| "no refresh config available")?;
                let new_token = refresh_x_access_token(client, &mut config).await?;
                *token = new_token.clone();
                refresh_config.replace(config);
                resolve_fetch_user_id(client, &new_token, username).await
            } else {
                Err(err)
            }
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
        ])
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing required X API bearer token (set X_BEARER_TOKEN or X_ACCESS_TOKEN or X_USER_ACCESS_TOKEN)"
            )
        })?;

        let client = Client::builder()
            .timeout(Duration::from_secs(cfg.fetch_timeout.round() as u64))
            .build()?;

        let mut access_token = token;
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
        fetch_bookmarks_with_refresh(fetcher, fetch_client, refresh_config).await?
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

    if args.clear_cache {
        if let Some(cache) = &cache {
            let removed = cache.clear().await?;
            println!("cache cleared: {removed}");
        } else {
            println!("cache disabled");
        }
        return Ok(());
    }

    if args.cache_stats {
        if let Some(cache) = &cache {
            let stats = cache.stats().await?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
        } else {
            println!("cache disabled");
        }
        return Ok(());
    }

    let shared_http = Client::builder()
        .timeout(Duration::from_secs(cfg.api_timeout.round() as u64))
        .build()?;

    let providers = pipeline_providers(shared_http.clone())?;
    let mut refresh_config = load_refresh_config();
    let mut fetcher = build_fetcher(&args, &cfg, &mut refresh_config).await?;
    let notifier = build_notifier();

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
