use crate::error::{PipelineError, PipelineResult};
use crate::models::PipelineResult as PipelineRunResult;
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_user: String,
    pub smtp_password: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct SmtpNotifier {
    config: EmailConfig,
}

impl SmtpNotifier {
    pub fn new(config: EmailConfig) -> Self {
        Self { config }
    }

    pub async fn send_meta_saved(&self, meta_path: &str) -> PipelineResult<()> {
        let subject = format!("Bookmark meta saved: {meta_path}");
        let body = format!("Meta file written: {meta_path}");
        self.send_text(subject, body).await
    }

    /// Send a rich HTML notification for a newly processed bookmark.
    pub async fn send_bookmark_processed(
        &self,
        result: &PipelineRunResult,
    ) -> PipelineResult<()> {
        let classification = result.classification.as_ref();
        let plan = result.plan.as_ref();

        let category = classification
            .map(|c| c.category.as_str())
            .unwrap_or("unknown");
        let subcategory = classification
            .map(|c| c.subcategory.as_str())
            .unwrap_or("unknown");
        let is_finance = classification.map(|c| c.is_finance).unwrap_or(false);
        let summary = classification
            .map(|c| c.summary.as_str())
            .unwrap_or("");
        let topic = classification
            .map(|c| c.detected_topic.as_str())
            .unwrap_or("");
        let confidence = classification.map(|c| c.confidence).unwrap_or(0.0);

        let ticker = plan.map(|p| p.ticker.as_str()).unwrap_or("—");
        let direction = plan.map(|p| p.direction.as_str()).unwrap_or("—");
        let timeframe = plan.map(|p| p.timeframe.as_str()).unwrap_or("—");
        let script_type = plan.map(|p| p.script_type.as_str()).unwrap_or("—");
        let rationale = plan.map(|p| p.rationale.as_str()).unwrap_or("");

        let has_script = !result.pine_script.is_empty();
        let validation_passed = result
            .validation
            .as_ref()
            .map(|v| v.valid)
            .unwrap_or(false);

        let emoji_category = if is_finance { "📈" } else { "📝" };
        let emoji_validation = if has_script {
            if validation_passed { "✅" } else { "⚠️" }
        } else {
            "—"
        };

        let badge_color = if is_finance { "#3fb950" } else { "#58a6ff" };

        let subject = if is_finance {
            format!("📈 {ticker} — {category}/{subcategory} bookmark processed")
        } else {
            format!("📝 {category}/{subcategory} bookmark processed")
        };

        let strategy_section = if is_finance {
            format!(
                r#"
    <tr>
      <td colspan="2" style="padding:12px 0 6px;font-size:13px;font-weight:600;color:#c9d1d9;border-bottom:1px solid #30363d">Strategy</td>
    </tr>
    <tr>
      <td style="padding:6px 0;color:#8b949e;font-size:13px;width:120px">Ticker</td>
      <td style="padding:6px 0;color:#fff;font-size:13px;font-weight:600">{ticker}</td>
    </tr>
    <tr>
      <td style="padding:6px 0;color:#8b949e;font-size:13px">Direction</td>
      <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{direction}</td>
    </tr>
    <tr>
      <td style="padding:6px 0;color:#8b949e;font-size:13px">Timeframe</td>
      <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{timeframe}</td>
    </tr>
    <tr>
      <td style="padding:6px 0;color:#8b949e;font-size:13px">Script Type</td>
      <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{script_type}</td>
    </tr>
    <tr>
      <td style="padding:6px 0;color:#8b949e;font-size:13px">Pine Script</td>
      <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{emoji_validation} {valid_label}</td>
    </tr>"#,
                valid_label = if has_script {
                    if validation_passed { "Generated & validated" } else { "Generated (warnings)" }
                } else {
                    "Not generated"
                },
            )
        } else {
            String::new()
        };

        let rationale_section = if !rationale.is_empty() {
            format!(
                r#"
    <tr>
      <td colspan="2" style="padding:12px 0 6px;font-size:13px;font-weight:600;color:#c9d1d9;border-bottom:1px solid #30363d">Rationale</td>
    </tr>
    <tr>
      <td colspan="2" style="padding:6px 0;color:#8b949e;font-size:13px;line-height:1.5">{rationale}</td>
    </tr>"#,
            )
        } else {
            String::new()
        };

        let tweet_text = classification
            .map(|c| {
                let t = &c.raw_text;
                if t.len() > 280 { format!("{}...", &t[..277]) } else { t.clone() }
            })
            .unwrap_or_default();

        let tweet_section = if !tweet_text.is_empty() {
            format!(
                r#"
    <tr>
      <td colspan="2" style="padding:12px 0 6px;font-size:13px;font-weight:600;color:#c9d1d9;border-bottom:1px solid #30363d">Tweet</td>
    </tr>
    <tr>
      <td colspan="2" style="padding:8px 12px;color:#8b949e;font-size:13px;line-height:1.5;background:#161b22;border-radius:6px;font-style:italic">{}</td>
    </tr>"#,
                html_escape(&tweet_text),
            )
        } else {
            String::new()
        };

        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="margin:0;padding:0;background:#0d1117;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif">
<table width="100%" cellpadding="0" cellspacing="0" style="background:#0d1117;padding:20px">
<tr><td align="center">
<table width="560" cellpadding="0" cellspacing="0" style="background:#0d1117">
  <!-- Header -->
  <tr>
    <td style="padding:0 0 16px">
      <span style="font-size:18px;font-weight:600;color:#fff">{emoji_category} X Bookmarks Pipeline</span>
    </td>
  </tr>
  <!-- Card -->
  <tr>
    <td style="background:#161b22;border:1px solid #30363d;border-radius:8px;padding:20px">
      <table width="100%" cellpadding="0" cellspacing="0">
        <!-- Classification -->
        <tr>
          <td style="padding:0 0 4px;color:#8b949e;font-size:13px;width:120px">Category</td>
          <td style="padding:0 0 4px">
            <span style="display:inline-block;padding:2px 10px;border-radius:12px;font-size:12px;font-weight:500;background:{badge_color}22;color:{badge_color}">{category}/{subcategory}</span>
          </td>
        </tr>
        <tr>
          <td style="padding:6px 0;color:#8b949e;font-size:13px">Topic</td>
          <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{topic_display}</td>
        </tr>
        <tr>
          <td style="padding:6px 0;color:#8b949e;font-size:13px">Confidence</td>
          <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{confidence:.0}%</td>
        </tr>
        <tr>
          <td style="padding:6px 0;color:#8b949e;font-size:13px">Summary</td>
          <td style="padding:6px 0;color:#c9d1d9;font-size:13px">{summary}</td>
        </tr>
        <tr>
          <td style="padding:6px 0;color:#8b949e;font-size:13px">Bookmark</td>
          <td style="padding:6px 0;font-size:13px"><span style="font-family:monospace;color:#58a6ff">{tweet_id}</span></td>
        </tr>
        {strategy_section}
        {rationale_section}
        {tweet_section}
      </table>
    </td>
  </tr>
  <!-- Footer -->
  <tr>
    <td style="padding:16px 0 0;color:#484f58;font-size:11px;text-align:center">
      X Bookmarks Pipeline &middot; Rust
    </td>
  </tr>
</table>
</td></tr>
</table>
</body>
</html>"#,
            tweet_id = result.tweet_id,
            topic_display = if topic.is_empty() { "—" } else { topic },
            confidence = confidence * 100.0,
        );

        self.send_html(subject, html).await
    }

    /// Send a rich HTML cycle summary for daemon mode.
    pub async fn send_cycle_summary(
        &self,
        total: usize,
        completed: usize,
        cached: usize,
        failed: usize,
        results: &[PipelineRunResult],
    ) -> PipelineResult<()> {
        let new_count = completed.saturating_sub(cached);

        let subject = if new_count > 0 {
            format!("📊 Pipeline cycle: {new_count} new, {cached} cached, {failed} failed ({total} total)")
        } else {
            format!("📊 Pipeline cycle: {total} bookmarks ({cached} cached, {failed} failed)")
        };

        // Build per-bookmark rows for new (non-cached) results
        let mut new_rows = String::new();
        for r in results {
            if r.cached || !r.error.is_empty() {
                continue;
            }
            let cat = r.classification.as_ref().map(|c| c.category.as_str()).unwrap_or("—");
            let subcat = r.classification.as_ref().map(|c| c.subcategory.as_str()).unwrap_or("—");
            let is_finance = r.classification.as_ref().map(|c| c.is_finance).unwrap_or(false);
            let ticker = r.plan.as_ref().map(|p| p.ticker.as_str()).unwrap_or("—");
            let has_script = if !r.pine_script.is_empty() { "✅" } else { "—" };
            let badge_color = if is_finance { "#3fb950" } else { "#58a6ff" };

            new_rows.push_str(&format!(
                r#"<tr>
  <td style="padding:6px 8px;border-bottom:1px solid #21262d;color:#c9d1d9;font-size:12px;font-family:monospace">{id}</td>
  <td style="padding:6px 8px;border-bottom:1px solid #21262d"><span style="padding:2px 8px;border-radius:10px;font-size:11px;background:{badge_color}22;color:{badge_color}">{cat}/{subcat}</span></td>
  <td style="padding:6px 8px;border-bottom:1px solid #21262d;color:#c9d1d9;font-size:12px">{ticker}</td>
  <td style="padding:6px 8px;border-bottom:1px solid #21262d;color:#c9d1d9;font-size:12px;text-align:center">{has_script}</td>
</tr>"#,
                id = &r.tweet_id[..r.tweet_id.len().min(12)],
            ));
        }

        // Error rows
        let mut error_rows = String::new();
        for r in results {
            if r.error.is_empty() {
                continue;
            }
            error_rows.push_str(&format!(
                r#"<tr>
  <td style="padding:6px 8px;border-bottom:1px solid #21262d;color:#c9d1d9;font-size:12px;font-family:monospace">{id}</td>
  <td style="padding:6px 8px;border-bottom:1px solid #21262d;color:#f85149;font-size:12px">{error}</td>
</tr>"#,
                id = &r.tweet_id[..r.tweet_id.len().min(12)],
                error = html_escape(&r.error),
            ));
        }

        let new_table = if !new_rows.is_empty() {
            format!(
                r#"
      <tr><td colspan="2" style="padding:16px 0 6px;font-size:14px;font-weight:600;color:#c9d1d9">New Bookmarks</td></tr>
      <tr><td colspan="2">
        <table width="100%" cellpadding="0" cellspacing="0" style="border:1px solid #30363d;border-radius:6px;overflow:hidden">
          <tr style="background:#1c2128">
            <th style="padding:6px 8px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">ID</th>
            <th style="padding:6px 8px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">Category</th>
            <th style="padding:6px 8px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">Ticker</th>
            <th style="padding:6px 8px;text-align:center;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">Script</th>
          </tr>
          {new_rows}
        </table>
      </td></tr>"#,
            )
        } else {
            r#"<tr><td colspan="2" style="padding:16px 0 6px;color:#8b949e;font-size:13px">No new bookmarks in this cycle.</td></tr>"#.to_string()
        };

        let error_table = if !error_rows.is_empty() {
            format!(
                r#"
      <tr><td colspan="2" style="padding:16px 0 6px;font-size:14px;font-weight:600;color:#f85149">Errors</td></tr>
      <tr><td colspan="2">
        <table width="100%" cellpadding="0" cellspacing="0" style="border:1px solid #30363d;border-radius:6px;overflow:hidden">
          <tr style="background:#1c2128">
            <th style="padding:6px 8px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">ID</th>
            <th style="padding:6px 8px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">Error</th>
          </tr>
          {error_rows}
        </table>
      </td></tr>"#,
            )
        } else {
            String::new()
        };

        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="margin:0;padding:0;background:#0d1117;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif">
<table width="100%" cellpadding="0" cellspacing="0" style="background:#0d1117;padding:20px">
<tr><td align="center">
<table width="600" cellpadding="0" cellspacing="0" style="background:#0d1117">
  <tr>
    <td style="padding:0 0 16px">
      <span style="font-size:18px;font-weight:600;color:#fff">📊 X Bookmarks Pipeline — Cycle Summary</span>
    </td>
  </tr>
  <tr>
    <td style="background:#161b22;border:1px solid #30363d;border-radius:8px;padding:20px">
      <table width="100%" cellpadding="0" cellspacing="0">
        <!-- Stats cards -->
        <tr>
          <td style="padding:0 0 16px" colspan="2">
            <table width="100%" cellpadding="0" cellspacing="0">
              <tr>
                <td width="25%" style="text-align:center;padding:12px;background:#0d1117;border-radius:6px">
                  <div style="font-size:24px;font-weight:600;color:#fff">{total}</div>
                  <div style="font-size:11px;color:#8b949e;text-transform:uppercase;letter-spacing:0.05em">Total</div>
                </td>
                <td width="4"></td>
                <td width="25%" style="text-align:center;padding:12px;background:#0d1117;border-radius:6px">
                  <div style="font-size:24px;font-weight:600;color:#3fb950">{new_count}</div>
                  <div style="font-size:11px;color:#8b949e;text-transform:uppercase;letter-spacing:0.05em">New</div>
                </td>
                <td width="4"></td>
                <td width="25%" style="text-align:center;padding:12px;background:#0d1117;border-radius:6px">
                  <div style="font-size:24px;font-weight:600;color:#8b949e">{cached}</div>
                  <div style="font-size:11px;color:#8b949e;text-transform:uppercase;letter-spacing:0.05em">Cached</div>
                </td>
                <td width="4"></td>
                <td width="25%" style="text-align:center;padding:12px;background:#0d1117;border-radius:6px">
                  <div style="font-size:24px;font-weight:600;color:{failed_color}">{failed}</div>
                  <div style="font-size:11px;color:#8b949e;text-transform:uppercase;letter-spacing:0.05em">Failed</div>
                </td>
              </tr>
            </table>
          </td>
        </tr>
        {new_table}
        {error_table}
      </table>
    </td>
  </tr>
  <tr>
    <td style="padding:16px 0 0;color:#484f58;font-size:11px;text-align:center">
      X Bookmarks Pipeline &middot; Rust &middot; Daemon Mode
    </td>
  </tr>
</table>
</td></tr>
</table>
</body>
</html>"#,
            failed_color = if failed > 0 { "#f85149" } else { "#8b949e" },
        );

        self.send_html(subject, html).await
    }

    pub async fn send_text(&self, subject: String, body: String) -> PipelineResult<()> {
        let config = self.config.clone();
        tokio::task::spawn_blocking(move || Self::send_sync(config, &subject, &body, None))
            .await
            .map_err(|err| PipelineError::TaskJoin {
                details: err.to_string(),
            })?
    }

    pub async fn send_html(&self, subject: String, html: String) -> PipelineResult<()> {
        let config = self.config.clone();
        tokio::task::spawn_blocking(move || {
            Self::send_sync(config, &subject, &html, Some(ContentType::TEXT_HTML))
        })
        .await
        .map_err(|err| PipelineError::TaskJoin {
            details: err.to_string(),
        })?
    }

    fn send_sync(
        config: EmailConfig,
        subject: &str,
        body: &str,
        content_type: Option<ContentType>,
    ) -> PipelineResult<()> {
        let from: Mailbox = config
            .from
            .parse::<Mailbox>()
            .map_err(|err| PipelineError::Email {
                details: err.to_string(),
            })?;
        let to: Mailbox = config
            .to
            .parse::<Mailbox>()
            .map_err(|err| PipelineError::Email {
                details: err.to_string(),
            })?;

        let mut builder = Message::builder()
            .from(from)
            .to(to)
            .subject(subject);

        if let Some(ct) = content_type {
            builder = builder.header(ct);
        }

        let message = builder
            .body(body.to_string())
            .map_err(|err| PipelineError::Email {
                details: err.to_string(),
            })?;

        let creds = Credentials::new(config.smtp_user, config.smtp_password);
        let mailer = SmtpTransport::relay(&config.smtp_host)
            .map_err(|err| PipelineError::Email {
                details: err.to_string(),
            })?
            .credentials(creds)
            .build();
        mailer.send(&message).map_err(|err| PipelineError::Email {
            details: err.to_string(),
        })?;
        Ok(())
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
