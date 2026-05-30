use std::sync::Mutex;

use app_common::redact_sensitive_text;
use app_plugin_runtime::{RuntimeLogLevel, RuntimeLogSink};
use app_storage::{Database, ScriptLog, ScriptLogRepository};
use time::OffsetDateTime;

use crate::utils::now_rfc3339;
use crate::{CoreError, CoreResult};

const MAX_LOG_MESSAGE_CHARS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CapturedScriptLog {
    pub(super) level: String,
    pub(super) message: String,
    pub(super) created_at: String,
}

#[derive(Debug, Default)]
pub(super) struct ScriptLogCollector {
    entries: Mutex<Vec<CapturedScriptLog>>,
}

impl ScriptLogCollector {
    pub(super) fn take(&self) -> Vec<CapturedScriptLog> {
        match self.entries.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => Vec::new(),
        }
    }
}

impl RuntimeLogSink for ScriptLogCollector {
    fn emit(&self, level: RuntimeLogLevel, message: &str) {
        let created_at = now_rfc3339().unwrap_or_else(|_| {
            OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
        });
        let captured = CapturedScriptLog {
            level: level.as_str().to_string(),
            message: sanitize_script_log_message(message),
            created_at,
        };

        if let Ok(mut guard) = self.entries.lock() {
            guard.push(captured);
        }
    }
}

pub(super) fn persist_script_logs(
    db: &Database,
    refresh_job_id: &str,
    source_instance_id: &str,
    plugin_id: &str,
    logs: Vec<CapturedScriptLog>,
) -> CoreResult<()> {
    if logs.is_empty() {
        return Ok(());
    }

    let repository = ScriptLogRepository::new(db);
    for (index, log) in logs.into_iter().enumerate() {
        let item = ScriptLog {
            id: format!(
                "script-log-{refresh_job_id}-{}-{index}",
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
            refresh_job_id: refresh_job_id.to_string(),
            source_instance_id: source_instance_id.to_string(),
            plugin_id: plugin_id.to_string(),
            level: log.level,
            message: log.message,
            created_at: log.created_at,
        };
        repository.insert(&item).map_err(CoreError::Storage)?;
    }

    Ok(())
}

fn sanitize_script_log_message(message: &str) -> String {
    redact_sensitive_text(&truncate_message(message))
}

fn truncate_message(message: &str) -> String {
    const SUFFIX: &str = "...(truncated)";
    if message.chars().count() <= MAX_LOG_MESSAGE_CHARS {
        return message.to_string();
    }

    let keep = MAX_LOG_MESSAGE_CHARS.saturating_sub(SUFFIX.chars().count());
    let truncated = message.chars().take(keep).collect::<String>();
    format!("{truncated}{SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_script_log_message_redacts_sensitive_pairs() {
        let input = "token=abc password:xyz cookie=session123 api_key=qwe";
        let output = sanitize_script_log_message(input);
        assert!(output.contains("token=***"));
        assert!(output.contains("password:***"));
        assert!(output.contains("cookie=***"));
        assert!(output.contains("api_key=***"));
        assert!(!output.contains("abc"));
        assert!(!output.contains("xyz"));
        assert!(!output.contains("session123"));
        assert!(!output.contains("qwe"));
    }

    #[test]
    fn sanitize_script_log_message_redacts_query_and_bearer() {
        let input = "url=https://example.com?a=1&token=abc123 Authorization: Bearer sensitive";
        let output = sanitize_script_log_message(input);
        assert!(output.contains("&token=***"));
        assert!(output.contains("Authorization: Bearer ***"));
        assert!(!output.contains("abc123"));
        assert!(!output.contains("sensitive"));
    }

    #[test]
    fn sanitize_script_log_message_truncates_over_limit() {
        let input = "a".repeat(MAX_LOG_MESSAGE_CHARS + 10);
        let output = sanitize_script_log_message(&input);
        assert!(output.ends_with("...(truncated)"));
        assert!(output.len() < input.len());
    }
}
