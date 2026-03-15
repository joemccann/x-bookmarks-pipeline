use std::{env, sync::Arc, time::Duration};

use anyhow::Context;
use x_bookmarks_pipeline_rust::{
    cache::BookmarkCache,
    config::AppConfig,
    llm::{CerebrasProvider, ClaudeProvider, OpenAIProvider, XaiProvider},
    models::Bookmark,
    notify::{EmailConfig, SmtpNotifier},
    orchestrator::{OnMetaSaved, Pipeline},
};
use reqwest::Client;

fn env_any(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

fn require_env(name: &str, aliases: &[&str]) -> anyhow::Result<String> {
    env_any(&std::iter::once(name).chain(aliases.iter().copied()).collect::<Vec<_>>())
        .with_context(|| format!("missing required env var {name}"))
}

fn parse_bookmarks() -> Vec<Bookmark> {
    vec![
        Bookmark {
            id: "tweet-1001".to_string(),
            text: "Bitcoin shows potential breakout near 68k; RSI overbought with resistance".to_string(),
            author: "alpha_trader".to_string(),
            date: "2026-03-14".to_string(),
            image_urls: vec!["https://example.com/sample-chart.png".to_string()],
            tweet_url: "https://x.com/example/status/1001".to_string(),
            chart_description: String::new(),
        },
        Bookmark {
            id: "tweet-1002".to_string(),
            text: "Nice article about Rust async ergonomics".to_string(),
            author: "builder".to_string(),
            date: "2026-03-12".to_string(),
            image_urls: vec![],
            tweet_url: "https://x.com/example/status/1002".to_string(),
            chart_description: String::new(),
        },
    ]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if dotenvy::from_filename(".env").is_err() {
        let _ = dotenvy::dotenv();
    }

    let cfg = AppConfig::from_env();
    let shared_http = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let cerebras = Arc::new(CerebrasProvider::new(
        require_env("CEREBRAS_API_KEY", &[])?,
        shared_http.clone(),
    ));
    let xai = Arc::new(XaiProvider::new(
        require_env("XAI_API_KEY", &[])?,
        shared_http.clone(),
    ));
    let claude = Arc::new(ClaudeProvider::new(
        require_env("ANTHROPIC_API_KEY", &[])?,
        shared_http.clone(),
    ));
    let openai = Arc::new(OpenAIProvider::new(
        require_env("OPENAI_API_KEY", &[])?,
        shared_http.clone(),
    ));

    let cache = BookmarkCache::new(&cfg.cache_path).ok();

    let notifier = match (
        env_any(&["SMTP_HOST", "XPB_SMTP_HOST"]),
        env_any(&["SMTP_USER", "XPB_SMTP_USER"]),
        env_any(&["SMTP_PASSWORD", "SMTP_PASS", "XPB_SMTP_PASSWORD"]),
        env_any(&["SMTP_FROM", "EMAIL_FROM", "XPB_SMTP_FROM"]),
        env_any(&["SMTP_TO", "EMAIL_TO", "XPB_SMTP_TO"]),
    ) {
        (Some(host), Some(user), Some(password), Some(from), Some(to)) => {
            Some(Arc::new(SmtpNotifier::new(EmailConfig {
                smtp_host: host,
                smtp_user: user,
                smtp_password: password,
                from,
                to,
            })))
        }
        _ => None,
    };

    let hook: OnMetaSaved = Arc::new(|meta_path: &str| {
        println!("meta saved: {meta_path}");
        Ok(())
    });

    let pipeline = Arc::new(
        Pipeline::new(
            cerebras,
            xai,
            claude,
            openai,
            cache,
            notifier,
            &cfg,
        )
        .with_on_meta_saved(hook),
    );

    let bookmarks = parse_bookmarks();
    let results = pipeline.run_batch(bookmarks, true).await;
    println!("processed {} bookmarks", results.len());
    for result in results {
        println!(
            "{} => cached={}, has_script={}, error={}",
            result.tweet_id,
            result.cached,
            !result.pine_script.is_empty(),
            if result.error.is_empty() { "none" } else { &result.error }
        );
    }

    Ok(())
}
