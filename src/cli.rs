use crate::models::{Bookmark, XBookmark};
use anyhow::{Context, Result};
use clap::Parser;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Parser)]
#[command(
    name = "x-bookmarks-pipeline-rust",
    version,
    about = "Run the Rust X bookmarks pipeline"
)]
pub struct CliArgs {
    #[arg(long, help = "Read bookmark text entries directly")]
    pub text: Vec<String>,

    #[arg(short, long, value_name = "FILE", help = "Load bookmarks from file")]
    pub file: Option<PathBuf>,

    #[arg(long = "fetch", help = "Fetch bookmarks from X API")]
    pub fetch: bool,

    #[arg(long = "fetch-user-id", value_name = "USER_ID", help = "X user id for bookmark fetch endpoint")]
    pub fetch_user_id: Option<String>,

    #[arg(long = "fetch-endpoint", value_name = "URL", help = "Override X bookmarks endpoint URL")]
    pub fetch_endpoint: Option<String>,

    #[arg(long = "fetch-limit", default_value_t = 100, help = "Maximum bookmarks to fetch")]
    pub fetch_limit: usize,

    #[arg(long = "fetch-pages", default_value_t = 5, help = "Maximum bookmark pages to request")]
    pub fetch_pages: usize,

    #[arg(long = "no-cache", help = "Disable cache reads/writes")]
    pub no_cache: bool,

    #[arg(long = "no-vision", help = "Disable vision analysis stage")]
    pub no_vision: bool,

    #[arg(short = 'w', long = "workers", help = "Override worker count from config")]
    pub workers: Option<usize>,

    #[arg(long = "save", default_value_t = true, help = "Persist outputs and meta files")]
    pub save: bool,

    #[arg(long = "no-save", help = "Disable persisting outputs and metadata")]
    pub no_save: bool,

    #[arg(long = "clear-cache", help = "Clear cache and exit")]
    pub clear_cache: bool,

    #[arg(long = "cache-stats", help = "Print cache statistics and exit")]
    pub cache_stats: bool,

    #[arg(long = "daemon", help = "Run continuously in polling mode")]
    pub daemon: bool,

    #[arg(long = "daemon-interval", default_value_t = 300, help = "Polling interval in seconds when --daemon is enabled")]
    pub daemon_interval: u64,

    #[arg(long = "max-cycles", help = "Maximum daemon cycles before exit")]
    pub max_cycles: Option<usize>,

    #[arg(long = "output-dir", help = "Override output directory")]
    pub output_dir: Option<String>,

    #[arg(long = "cache-path", help = "Override cache DB path")]
    pub cache_path: Option<String>,
}

impl CliArgs {
    pub fn should_save(&self) -> bool {
        self.save && !self.no_save
    }
}

pub fn load_bookmarks(args: &CliArgs) -> Result<Vec<Bookmark>> {
    let mut bookmarks = Vec::new();
    let mut index = 0usize;

    if let Some(path) = &args.file {
        for mut bookmark in parse_bookmarks_file(path)? {
            bookmark = normalize_bookmark(bookmark, "file", index);
            index += 1;
            bookmarks.push(bookmark);
        }
    }

    for text in &args.text {
        bookmarks.push(bookmark_from_text(text, index));
        index += 1;
    }

    if bookmarks.is_empty() {
        Err(anyhow::anyhow!(
            "No bookmark sources provided. Use --text, --file, or --fetch."
        ))
    } else {
        Ok(bookmarks)
    }
}

fn parse_bookmarks_file(path: &Path) -> Result<Vec<Bookmark>> {
    let raw = fs::read_to_string(path).with_context(|| format!("failed to read {path:?}"))?;

    if let Ok(values) = serde_json::from_str::<Vec<Bookmark>>(&raw) {
        return Ok(values);
    }

    if let Ok(values) = serde_json::from_str::<Vec<XBookmark>>(&raw) {
        return Ok(values.into_iter().map(|x| x.to_bookmark()).collect());
    }

    if let Ok(values) = serde_json::from_str::<Value>(&raw) {
        if let Some(array) = values.as_array() {
            let mut parsed = Vec::with_capacity(array.len());
            for (idx, value) in array.iter().enumerate() {
                parsed.push(parse_bookmark_value(value).with_context(|| {
                    format!("invalid bookmark entry at index {idx} in {path:?}")
                })?);
            }
            return Ok(parsed);
        }
        if let Some(listed) = values.get("bookmarks").and_then(Value::as_array) {
            let mut parsed = Vec::with_capacity(listed.len());
            for (idx, value) in listed.iter().enumerate() {
                parsed.push(parse_bookmark_value(value).with_context(|| {
                    format!("invalid bookmark entry at index {idx} in {path:?}")
                })?);
            }
            return Ok(parsed);
        }
    }

    Ok(parse_bookmarks_plain_lines(&raw))
}

fn parse_bookmark_value(value: &Value) -> Result<Bookmark> {
    if let Ok(bookmark) = serde_json::from_value::<Bookmark>(value.clone()) {
        return Ok(bookmark);
    }
    if let Ok(bookmark) = serde_json::from_value::<XBookmark>(value.clone()) {
        return Ok(bookmark.to_bookmark());
    }
    let line = value.to_string();
    Ok(bookmark_from_text(&line, 0))
}

fn parse_bookmarks_plain_lines(raw: &str) -> Vec<Bookmark> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| bookmark_from_text(line, index))
        .collect()
}

fn bookmark_from_text(text: &str, index: usize) -> Bookmark {
    Bookmark {
        id: format!("text-{}", deterministic_id(&format!("line-{index}:{text}"))),
        text: text.to_string(),
        author: "cli".to_string(),
        date: "undated".to_string(),
        image_urls: Vec::new(),
        tweet_url: format!("https://x.com/i/bookmarks/{index}"),
        chart_description: String::new(),
    }
}

fn normalize_bookmark(mut bookmark: Bookmark, source: &str, index: usize) -> Bookmark {
    if bookmark.id.trim().is_empty() {
        bookmark.id = format!("{source}-{}", deterministic_id(&format!("{source}:{index}:{}", bookmark.text)));
    }
    if bookmark.author.trim().is_empty() {
        bookmark.author = "cli".to_string();
    }
    if bookmark.date.trim().is_empty() {
        bookmark.date = "undated".to_string();
    }
    if bookmark.tweet_url.trim().is_empty() {
        bookmark.tweet_url = format!("https://x.com/i/bookmarks/{}", bookmark.id);
    }
    bookmark
}

fn deterministic_id(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut hex = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        path.push(format!("x-bp-{name}-{}-{}.json", process::id(), nanos));
        path
    }

    #[test]
    fn cli_text_inputs_parse_to_bookmarks() {
        let args = CliArgs::parse_from(["x-bookmarks", "--text", "chart breakout", "--text", "manual note"]);
        let bookmarks = load_bookmarks(&args).unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert!(bookmarks[0].id.starts_with("text-"));
        assert_eq!(bookmarks[0].author, "cli");
    }

    #[test]
    fn cli_file_json_array_parses_x_bookmarks() {
        let path = temp_path("json_array");
        fs::write(
            &path,
            r#"[{"tweet_id":"tweet-1","text":"BTC chart","author":"alpha","date":"2026-03-14","image_urls":[],"is_article":false,"tweet_url":"https://x.com/a/status/1"}]"#,
        )
        .unwrap();
        let args = CliArgs::parse_from(["x-bookmarks", "--file", path.to_str().unwrap()]);
        let bookmarks = load_bookmarks(&args).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, "tweet-1");
        assert_eq!(bookmarks[0].author, "alpha");
    }

    #[test]
    fn cli_plain_text_file_parses_each_line() {
        let path = temp_path("plain");
        fs::write(&path, "first line\nsecond line\n").unwrap();
        let args = CliArgs::parse_from(["x-bookmarks", "--file", path.to_str().unwrap()]);
        let bookmarks = load_bookmarks(&args).unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].author, "cli");
    }
}
