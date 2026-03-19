use crate::cost::CostTracker;
use crate::error::{PipelineError, PipelineResult};
use crate::models::PipelineResult as PipelineRunResult;
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::Serialize;
use std::collections::HashMap;

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

#[derive(Debug, Clone, PartialEq)]
struct RenderedCycleSummaryEmail {
    subject: String,
    html: String,
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
        cost_tracker: Option<&CostTracker>,
    ) -> PipelineResult<()> {
        let cost_totals = summarize_costs(cost_tracker);
        let Some(email) = build_cycle_summary_email(results, &cost_totals) else {
            return Ok(());
        };

        self.send_html(email.subject, email.html).await
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

        let mut builder = Message::builder().from(from).to(to).subject(subject);

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

fn build_cycle_summary_email(
    results: &[PipelineRunResult],
    cost_totals: &HashMap<String, f64>,
) -> Option<RenderedCycleSummaryEmail> {
    let new: Vec<_> = results.iter().filter(|r| !r.cached).collect();
    let errors: Vec<_> = new.iter().filter(|r| !r.error.is_empty()).collect();
    let ok: Vec<_> = new.iter().filter(|r| r.error.is_empty()).collect();

    if new.is_empty() {
        return None;
    }

    let subject = if errors.is_empty() {
        format!("New bookmarks: {}", ok.len())
    } else {
        format!("New bookmarks: {} ({} errors)", new.len(), errors.len())
    };

    let total_cost: f64 = new
        .iter()
        .map(|r| cost_totals.get(&r.tweet_id).copied().unwrap_or_default())
        .sum();
    let intro_text = if errors.is_empty() {
        format!(
            "{} new bookmarks are ready. Total LLM cost for this cycle: {}.",
            ok.len(),
            format_usd(total_cost)
        )
    } else {
        format!(
            "{} bookmarks were processed with {} errors. Total LLM cost for this cycle: {}.",
            new.len(),
            errors.len(),
            format_usd(total_cost)
        )
    };
    let preheader = format!(
        "{} new bookmarks, {} errors, total LLM cost {}.",
        new.len(),
        errors.len(),
        format_usd(total_cost)
    );

    let mut rows = String::new();
    for r in &ok {
        let cat = r
            .classification
            .as_ref()
            .map(|c| c.category.as_str())
            .unwrap_or("-");
        let subcat = r
            .classification
            .as_ref()
            .map(|c| c.subcategory.as_str())
            .unwrap_or("");
        let summary = r
            .classification
            .as_ref()
            .map(|c| c.summary.as_str())
            .filter(|summary| !summary.trim().is_empty())
            .unwrap_or("No summary available.");
        let is_finance = r
            .classification
            .as_ref()
            .map(|c| c.is_finance)
            .unwrap_or(false);
        let (badge_bg, badge_fg) = if is_finance {
            ("#dff3e6", "#1f6d3d")
        } else {
            ("#dde9fb", "#214f9c")
        };
        let tweet_url = format!("https://x.com/i/web/status/{}", r.tweet_id);
        let category_label = if subcat.is_empty() {
            cat.to_string()
        } else {
            format!("{cat} / {subcat}")
        };
        let cost_display = format_usd(cost_totals.get(&r.tweet_id).copied().unwrap_or_default());

        rows.push_str(&format!(
            r#"<tr class="stack-row">
  <td class="stack-cell" style="padding:16px 18px;border-bottom:1px solid #e9dfd1;vertical-align:top;width:150px">
    <span class="stack-label" style="display:none;mso-hide:all;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f;padding-bottom:6px">Bookmark</span>
    <a class="link-button" href="{tweet_url}" style="display:inline-block;padding:10px 14px;border-radius:999px;background:#132033;color:#fffaf2;text-decoration:none;font-size:13px;font-weight:700;line-height:1.2">Open tweet</a>
    <div style="padding-top:8px;color:#8d816f;font-size:12px;line-height:1.5">ID {tweet_id}</div>
  </td>
  <td class="stack-cell" style="padding:16px 18px;border-bottom:1px solid #e9dfd1;vertical-align:top;width:170px">
    <span class="stack-label" style="display:none;mso-hide:all;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f;padding-bottom:6px">Category</span>
    <span style="display:inline-block;padding:6px 10px;border-radius:999px;background:{badge_bg};color:{badge_fg};font-size:12px;font-weight:700;line-height:1.4">{category_label}</span>
  </td>
  <td class="stack-cell align-right" style="padding:16px 18px;border-bottom:1px solid #e9dfd1;vertical-align:top;text-align:right;width:132px;white-space:nowrap">
    <span class="stack-label" style="display:none;mso-hide:all;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f;padding-bottom:6px">Total Cost (USD)</span>
    <span style="display:inline-block;padding:6px 10px;border-radius:999px;background:#efe7d9;color:#132033;font-size:13px;font-weight:700;line-height:1.4">{cost_display}</span>
  </td>
  <td class="stack-cell" style="padding:16px 18px;border-bottom:1px solid #e9dfd1;vertical-align:top;color:#433a32;font-size:14px;line-height:1.65">
    <span class="stack-label" style="display:none;mso-hide:all;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f;padding-bottom:6px">Summary</span>
    {summary}
  </td>
</tr>"#,
            tweet_url = html_escape(&tweet_url),
            tweet_id = html_escape(&r.tweet_id),
            category_label = html_escape(&category_label),
            summary = html_escape(&truncate_text(summary, 220)),
        ));
    }

    let bookmarks_table = if !rows.is_empty() {
        format!(
            r#"<table width="100%" cellpadding="0" cellspacing="0" style="border:1px solid #e9dfd1;border-radius:18px;background:#fffdf9">
  <tr class="table-head" style="background:#f7f1e6">
    <th style="padding:14px 18px;text-align:left;color:#8d816f;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;width:150px">Bookmark</th>
    <th style="padding:14px 18px;text-align:left;color:#8d816f;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;width:170px">Category</th>
    <th style="padding:14px 18px;text-align:right;color:#8d816f;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;width:132px">Total Cost (USD)</th>
    <th style="padding:14px 18px;text-align:left;color:#8d816f;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase">Summary</th>
  </tr>
  {rows}
</table>"#
        )
    } else {
        String::new()
    };

    let mut error_rows = String::new();
    for r in &errors {
        let tweet_url = format!("https://x.com/i/web/status/{}", r.tweet_id);
        error_rows.push_str(&format!(
            r#"<tr class="stack-row">
  <td class="stack-cell" style="padding:16px 18px;border-bottom:1px solid #f2d6d4;vertical-align:top;width:150px">
    <span class="stack-label" style="display:none;mso-hide:all;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f;padding-bottom:6px">Bookmark</span>
    <a class="link-button" href="{tweet_url}" style="display:inline-block;padding:10px 14px;border-radius:999px;background:#561b18;color:#fff7f6;text-decoration:none;font-size:13px;font-weight:700;line-height:1.2">Open tweet</a>
    <div style="padding-top:8px;color:#8d816f;font-size:12px;line-height:1.5">ID {tweet_id}</div>
  </td>
  <td class="stack-cell" style="padding:16px 18px;border-bottom:1px solid #f2d6d4;vertical-align:top;color:#7f1d1d;font-size:14px;line-height:1.65">
    <span class="stack-label" style="display:none;mso-hide:all;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f;padding-bottom:6px">Error</span>
    {error}
  </td>
</tr>"#,
            tweet_url = html_escape(&tweet_url),
            tweet_id = html_escape(&r.tweet_id),
            error = html_escape(&truncate_text(&r.error, 280)),
        ));
    }

    let errors_table = if !error_rows.is_empty() {
        format!(
            r#"<div style="padding-top:24px">
  <div style="padding-bottom:10px;font-size:12px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#7f1d1d">Errors</div>
  <table width="100%" cellpadding="0" cellspacing="0" style="border:1px solid #f2d6d4;border-radius:18px;background:#fff7f6">
    <tr class="table-head" style="background:#fdecec">
      <th style="padding:14px 18px;text-align:left;color:#a14643;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;width:150px">Bookmark</th>
      <th style="padding:14px 18px;text-align:left;color:#a14643;font-size:11px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase">Error</th>
    </tr>
    {error_rows}
  </table>
</div>"#
        )
    } else {
        String::new()
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta name="x-apple-disable-message-reformatting">
  <title>{subject}</title>
  <style>
    body {{
      margin: 0;
      padding: 0;
      background-color: #efe7d9;
    }}
    table {{
      border-collapse: collapse;
    }}
    .preheader {{
      display: none !important;
      visibility: hidden;
      opacity: 0;
      color: transparent;
      height: 0;
      width: 0;
      overflow: hidden;
      mso-hide: all;
    }}
    @media only screen and (max-width: 640px) {{
      .email-shell {{
        width: 100% !important;
      }}
      .outer-padding {{
        padding: 18px 12px !important;
      }}
      .hero-padding {{
        padding: 24px 20px !important;
      }}
      .content-padding {{
        padding: 20px 16px !important;
      }}
      .hero-title {{
        font-size: 26px !important;
        line-height: 1.15 !important;
      }}
      .stats-stack td {{
        display: block !important;
        width: 100% !important;
        padding: 0 0 12px !important;
      }}
      .table-head {{
        display: none !important;
      }}
      .stack-row {{
        display: block !important;
      }}
      .stack-cell {{
        display: block !important;
        width: auto !important;
        padding: 12px 16px !important;
        text-align: left !important;
        border-bottom: 0 !important;
      }}
      .stack-row td:last-child {{
        border-bottom: 1px solid #e9dfd1 !important;
        padding-bottom: 18px !important;
      }}
      .stack-label {{
        display: block !important;
      }}
      .link-button {{
        display: block !important;
        width: 100% !important;
        box-sizing: border-box !important;
        text-align: center !important;
      }}
      .align-right {{
        text-align: left !important;
      }}
    }}
  </style>
</head>
<body style="margin:0;padding:0;background:#efe7d9;font-family:'Avenir Next','Segoe UI','Helvetica Neue',Helvetica,Arial,sans-serif;color:#132033">
  <div class="preheader">{preheader}</div>
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;background:#efe7d9">
    <tr>
      <td class="outer-padding" align="center" style="padding:28px 16px">
        <table role="presentation" width="100%" cellpadding="0" cellspacing="0" class="email-shell" style="width:100%;max-width:680px">
          <tr>
            <td class="hero-padding" style="padding:30px 32px;background:#132033;border-radius:28px">
              <div style="font-size:11px;line-height:1.4;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;color:#ffb68c">X Bookmarks Pipeline</div>
              <div class="hero-title" style="padding-top:14px;font-size:34px;line-height:1.05;font-weight:700;letter-spacing:-0.03em;color:#fffaf2">Latest daemon cycle</div>
              <div style="padding-top:14px;font-size:15px;line-height:1.7;color:#d9e1eb">{intro}</div>
            </td>
          </tr>
          <tr>
            <td style="padding-top:18px">
              <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;background:#fffaf2;border:1px solid #e9dfd1;border-radius:28px;box-shadow:0 18px 48px rgba(19,32,51,0.08)">
                <tr>
                  <td class="content-padding" style="padding:28px 28px 30px">
                    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" class="stats-stack" style="width:100%;padding-bottom:22px">
                      <tr>
                        <td style="width:25%;padding:0 10px 0 0">
                          <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;background:#fffdf9;border:1px solid #e9dfd1;border-radius:18px">
                            <tr><td style="padding:14px 16px">
                              <div style="font-size:11px;line-height:1.4;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f">New</div>
                              <div style="padding-top:6px;font-size:28px;line-height:1;font-weight:700;color:#132033">{new_count}</div>
                            </td></tr>
                          </table>
                        </td>
                        <td style="width:25%;padding:0 10px 0 0">
                          <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;background:#fffdf9;border:1px solid #e9dfd1;border-radius:18px">
                            <tr><td style="padding:14px 16px">
                              <div style="font-size:11px;line-height:1.4;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f">Successful</div>
                              <div style="padding-top:6px;font-size:28px;line-height:1;font-weight:700;color:#132033">{ok_count}</div>
                            </td></tr>
                          </table>
                        </td>
                        <td style="width:25%;padding:0 10px 0 0">
                          <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;background:#fff7f6;border:1px solid #f2d6d4;border-radius:18px">
                            <tr><td style="padding:14px 16px">
                              <div style="font-size:11px;line-height:1.4;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#a14643">Errors</div>
                              <div style="padding-top:6px;font-size:28px;line-height:1;font-weight:700;color:#7f1d1d">{error_count}</div>
                            </td></tr>
                          </table>
                        </td>
                        <td style="width:25%;padding:0">
                          <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;background:#f7f1e6;border:1px solid #e9dfd1;border-radius:18px">
                            <tr><td style="padding:14px 16px">
                              <div style="font-size:11px;line-height:1.4;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f">LLM Cost</div>
                              <div style="padding-top:8px;font-size:18px;line-height:1.2;font-weight:700;color:#132033">{total_cost}</div>
                            </td></tr>
                          </table>
                        </td>
                      </tr>
                    </table>
                    <div style="padding-bottom:10px;font-size:12px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#8d816f">Bookmarks</div>
                    {bookmarks_table}
                    {errors_table}
                  </td>
                </tr>
              </table>
            </td>
          </tr>
          <tr>
            <td style="padding:16px 4px 0;text-align:center;color:#7f7568;font-size:11px;line-height:1.6">
              X Bookmarks Pipeline
            </td>
          </tr>
        </table>
      </td>
    </tr>
  </table>
</body>
</html>"#,
        subject = html_escape(&subject),
        preheader = html_escape(&preheader),
        intro = html_escape(&intro_text),
        new_count = new.len(),
        ok_count = ok.len(),
        error_count = errors.len(),
        total_cost = html_escape(&format_usd(total_cost)),
        bookmarks_table = bookmarks_table,
        errors_table = errors_table,
    );

    Some(RenderedCycleSummaryEmail { subject, html })
}

fn summarize_costs(cost_tracker: Option<&CostTracker>) -> HashMap<String, f64> {
    let mut totals = HashMap::new();
    if let Some(cost_tracker) = cost_tracker {
        for entry in cost_tracker.entries() {
            *totals.entry(entry.bookmark_id).or_insert(0.0) += entry.cost_usd;
        }
    }
    totals
}

fn format_usd(value: f64) -> String {
    if value >= 1.0 {
        format!("${value:.2}")
    } else if value >= 0.01 {
        format!("${value:.4}")
    } else {
        format!("${value:.6}")
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return value.chars().take(max_chars).collect();
    }

    let mut truncated: String = value.chars().take(max_chars - 3).collect();
    truncated.push_str("...");
    truncated
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClassificationResult;

    fn sample_result(
        tweet_id: &str,
        cached: bool,
        error: &str,
        category: &str,
        subcategory: &str,
        summary: &str,
        is_finance: bool,
    ) -> PipelineRunResult {
        let mut result = PipelineRunResult::new(tweet_id);
        result.cached = cached;
        result.error = error.to_string();
        result.classification = Some(ClassificationResult {
            tweet_id: tweet_id.to_string(),
            is_finance,
            confidence: 0.97,
            classification_source: "test".to_string(),
            has_trading_pattern: is_finance,
            has_visual_data: true,
            category: category.to_string(),
            subcategory: subcategory.to_string(),
            detected_topic: "BTC".to_string(),
            summary: summary.to_string(),
            raw_text: "raw".to_string(),
            image_urls: Vec::new(),
        });
        result
    }

    #[test]
    fn build_cycle_summary_email_includes_cost_column_and_mobile_styles() {
        let ok = sample_result(
            "111",
            false,
            "",
            "finance",
            "crypto",
            "Breakout setup with clear resistance flip and strong relative volume.",
            true,
        );
        let err = sample_result(
            "222",
            false,
            "planner failed to produce a valid strategy",
            "other",
            "general",
            "Ignored because this row is rendered in the errors section.",
            false,
        );
        let mut cost_totals = HashMap::new();
        cost_totals.insert("111".to_string(), 0.000312);
        cost_totals.insert("222".to_string(), 0.0142);

        let email = build_cycle_summary_email(&[ok, err], &cost_totals).expect("email to render");

        assert_eq!(email.subject, "New bookmarks: 2 (1 errors)");
        assert!(email.html.contains("Total Cost (USD)"));
        assert!(email.html.contains("$0.000312"));
        assert!(email.html.contains("@media only screen and (max-width: 640px)"));
        assert!(email.html.contains("Latest daemon cycle"));
        assert!(email.html.contains("planner failed to produce a valid strategy"));
    }

    #[test]
    fn build_cycle_summary_email_skips_fully_cached_cycles() {
        let cached = sample_result(
            "333",
            true,
            "",
            "finance",
            "macro",
            "This bookmark should be skipped because it is cached.",
            true,
        );

        assert!(build_cycle_summary_email(&[cached], &HashMap::new()).is_none());
    }
}
