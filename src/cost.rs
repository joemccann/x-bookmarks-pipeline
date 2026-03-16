use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Token usage from a single LLM API call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl UsageInfo {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Per-model pricing in USD per 1M tokens: (input, output).
/// Updated 2026-03. Conservative estimates; actual pricing may vary.
fn price_per_million(provider: &str, model: &str) -> (f64, f64) {
    match provider {
        "cerebras" => {
            if model.contains("qwen-3-235b") {
                (0.20, 0.20) // Cerebras inference pricing
            } else {
                (0.10, 0.10)
            }
        }
        "xai" => {
            if model.contains("grok-4") {
                (3.00, 15.00) // xAI Grok-4 pricing
            } else if model.contains("grok-3") {
                (3.00, 15.00)
            } else {
                (2.00, 10.00)
            }
        }
        "claude" => {
            if model.contains("opus") {
                (15.00, 75.00) // Claude Opus
            } else if model.contains("sonnet") {
                (3.00, 15.00) // Claude Sonnet
            } else if model.contains("haiku") {
                (0.25, 1.25) // Claude Haiku
            } else {
                (15.00, 75.00) // default to Opus pricing
            }
        }
        "openai" => {
            if model.contains("gpt-5") {
                (2.00, 8.00) // GPT-5 estimated
            } else if model.contains("gpt-4") || model.contains("o3") || model.contains("o4") {
                (2.50, 10.00) // GPT-4o/o3
            } else {
                (2.00, 8.00)
            }
        }
        _ => (1.00, 4.00), // fallback
    }
}

/// Compute the USD cost for a single API call.
pub fn compute_cost(usage: &UsageInfo) -> f64 {
    let (input_rate, output_rate) = price_per_million(&usage.provider, &usage.model);
    let input_cost = usage.input_tokens as f64 * input_rate / 1_000_000.0;
    let output_cost = usage.output_tokens as f64 * output_rate / 1_000_000.0;
    input_cost + output_cost
}

/// A single cost entry for one LLM call within a bookmark's pipeline run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub bookmark_id: String,
    pub stage: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// Accumulates cost entries across pipeline stages for all bookmarks.
#[derive(Debug, Clone)]
pub struct CostTracker {
    entries: Arc<Mutex<Vec<CostEntry>>>,
    active_bookmark: Arc<Mutex<String>>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            active_bookmark: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn record(&self, bookmark_id: &str, stage: &str, usage: &UsageInfo) {
        let cost = compute_cost(usage);
        let entry = CostEntry {
            bookmark_id: bookmark_id.to_string(),
            stage: stage.to_string(),
            provider: usage.provider.clone(),
            model: usage.model.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_usd: cost,
        };
        self.entries.lock().unwrap().push(entry);
    }

    /// Set the active bookmark ID context for recording.
    /// This is used by providers that don't know which bookmark they're processing.
    pub fn set_active_bookmark(&self, bookmark_id: &str) {
        *self.active_bookmark.lock().unwrap() = bookmark_id.to_string();
    }

    /// Record usage with the currently active bookmark context.
    pub fn record_active(&self, stage: &str, usage: &UsageInfo) {
        let bookmark_id = self.active_bookmark.lock().unwrap().clone();
        self.record(&bookmark_id, stage, usage);
    }

    pub fn entries(&self) -> Vec<CostEntry> {
        self.entries.lock().unwrap().clone()
    }

    pub fn total_cost(&self) -> f64 {
        self.entries.lock().unwrap().iter().map(|e| e.cost_usd).sum()
    }

    pub fn to_json(&self) -> serde_json::Value {
        let entries = self.entries();
        let total: f64 = entries.iter().map(|e| e.cost_usd).sum();
        let total_input: u64 = entries.iter().map(|e| e.input_tokens).sum();
        let total_output: u64 = entries.iter().map(|e| e.output_tokens).sum();

        serde_json::json!({
            "total_cost_usd": format!("{:.6}", total),
            "total_input_tokens": total_input,
            "total_output_tokens": total_output,
            "calls": entries,
        })
    }
}

/// Summary of costs across all bookmarks in a pipeline run, for reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCostSummary {
    pub bookmark_id: String,
    pub category: String,
    pub is_finance: bool,
    pub total_cost_usd: f64,
    pub entries: Vec<CostEntry>,
}

/// Generate a markdown cost report from per-bookmark summaries.
pub fn generate_cost_report(summaries: &[RunCostSummary]) -> String {
    let mut md = String::new();
    md.push_str("# LLM Cost Report\n\n");
    md.push_str(&format!(
        "Generated: {}\n\n",
        chrono_now()
    ));

    // YTD summary by provider
    let mut by_provider: HashMap<String, (u64, u64, f64)> = HashMap::new();
    let mut grand_total = 0.0f64;

    for summary in summaries {
        grand_total += summary.total_cost_usd;
        for entry in &summary.entries {
            let e = by_provider.entry(entry.provider.clone()).or_default();
            e.0 += entry.input_tokens;
            e.1 += entry.output_tokens;
            e.2 += entry.cost_usd;
        }
    }

    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "- **Total bookmarks processed**: {}\n",
        summaries.len()
    ));
    md.push_str(&format!(
        "- **Total LLM cost**: ${:.4}\n\n",
        grand_total
    ));

    // Provider breakdown
    md.push_str("## Cost by Provider\n\n");
    md.push_str("| Provider | Input Tokens | Output Tokens | Cost (USD) | % of Total |\n");
    md.push_str("|----------|-------------|---------------|-----------|------------|\n");

    let mut providers: Vec<_> = by_provider.iter().collect();
    providers.sort_by(|a, b| b.1 .2.partial_cmp(&a.1 .2).unwrap());

    for (provider, (input, output, cost)) in &providers {
        let pct = if grand_total > 0.0 {
            cost / grand_total * 100.0
        } else {
            0.0
        };
        md.push_str(&format!(
            "| {} | {:>11} | {:>13} | ${:>8.4} | {:>9.1}% |\n",
            provider, input, output, cost, pct
        ));
    }
    md.push('\n');

    // Cost by stage
    let mut by_stage: HashMap<String, f64> = HashMap::new();
    for summary in summaries {
        for entry in &summary.entries {
            *by_stage.entry(entry.stage.clone()).or_default() += entry.cost_usd;
        }
    }

    md.push_str("## Cost by Pipeline Stage\n\n");
    md.push_str("| Stage | Cost (USD) | % of Total |\n");
    md.push_str("|-------|-----------|------------|\n");

    let mut stages: Vec<_> = by_stage.iter().collect();
    stages.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

    for (stage, cost) in &stages {
        let pct = if grand_total > 0.0 {
            *cost / grand_total * 100.0
        } else {
            0.0
        };
        md.push_str(&format!(
            "| {} | ${:>8.4} | {:>9.1}% |\n",
            stage, cost, pct
        ));
    }
    md.push('\n');

    // Per-bookmark table
    md.push_str("## Per-Bookmark Costs\n\n");
    md.push_str("| Bookmark ID | Category | Finance | Cost (USD) | Stages |\n");
    md.push_str("|-------------|----------|---------|-----------|--------|\n");

    let mut sorted_summaries: Vec<_> = summaries.iter().collect();
    sorted_summaries.sort_by(|a, b| b.total_cost_usd.partial_cmp(&a.total_cost_usd).unwrap());

    for s in &sorted_summaries {
        let stages: Vec<_> = s.entries.iter().map(|e| e.stage.as_str()).collect();
        let stages_str = stages.join(", ");
        md.push_str(&format!(
            "| {} | {} | {} | ${:.6} | {} |\n",
            s.bookmark_id,
            s.category,
            if s.is_finance { "yes" } else { "no" },
            s.total_cost_usd,
            stages_str,
        ));
    }

    md
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple UTC timestamp without chrono crate
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Approximate date from days since epoch (good enough for reports)
    let (year, month, day) = days_to_date(days);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02} UTC")
}

fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's civil_from_days
    let z = days_since_epoch as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_cost_cerebras_qwen() {
        let usage = UsageInfo {
            provider: "cerebras".to_string(),
            model: "qwen-3-235b-a22b-instruct-2507".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
        };
        let cost = compute_cost(&usage);
        // (1000 * 0.20 + 500 * 0.20) / 1_000_000 = 0.0003
        assert!((cost - 0.0003).abs() < 1e-10);
    }

    #[test]
    fn compute_cost_claude_opus() {
        let usage = UsageInfo {
            provider: "claude".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 2000,
            output_tokens: 1000,
        };
        let cost = compute_cost(&usage);
        // (2000 * 15.0 + 1000 * 75.0) / 1_000_000 = 0.105
        assert!((cost - 0.105).abs() < 1e-10);
    }

    #[test]
    fn compute_cost_openai_gpt5() {
        let usage = UsageInfo {
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            input_tokens: 3000,
            output_tokens: 1500,
        };
        let cost = compute_cost(&usage);
        // (3000 * 2.0 + 1500 * 8.0) / 1_000_000 = 0.018
        assert!((cost - 0.018).abs() < 1e-10);
    }

    #[test]
    fn compute_cost_xai_grok4() {
        let usage = UsageInfo {
            provider: "xai".to_string(),
            model: "grok-4-0709".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
        };
        let cost = compute_cost(&usage);
        // (1000 * 3.0 + 500 * 15.0) / 1_000_000 = 0.0105
        assert!((cost - 0.0105).abs() < 1e-10);
    }

    #[test]
    fn cost_tracker_accumulates() {
        let tracker = CostTracker::new();
        tracker.record("tweet-1", "classify", &UsageInfo {
            provider: "cerebras".to_string(),
            model: "qwen-3-235b".to_string(),
            input_tokens: 500,
            output_tokens: 200,
        });
        tracker.record("tweet-1", "plan", &UsageInfo {
            provider: "claude".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
        });

        let entries = tracker.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].stage, "classify");
        assert_eq!(entries[1].stage, "plan");
        assert!(tracker.total_cost() > 0.0);
    }

    #[test]
    fn cost_tracker_to_json_structure() {
        let tracker = CostTracker::new();
        tracker.record("tweet-1", "classify", &UsageInfo {
            provider: "cerebras".to_string(),
            model: "qwen-3-235b".to_string(),
            input_tokens: 100,
            output_tokens: 50,
        });
        let json = tracker.to_json();
        assert!(json["total_cost_usd"].is_string());
        assert!(json["calls"].is_array());
        assert_eq!(json["total_input_tokens"], 100);
        assert_eq!(json["total_output_tokens"], 50);
    }

    #[test]
    fn generate_cost_report_produces_markdown() {
        let summaries = vec![
            RunCostSummary {
                bookmark_id: "tweet-1".to_string(),
                category: "finance".to_string(),
                is_finance: true,
                total_cost_usd: 0.05,
                entries: vec![CostEntry {
                    bookmark_id: "tweet-1".to_string(),
                    stage: "classify".to_string(),
                    provider: "cerebras".to_string(),
                    model: "qwen-3-235b".to_string(),
                    input_tokens: 500,
                    output_tokens: 200,
                    cost_usd: 0.0001,
                }, CostEntry {
                    bookmark_id: "tweet-1".to_string(),
                    stage: "plan".to_string(),
                    provider: "claude".to_string(),
                    model: "opus".to_string(),
                    input_tokens: 2000,
                    output_tokens: 500,
                    cost_usd: 0.0499,
                }],
            },
            RunCostSummary {
                bookmark_id: "tweet-2".to_string(),
                category: "technology".to_string(),
                is_finance: false,
                total_cost_usd: 0.001,
                entries: vec![CostEntry {
                    bookmark_id: "tweet-2".to_string(),
                    stage: "classify".to_string(),
                    provider: "cerebras".to_string(),
                    model: "qwen-3-235b".to_string(),
                    input_tokens: 300,
                    output_tokens: 100,
                    cost_usd: 0.001,
                }],
            },
        ];
        let report = generate_cost_report(&summaries);
        assert!(report.contains("# LLM Cost Report"));
        assert!(report.contains("Cost by Provider"));
        assert!(report.contains("Per-Bookmark Costs"));
        assert!(report.contains("tweet-1"));
        assert!(report.contains("tweet-2"));
        assert!(report.contains("cerebras"));
        assert!(report.contains("claude"));
    }

    #[test]
    fn days_to_date_epoch() {
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_date_known_date() {
        // 2026-03-16 is day 20528 since epoch
        let (y, m, d) = days_to_date(20528);
        assert_eq!((y, m, d), (2026, 3, 16));
    }
}
