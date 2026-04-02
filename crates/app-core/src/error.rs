use app_plugin_runtime::PluginRuntimeError;
use app_secrets::SecretError;
use app_storage::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("插件运行时错误：{0}")]
    PluginRuntime(#[from] PluginRuntimeError),
    #[error("存储层错误：{0}")]
    Storage(#[from] StorageError),
    #[error("密钥存储错误：{0}")]
    Secret(#[from] SecretError),
    #[error("文件系统错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("时间格式化失败：{0}")]
    TimeFormat(#[from] time::error::Format),
    #[error("传输层错误：{0}")]
    Transport(#[from] app_transport::TransportError),
    #[error("随机数生成失败：{0}")]
    Random(String),
    #[error("插件已安装：{0}")]
    PluginAlreadyInstalled(String),
    #[error("配置校验失败：{0}")]
    ConfigInvalid(String),
    #[error("插件不存在：{0}")]
    PluginNotFound(String),
    #[error("来源不存在：{0}")]
    SourceNotFound(String),
    #[error("订阅拉取失败：{0}")]
    SubscriptionFetch(String),
    #[error("订阅解析失败：{0}")]
    SubscriptionParse(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::PluginRuntime(error) => error.code(),
            Self::ConfigInvalid(_) => "E_CONFIG_INVALID",
            Self::PluginNotFound(_) | Self::SourceNotFound(_) => "E_NOT_FOUND",
            Self::PluginAlreadyInstalled(_) => "E_PLUGIN_INVALID",
            Self::SubscriptionParse(_) => "E_PARSE",
            Self::Storage(_)
            | Self::Secret(_)
            | Self::Io(_)
            | Self::TimeFormat(_)
            | Self::Random(_)
            | Self::SubscriptionFetch(_) => "E_INTERNAL",
            Self::Transport(error) => error.code(),
        }
    }
}

impl From<getrandom::Error> for CoreError {
    fn from(error: getrandom::Error) -> Self {
        Self::Random(error.to_string())
    }
}
