use std::{env, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use x_bookmarks_pipeline_rust::{
    cache::BookmarkCache,
    cli::{self, CliArgs},
    config::AppConfig,
    fetcher::XBookmarkFetcher,
    llm::{CerebrasProvider, ClaudeProvider, LLMProvider, OpenAIProvider, XaiProvider},
    notify::{EmailConfig, SmtpNotifier},
    orchestrator::{OnMetaSaved, Pipeline},
};
use reqwest::Client;

fn env_any(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

fn require_env(name: &str, aliases: &[&str]) -> anyhow::Result<String> {
    env_any(&std::iter::once(name).chain(aliases.iter().copied()).collect::<Vec<_>>())
        .with_context(|| format!("missing required env var {name}"))
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
        cfg.max_workers = workers.max(1usize);
    }

    let shared_http = Client::builder()
        .timeout(Duration::from_secs(cfg.api_timeout.round() as u64))
        .build()?;

    let providers: (
        std::sync::Arc<dyn LLMProvider>,
        std::sync::Arc<dyn LLMProvider>,
        std::sync::Arc<dyn LLMProvider>,
        std::sync::Arc<dyn LLMProvider>,
    ) = (
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
    );

    let cache: Option<BookmarkCache> = if args.no_cache {
        None
    } else {
        Some(BookmarkCache::new(&cfg.cache_path).with_context(|| format!("opening cache {}", cfg.cache_path))?)
    };

    let fetcher = if args.fetch {
        let token = env_any(&["X_BEARER_TOKEN", "X_ACCESS_TOKEN", "XPB_X_BEARER_TOKEN"])
            .ok_or_else(|| anyhow::anyhow!("missing required X API bearer token"))?;
        let user_id = args
            .fetch_user_id
            .clone()
            .or_else(|| env_any(&["X_FETCH_USER_ID", "XPB_X_FETCH_USER_ID"]));

        let endpoint = if let Some(endpoint) = args.fetch_endpoint.clone() {
            endpoint
        } else {
            let resolved_user = user_id.clone().ok_or_else(|| {
                anyhow::anyhow!("--fetch requires --fetch-user-id or X_FETCH_USER_ID when --fetch-endpoint is not set")
            })?;
            format!("https://api.x.com/2/users/{resolved_user}/bookmarks")
        };

        Some(XBookmarkFetcher::new(
            endpoint,
            token,
            args.fetch_limit.min(100),
            args.fetch_limit,
            args.fetch_pages,
            Client::builder()
                .timeout(Duration::from_secs(cfg.fetch_timeout.round() as u64))
                .build()?,
        ))
    } else {
        None
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

    let bookmarks = if let Some(fetcher) = &fetcher {
        fetcher.fetch().await?
    } else {
        cli::load_bookmarks(&args)?
    };
    println!("loaded {} bookmarks", bookmarks.len());

    if bookmarks.is_empty() {
        println!("no bookmarks to process");
        return Ok(());
    }

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
            Arc::clone(&providers.0),
            Arc::clone(&providers.1),
            Arc::clone(&providers.2),
            Arc::clone(&providers.3),
            cache,
            notifier,
            &cfg,
        )
        .with_cache(!args.no_cache)
        .with_vision(!args.no_vision)
        .with_on_meta_saved(hook),
    );

    let results = pipeline.run_batch(bookmarks, args.should_save()).await;
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
