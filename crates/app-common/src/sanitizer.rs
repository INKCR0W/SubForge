use std::sync::OnceLock;

use regex::Regex;

const REDACTED: &str = "***";
const KEY_VALUE_KEYS_PATTERN: &str = r"access_token|accessToken|admin_token|adminToken|api_key|apiKey|apikey|secret_key|secretKey|set-cookie|x-api-key|x-auth-token|x-access-token|password|passwd|cookie|secret|token|auth";
const QUERY_KEYS_PATTERN: &str = r"access_token|accessToken|admin_token|adminToken|api_key|apiKey|apikey|secret_key|secretKey|password|passwd|authorization|cookie|secret|token|auth";

static AUTHORIZATION_RE: OnceLock<Regex> = OnceLock::new();
static BEARER_RE: OnceLock<Regex> = OnceLock::new();
static COOKIE_HEADER_RE: OnceLock<Regex> = OnceLock::new();
static KEY_VALUE_RE: OnceLock<Regex> = OnceLock::new();
static QUERY_RE: OnceLock<Regex> = OnceLock::new();
static URL_USERINFO_RE: OnceLock<Regex> = OnceLock::new();

/// 对可能暴露到日志、API 或事件流的文本做统一敏感信息脱敏。
///
/// 该函数只处理文本出口的通用模式，不替代结构化权限校验或 secret 存储隔离。
pub fn redact_sensitive_text(message: &str) -> String {
    let url_userinfo_re = URL_USERINFO_RE.get_or_init(|| {
        Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)([^/?#@\s]+)@")
            .expect("URL userinfo 脱敏正则必须合法")
    });
    let mut sanitized = url_userinfo_re
        .replace_all(message, format!("$1{REDACTED}@"))
        .to_string();

    let authorization_re = AUTHORIZATION_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(authorization)\b(\s*[:=]\s*)([A-Za-z]+\s+)?([A-Za-z0-9\-._~+/=]+)")
            .expect("Authorization 脱敏正则必须合法")
    });
    sanitized = authorization_re
        .replace_all(&sanitized, format!("$1$2${{3}}{REDACTED}"))
        .to_string();

    let bearer_re = BEARER_RE.get_or_init(|| {
        Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9\-._~+/=]+").expect("Bearer token 脱敏正则必须合法")
    });
    sanitized = bearer_re
        .replace_all(&sanitized, format!("Bearer {REDACTED}"))
        .to_string();

    let cookie_header_re = COOKIE_HEADER_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(cookie|set-cookie)\b(\s*:\s*)[^\r\n]+")
            .expect("Cookie header 脱敏正则必须合法")
    });
    sanitized = cookie_header_re
        .replace_all(&sanitized, format!("$1$2{REDACTED}"))
        .to_string();

    let query_re = QUERY_RE.get_or_init(|| {
        Regex::new(&format!(r"(?i)([?&](?:{QUERY_KEYS_PATTERN})=)[^&\s]+"))
            .expect("敏感 query 脱敏正则必须合法")
    });
    sanitized = query_re
        .replace_all(&sanitized, format!("$1{REDACTED}"))
        .to_string();

    let key_value_re = KEY_VALUE_RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i)\b({KEY_VALUE_KEYS_PATTERN})\b(\s*[:=]\s*)(bearer\s+)?([^&?\s,;]+)"
        ))
        .expect("敏感 key-value 脱敏正则必须合法")
    });
    key_value_re
        .replace_all(&sanitized, format!("$1$2${{3}}{REDACTED}"))
        .to_string()
}
