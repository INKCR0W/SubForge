use thiserror::Error;
pub type SecretResult<T> = Result<T, SecretError>;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("无效 scope：{0}")]
    InvalidScope(String),
    #[error("无效 key：{0}")]
    InvalidKey(String),
    #[error("密钥不存在：{0}")]
    SecretMissing(String),
    #[error("后端错误：{0}")]
    Backend(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误：{0}")]
    Serde(#[from] serde_json::Error),
}

impl SecretError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidScope(_) | Self::InvalidKey(_) => "E_CONFIG_INVALID",
            Self::SecretMissing(_) => "E_SECRET_MISSING",
            Self::Backend(_) | Self::Io(_) | Self::Serde(_) => "E_INTERNAL",
        }
    }
}
