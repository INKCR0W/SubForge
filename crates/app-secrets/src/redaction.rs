use std::fmt;

use crate::constants::REDACTED;
/// 用于日志/调试输出的密钥遮罩视图。
pub struct RedactedSecret<'a> {
    _value: &'a str,
}

pub fn redact_secret(value: &str) -> RedactedSecret<'_> {
    RedactedSecret { _value: value }
}

impl fmt::Debug for RedactedSecret<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for RedactedSecret<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}
