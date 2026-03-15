use std::{env, sync::Arc, time::Duration};

use anyhow::Context;
use x_bookmarks_pipeline_rust::{
    cache::BookmarkCache,
    llm::{CerebrasProvider, ClaudeProvider, OpenAIProvider, XaiProvider},
    models::{Bookmark, FinalScript},
    notify::{EmailConfig, SmtpNotifier},
    orchestrator::{OnMetaSaved, Pipeline},
};
use reqwest::Client;

fn env_any(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = env::var(name) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn parse_required_env(name: &str, aliases: &[&str]) -> anyhow::Result<String> {
    env_any(&std::iter::once(name)
        .chain(aliases.iter().copied())
        .collect::<Vec<_>>())
    .with_context(|| format!("Missing required env var: {name} (tried aliases: {aliases:?})"))
}

fn parse_bookmarks() -> Vec<Bookmark> {
    vec![
        Bookmark {
            id: "tweet-1001".to_string(),
            url: "https://x.com/example/status/1001".to_string(),
            title: "Example chart link".to_string(),
            note: Some("seed input for migration dry-run".to_string()),
            image_url: Some("https://example.com/sample-chart.png".to_string()),
        },
        Bookmark {
            id: "tweet-1002".to_string(),
            url: "https://x.com/example/status/1002".to_string(),
            title: "No image bookmark example".to_string(),
            note: None,
            image_url: None,
        },
    ]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut env_loaded = false;
    for candidate in [".env", "../.env"] {
        if dotenvy::from_path(candidate).is_ok() {
            env_loaded = true;
            break;
        }
    }
    if !env_loaded {
        dotenvy::dotenv().ok();
    }

    let shared_http = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // Instantiate provider clients once and pass shared handles into the orchestrator.
    let openai = Arc::new(OpenAIProvider::new(
        parse_required_env("OPENAI_API_KEY", &[])?,
        shared_http.clone(),
    ));
    let _xai = Arc::new(XaiProvider::new(
        parse_required_env("XAI_API_KEY", &[])?,
        shared_http.clone(),
    ));
    let claude = Arc::new(ClaudeProvider::new(
        parse_required_env("ANTHROPIC_API_KEY", &[])?,
        shared_http.clone(),
    ));
    let cerebras = Arc::new(CerebrasProvider::new(
        parse_required_env("CEREBRAS_API_KEY", &[])?,
        shared_http.clone(),
    ));

    let cache = BookmarkCache::new(
        env_any(&["XPB_CACHE_PATH", "CACHE_PATH"]).unwrap_or_else(|| {
            "cache/bookmark_cache.sqlite".to_string()
        }),
    )?;

    let notifier = match (
        env_any(&["XPB_SMTP_HOST", "SMTP_HOST"]),
        env_any(&["XPB_SMTP_USER", "SMTP_USER"]),
        env_any(&["XPB_SMTP_PASSWORD", "SMTP_PASS"]),
        env_any(&["XPB_SMTP_FROM", "EMAIL_FROM"]),
        env_any(&["XPB_SMTP_TO", "EMAIL_TO"]),
    ) {
        (Some(smtp_host), Some(smtp_user), Some(smtp_password), Some(from), Some(to)) => {
            Some(Arc::new(
            SmtpNotifier::new(EmailConfig {
                smtp_host,
                smtp_user,
                smtp_password,
                from,
                to,
            }),
        ))
        }
        _ => None,
    };

    let pipeline = Arc::new(Pipeline::new(
        cerebras, // classify
        // Use xAI/Claude providers as needed in your deployment; this example uses
        // the Claude-style interface for image analysis and code generation.
        claude,   // analyze image
        openai,   // generate code
        cache,
        notifier,
        4,
    ));

    let on_meta_saved: OnMetaSaved = Arc::new(|result: &FinalScript| {
        println!(
            "meta saved: id={} category={}",
            result.bookmark_id, result.meta.classification.category
        );
        Ok(())
    });

    let bookmarks = parse_bookmarks();
    let outputs = pipeline
        .run(bookmarks, Some(on_meta_saved))
        .await
        .unwrap_or_else(|err| {
            println!("pipeline failed: {err:?}");
            Vec::new()
        });

    println!("produced {} scripts", outputs.len());
    for output in outputs {
        println!("--- {} ---\n{}\n", output.bookmark_id, output.pine_script);
    }

    Ok(())
}
