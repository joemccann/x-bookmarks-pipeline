use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub cerebras_model: String,
    pub xai_model: String,
    pub anthropic_model: String,
    pub openai_model: String,
    pub api_timeout: f64,
    pub vision_timeout: f64,
    pub fetch_timeout: f64,
    pub max_workers: usize,
    pub output_dir: String,
    pub cache_path: String,
    pub x_api_cache_path: String,
    pub default_ticker: String,
    pub default_timeframe: String,
    // X API cost optimization settings
    pub daemon_fetch_limit: usize,
    pub daemon_fetch_pages: usize,
    pub daemon_interval_seconds: u64,
    pub token_validation_cache_seconds: u64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            cerebras_model: env_or("CEREBRAS_MODEL", "qwen-3-235b-a22b-instruct-2507"),
            xai_model: env_or("XAI_MODEL", "grok-4-0709"),
            anthropic_model: env_or("ANTHROPIC_MODEL", "claude-opus-4-6"),
            openai_model: env_or("OPENAI_MODEL", "gpt-5.4"),
            api_timeout: env_or_float("API_TIMEOUT", 120.0),
            vision_timeout: env_or_float("VISION_TIMEOUT", 60.0),
            fetch_timeout: env_or_float("FETCH_TIMEOUT", 30.0),
            max_workers: env_or_usize("MAX_WORKERS", 5),
            output_dir: env_or("OUTPUT_DIR", "output"),
            cache_path: env_or("CACHE_PATH", "cache/bookmarks.db"),
            x_api_cache_path: env_or("X_API_CACHE_PATH", "cache/x_api.db"),
            default_ticker: env_or("DEFAULT_TICKER", "BTCUSDT"),
            default_timeframe: env_or("DEFAULT_TIMEFRAME", "D"),
            // Cost-optimized defaults for daemon mode:
            // - Fetch only 25 bookmarks per cycle (vs 100 before)
            // - Only 1 page per cycle (vs 5 before)
            // - 15 minute intervals (vs 5 minutes before)
            // - Cache token validation for 5 minutes
            daemon_fetch_limit: env_or_usize("DAEMON_FETCH_LIMIT", 25),
            daemon_fetch_pages: env_or_usize("DAEMON_FETCH_PAGES", 1),
            daemon_interval_seconds: env_or_u64("DAEMON_INTERVAL_SECONDS", 900), // 15 minutes
            token_validation_cache_seconds: env_or_u64("TOKEN_VALIDATION_CACHE_SECONDS", 300), // 5 minutes
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn env_or_usize(key: &str, default: usize) -> usize {
    env::var(key).ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(default)
}

fn env_or_u64(key: &str, default: u64) -> u64 {
    env::var(key).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(default)
}

fn env_or_float(key: &str, default: f64) -> f64 {
    env::var(key).ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(default)
}
