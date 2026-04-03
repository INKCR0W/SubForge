use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use app_storage::{Database, SettingsRepository};
use time::{Date, Duration as TimeDuration, Month, OffsetDateTime};
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::LoadedHeadlessConfig;
use crate::security::ensure_data_dir;

const LOG_FILE_PREFIX: &str = "subforge-core.log";
const DEFAULT_LOG_DIR_NAME: &str = "logs";
const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_LOG_RETENTION_DAYS: u16 = 7;

pub(crate) struct LoggingRuntime {
    _guard: Option<WorkerGuard>,
    pub(crate) initialized: bool,
    pub(crate) log_dir: PathBuf,
    pub(crate) level: String,
    pub(crate) retention_days: u16,
    pub(crate) cleaned_files: usize,
}

pub(crate) fn initialize_logging(
    data_dir: &Path,
    loaded_config: Option<&LoadedHeadlessConfig>,
    database: &Database,
) -> Result<LoggingRuntime> {
    let log_dir = resolve_log_dir(data_dir, loaded_config, database)?;
    let level = resolve_log_level(loaded_config, database)?;
    let retention_days = resolve_log_retention_days(loaded_config, database)?;
    ensure_data_dir(&log_dir)?;
    let cleaned_files = cleanup_expired_logs(&log_dir, retention_days, OffsetDateTime::now_utc())?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let level_filter = parse_tracing_level(&level);
    let init_result = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_max_level(level_filter)
        .try_init();

    let (guard, initialized) = match init_result {
        Ok(()) => (Some(guard), true),
        Err(error) => {
            eprintln!("WARNING: tracing 日志初始化失败，将继续运行（{error}）");
            (None, false)
        }
    };

    Ok(LoggingRuntime {
        _guard: guard,
        initialized,
        log_dir,
        level,
        retention_days,
        cleaned_files,
    })
}

fn resolve_log_dir(
    data_dir: &Path,
    loaded_config: Option<&LoadedHeadlessConfig>,
    database: &Database,
) -> Result<PathBuf> {
    if let Some(config) = loaded_config
        && let Some(path) = config.resolved_log_dir()?
    {
        return Ok(path);
    }
    if let Some(path) = read_setting(database, "log_dir")? {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let raw = PathBuf::from(trimmed);
            return Ok(if raw.is_absolute() {
                raw
            } else {
                data_dir.join(raw)
            });
        }
    }
    Ok(data_dir.join(DEFAULT_LOG_DIR_NAME))
}

fn resolve_log_level(
    loaded_config: Option<&LoadedHeadlessConfig>,
    database: &Database,
) -> Result<String> {
    if let Some(config) = loaded_config {
        return Ok(config.config.log.level.trim().to_ascii_lowercase());
    }
    if let Some(value) = read_setting(database, "log_level")? {
        let trimmed = value.trim().to_ascii_lowercase();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    Ok(DEFAULT_LOG_LEVEL.to_string())
}

fn resolve_log_retention_days(
    loaded_config: Option<&LoadedHeadlessConfig>,
    database: &Database,
) -> Result<u16> {
    if let Some(config) = loaded_config {
        return Ok(config.config.log.retention_days.max(1));
    }
    if let Some(value) = read_setting(database, "log_retention_days")? {
        let parsed = value
            .trim()
            .parse::<u16>()
            .unwrap_or(DEFAULT_LOG_RETENTION_DAYS);
        return Ok(parsed.max(1));
    }
    Ok(DEFAULT_LOG_RETENTION_DAYS)
}

fn read_setting(database: &Database, key: &str) -> Result<Option<String>> {
    let repository = SettingsRepository::new(database);
    let setting = repository
        .get(key)
        .with_context(|| format!("读取系统设置失败: {key}"))?;
    Ok(setting.map(|item| item.value))
}

fn parse_tracing_level(level: &str) -> Level {
    match level {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    }
}

fn cleanup_expired_logs(log_dir: &Path, retention_days: u16, now: OffsetDateTime) -> Result<usize> {
    let keep_days = retention_days.max(1);
    let oldest_kept_date = now.date() - TimeDuration::days(i64::from(keep_days - 1));
    let mut removed = 0usize;

    for entry in
        fs::read_dir(log_dir).with_context(|| format!("读取日志目录失败: {}", log_dir.display()))?
    {
        let entry = entry.with_context(|| format!("读取日志目录项失败: {}", log_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("读取日志文件类型失败: {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(LOG_FILE_PREFIX) {
            continue;
        }

        let Some(log_date) = extract_iso_date(&file_name) else {
            continue;
        };
        if log_date < oldest_kept_date {
            fs::remove_file(entry.path())
                .with_context(|| format!("删除过期日志失败: {}", entry.path().display()))?;
            removed += 1;
        }
    }

    Ok(removed)
}

fn extract_iso_date(file_name: &str) -> Option<Date> {
    let bytes = file_name.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    for index in 0..=bytes.len() - 10 {
        let slice = &bytes[index..index + 10];
        if let Some(date) = parse_iso_date_slice(slice) {
            return Some(date);
        }
    }
    None
}

fn parse_iso_date_slice(slice: &[u8]) -> Option<Date> {
    if slice.len() != 10 || slice[4] != b'-' || slice[7] != b'-' {
        return None;
    }
    let year = parse_u16(&slice[0..4])? as i32;
    let month = parse_u8(&slice[5..7])?;
    let day = parse_u8(&slice[8..10])?;
    let month = Month::try_from(month).ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

fn parse_u16(raw: &[u8]) -> Option<u16> {
    if raw.is_empty() || raw.iter().any(|value| !value.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(raw).ok()?.parse::<u16>().ok()
}

fn parse_u8(raw: &[u8]) -> Option<u8> {
    if raw.is_empty() || raw.iter().any(|value| !value.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(raw).ok()?.parse::<u8>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_iso_date_from_log_filename() {
        let parsed = extract_iso_date("subforge-core.log.2026-04-03");
        assert_eq!(
            parsed,
            Some(Date::from_calendar_date(2026, Month::April, 3).expect("构造日期失败"))
        );
    }

    #[test]
    fn cleanup_expired_logs_removes_only_outdated_managed_files() {
        let temp_dir = std::env::temp_dir().join(format!(
            "subforge-logging-cleanup-{}",
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("创建临时日志目录失败");

        let old_managed = temp_dir.join("subforge-core.log.2026-03-20");
        let retained_managed = temp_dir.join("subforge-core.log.2026-04-01");
        let unmanaged = temp_dir.join("app.log.2026-03-01");
        let current = temp_dir.join("subforge-core.log");
        fs::write(&old_managed, "old").expect("写入旧日志失败");
        fs::write(&retained_managed, "new").expect("写入保留日志失败");
        fs::write(&unmanaged, "ignore").expect("写入无关日志失败");
        fs::write(&current, "active").expect("写入当前日志失败");

        let now = Date::from_calendar_date(2026, Month::April, 3)
            .expect("构造日期失败")
            .with_hms(12, 0, 0)
            .expect("构造时间失败")
            .assume_utc();
        let removed = cleanup_expired_logs(&temp_dir, 7, now).expect("清理日志失败");

        assert_eq!(removed, 1);
        assert!(!old_managed.exists(), "过期日志应被删除");
        assert!(retained_managed.exists(), "保留期内日志不应删除");
        assert!(unmanaged.exists(), "非托管日志不应删除");
        assert!(current.exists(), "当前日志文件不应删除");

        fs::remove_dir_all(&temp_dir).expect("删除临时目录失败");
    }
}
