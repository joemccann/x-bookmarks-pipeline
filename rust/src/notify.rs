use crate::error::{PipelineError, PipelineResult};
use lettre::message::Mailbox;
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
        let config = self.config.clone();
        let subject = format!("Bookmark meta saved: {meta_path}");
        let body = format!("Meta file written: {meta_path}");

        tokio::task::spawn_blocking(move || Self::send_sync(config, &subject, &body))
            .await
            .map_err(|err| PipelineError::TaskJoin {
                details: err.to_string(),
            })?
    }

    fn send_sync(config: EmailConfig, subject: &str, body: &str) -> PipelineResult<()> {
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

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
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
