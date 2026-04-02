//! app-transport：网络传输拟真档位与请求策略。

use std::time::Duration;

use reqwest::Client;
use reqwest::redirect::Policy;
use thiserror::Error;

const STANDARD_TIMEOUT_SEC: u64 = 30;
const STANDARD_MAX_REDIRECTS: usize = 10;
const STANDARD_DEFAULT_USER_AGENT: &str = "SubForge/0.1.0 (standard)";

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("不支持的 network_profile：{0}")]
    UnsupportedProfile(String),
    #[error("HTTP 客户端初始化失败：{0}")]
    ClientBuild(#[from] reqwest::Error),
}

impl TransportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedProfile(_) => "E_CONFIG_INVALID",
            Self::ClientBuild(_) => "E_INTERNAL",
        }
    }
}

pub type TransportResult<T> = Result<T, TransportError>;

pub trait TransportProfile: Send + Sync {
    fn build_client(&self) -> TransportResult<Client>;
    fn request_delay(&self) -> Duration;
}

#[derive(Debug, Clone)]
pub struct StandardProfile {
    timeout: Duration,
    max_redirects: usize,
    request_delay: Duration,
    default_user_agent: &'static str,
}

impl Default for StandardProfile {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(STANDARD_TIMEOUT_SEC),
            max_redirects: STANDARD_MAX_REDIRECTS,
            request_delay: Duration::from_millis(0),
            default_user_agent: STANDARD_DEFAULT_USER_AGENT,
        }
    }
}

impl TransportProfile for StandardProfile {
    fn build_client(&self) -> TransportResult<Client> {
        let client = Client::builder()
            .redirect(Policy::limited(self.max_redirects))
            .timeout(self.timeout)
            .user_agent(self.default_user_agent)
            .danger_accept_invalid_certs(false)
            .build()?;
        Ok(client)
    }

    fn request_delay(&self) -> Duration {
        self.request_delay
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NetworkProfileFactory;

impl NetworkProfileFactory {
    pub fn create(profile: &str) -> TransportResult<Box<dyn TransportProfile>> {
        let profile = profile.trim();
        match profile {
            "" | "standard" => Ok(Box::new(StandardProfile::default())),
            _ => Err(TransportError::UnsupportedProfile(profile.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NetworkProfileFactory, StandardProfile, TransportError, TransportProfile};

    #[test]
    fn standard_profile_builds_https_request() {
        let profile = StandardProfile::default();
        let client = profile.build_client().expect("标准档位构建客户端失败");
        let request = client
            .get("https://example.com")
            .build()
            .expect("构建 HTTPS 请求失败");
        assert_eq!(request.url().scheme(), "https");
    }

    #[test]
    fn factory_returns_error_for_unknown_profile() {
        match NetworkProfileFactory::create("unknown-profile") {
            Ok(_) => panic!("未知档位必须返回错误"),
            Err(error) => {
                assert!(matches!(error, TransportError::UnsupportedProfile(_)));
            }
        }
    }
}
