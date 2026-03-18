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

    /// Send a single email summarizing all new bookmarks from a daemon cycle.
    /// Only called when there are new (non-cached) bookmarks or errors.
    pub async fn send_cycle_summary(
        &self,
        results: &[PipelineRunResult],
    ) -> PipelineResult<()> {
        let new: Vec<_> = results.iter().filter(|r| !r.cached).collect();
        let errors: Vec<_> = new.iter().filter(|r| !r.error.is_empty()).collect();
        let ok: Vec<_> = new.iter().filter(|r| r.error.is_empty()).collect();

        if new.is_empty() {
            return Ok(());
        }

        let subject = if errors.is_empty() {
            format!("New bookmarks: {}", ok.len())
        } else {
            format!("New bookmarks: {} ({} errors)", new.len(), errors.len())
        };

        let mut rows = String::new();
        for r in &ok {
            let cat = r.classification.as_ref().map(|c| c.category.as_str()).unwrap_or("—");
            let subcat = r.classification.as_ref().map(|c| c.subcategory.as_str()).unwrap_or("");
            let summary = r.classification.as_ref().map(|c| c.summary.as_str()).unwrap_or("—");
            let author = r.classification.as_ref()
                .map(|c| if c.raw_text.is_empty() { "" } else { "" })
                .unwrap_or("");
            let _ = author; // author is in the bookmark, not classification — use tweet_url
            let badge_color = if r.classification.as_ref().map(|c| c.is_finance).unwrap_or(false) {
                "#3fb950"
            } else {
                "#58a6ff"
            };

            // Build tweet URL — the result may have it in meta_path or we construct from tweet_id
            let tweet_url = format!("https://x.com/i/web/status/{}", r.tweet_id);

            let summary_display = if summary.len() > 160 {
                format!("{}...", &summary[..157])
            } else {
                summary.to_string()
            };

            rows.push_str(&format!(
                r#"<tr>
  <td style="padding:10px 12px;border-bottom:1px solid #21262d;vertical-align:top">
    <a href="{tweet_url}" style="color:#58a6ff;text-decoration:none;font-size:13px">View tweet</a>
  </td>
  <td style="padding:10px 12px;border-bottom:1px solid #21262d;vertical-align:top">
    <span style="padding:2px 8px;border-radius:10px;font-size:12px;background:{badge_color}22;color:{badge_color}">{cat}/{subcat}</span>
  </td>
  <td style="padding:10px 12px;border-bottom:1px solid #21262d;vertical-align:top;color:#c9d1d9;font-size:13px;line-height:1.4">{summary_escaped}</td>
</tr>"#,
                summary_escaped = html_escape(&summary_display),
            ));
        }

        let mut error_rows = String::new();
        for r in &errors {
            let tweet_url = format!("https://x.com/i/web/status/{}", r.tweet_id);
            error_rows.push_str(&format!(
                r#"<tr>
  <td style="padding:8px 12px;border-bottom:1px solid #21262d;vertical-align:top">
    <a href="{tweet_url}" style="color:#58a6ff;text-decoration:none;font-size:13px">View tweet</a>
  </td>
  <td style="padding:8px 12px;border-bottom:1px solid #21262d;color:#f85149;font-size:13px">{error}</td>
</tr>"#,
                error = html_escape(&r.error),
            ));
        }

        let bookmarks_table = if !rows.is_empty() {
            format!(
                r#"<table width="100%" cellpadding="0" cellspacing="0" style="border:1px solid #30363d;border-radius:6px;overflow:hidden">
  <tr style="background:#1c2128">
    <th style="padding:8px 12px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase;width:90px">Link</th>
    <th style="padding:8px 12px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase;width:140px">Category</th>
    <th style="padding:8px 12px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">Summary</th>
  </tr>
  {rows}
</table>"#,
            )
        } else {
            String::new()
        };

        let errors_table = if !error_rows.is_empty() {
            format!(
                r#"<div style="margin-top:16px">
  <div style="font-size:14px;font-weight:600;color:#f85149;margin-bottom:8px">Errors</div>
  <table width="100%" cellpadding="0" cellspacing="0" style="border:1px solid #30363d;border-radius:6px;overflow:hidden">
    <tr style="background:#1c2128">
      <th style="padding:8px 12px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase;width:90px">Link</th>
      <th style="padding:8px 12px;text-align:left;color:#8b949e;font-size:11px;font-weight:500;text-transform:uppercase">Error</th>
    </tr>
    {error_rows}
  </table>
</div>"#,
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
      <span style="font-size:18px;font-weight:600;color:#fff">New Bookmarks</span>
    </td>
  </tr>
  <tr>
    <td style="background:#161b22;border:1px solid #30363d;border-radius:8px;padding:20px">
      {bookmarks_table}
      {errors_table}
    </td>
  </tr>
  <tr>
    <td style="padding:16px 0 0;color:#484f58;font-size:11px;text-align:center">
      X Bookmarks Pipeline
    </td>
  </tr>
</table>
</td></tr>
</table>
</body>
</html>"#,
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
