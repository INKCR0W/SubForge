//! app-transport：网络传输拟真档位与请求策略。

use std::time::Duration;

use reqwest::Client;
use reqwest::StatusCode;
use reqwest::redirect::Policy;
use thiserror::Error;

const STANDARD_TIMEOUT_SEC: u64 = 30;
const STANDARD_MAX_REDIRECTS: usize = 10;
const STANDARD_DEFAULT_USER_AGENT: &str = "SubForge/0.1.0 (standard)";
const BROWSER_CHROME_TIMEOUT_SEC: u64 = 30;
const BROWSER_CHROME_MAX_REDIRECTS: usize = 10;
const BROWSER_CHROME_REQUEST_DELAY_MS: u64 = 500;
const BROWSER_CHROME_MAX_RETRIES: usize = 3;
const BROWSER_CHROME_DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const EMPTY_HEADER_TEMPLATE: [(&str, &str); 0] = [];
const BROWSER_CHROME_HEADER_TEMPLATE: [(&str, &str); 11] = [
    (
        "sec-ch-ua",
        "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
    ),
    ("sec-ch-ua-mobile", "?0"),
    ("sec-ch-ua-platform", "\"Windows\""),
    ("upgrade-insecure-requests", "1"),
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
    ),
    ("sec-fetch-site", "none"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-user", "?1"),
    ("sec-fetch-dest", "document"),
    ("accept-encoding", "gzip, deflate, br"),
    ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
];

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

pub trait TransportProfile: Send + Sync + std::fmt::Debug {
    fn profile_name(&self) -> &'static str;
    fn build_client(&self) -> TransportResult<Client>;
    fn request_delay(&self) -> Duration;
    fn default_headers(&self) -> &[(&'static str, &'static str)] {
        &EMPTY_HEADER_TEMPLATE
    }
    fn max_retries(&self) -> usize {
        0
    }
    fn is_retryable_status(&self, status_code: StatusCode) -> bool {
        let _ = status_code;
        false
    }
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
    fn profile_name(&self) -> &'static str {
        "standard"
    }

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

#[derive(Debug, Clone)]
pub struct BrowserChromeProfile {
    timeout: Duration,
    max_redirects: usize,
    request_delay: Duration,
    max_retries: usize,
    default_user_agent: &'static str,
}

impl Default for BrowserChromeProfile {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(BROWSER_CHROME_TIMEOUT_SEC),
            max_redirects: BROWSER_CHROME_MAX_REDIRECTS,
            request_delay: Duration::from_millis(BROWSER_CHROME_REQUEST_DELAY_MS),
            max_retries: BROWSER_CHROME_MAX_RETRIES,
            default_user_agent: BROWSER_CHROME_DEFAULT_USER_AGENT,
        }
    }
}

impl TransportProfile for BrowserChromeProfile {
    fn profile_name(&self) -> &'static str {
        "browser_chrome"
    }

    fn build_client(&self) -> TransportResult<Client> {
        let client = Client::builder()
            .redirect(Policy::limited(self.max_redirects))
            .timeout(self.timeout)
            .user_agent(self.default_user_agent)
            .cookie_store(true)
            .danger_accept_invalid_certs(false)
            .build()?;
        Ok(client)
    }

    fn request_delay(&self) -> Duration {
        self.request_delay
    }

    fn default_headers(&self) -> &[(&'static str, &'static str)] {
        &BROWSER_CHROME_HEADER_TEMPLATE
    }

    fn max_retries(&self) -> usize {
        self.max_retries
    }

    fn is_retryable_status(&self, status_code: StatusCode) -> bool {
        matches!(
            status_code,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        )
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NetworkProfileFactory;

impl NetworkProfileFactory {
    pub fn create(profile: &str) -> TransportResult<Box<dyn TransportProfile>> {
        let profile = profile.trim();
        match profile {
            "" | "standard" => Ok(Box::new(StandardProfile::default())),
            "browser_chrome" => Ok(Box::new(BrowserChromeProfile::default())),
            _ => Err(TransportError::UnsupportedProfile(profile.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use axum::Router;
    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
    use axum::routing::get;
    use reqwest::StatusCode;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::{
        BrowserChromeProfile, NetworkProfileFactory, StandardProfile, TransportError,
        TransportProfile,
    };

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
    fn browser_chrome_profile_exposes_chrome_headers_and_retry_policy() {
        let profile = BrowserChromeProfile::default();
        let headers = profile.default_headers();
        assert_eq!(headers.first().map(|(name, _)| *name), Some("sec-ch-ua"));
        assert_eq!(
            headers.get(3).map(|(name, _)| *name),
            Some("upgrade-insecure-requests")
        );
        assert_eq!(
            headers.last().map(|(name, _)| *name),
            Some("accept-language")
        );
        assert_eq!(profile.request_delay(), Duration::from_millis(500));
        assert_eq!(profile.max_retries(), 3);
        assert!(profile.is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(profile.is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
    }

    #[tokio::test]
    async fn browser_chrome_profile_persists_cookies() {
        #[derive(Clone)]
        struct CookieState {
            visits: Arc<AtomicUsize>,
        }

        let visits = Arc::new(AtomicUsize::new(0));
        let state = CookieState {
            visits: visits.clone(),
        };
        let app = Router::new()
            .route(
                "/cookie",
                get(
                    |State(state): State<CookieState>, headers: HeaderMap| async move {
                        let current = state.visits.fetch_add(1, Ordering::SeqCst);
                        let has_cookie = headers
                            .get(COOKIE)
                            .and_then(|value| value.to_str().ok())
                            .map(|value| value.contains("subforge_sid=abc123"))
                            .unwrap_or(false);
                        if current == 0 {
                            (
                                StatusCode::OK,
                                [
                                    (SET_COOKIE, "subforge_sid=abc123; Path=/; HttpOnly"),
                                    (CONTENT_TYPE, "text/plain"),
                                ],
                                "issued".to_string(),
                            )
                        } else if has_cookie {
                            (
                                StatusCode::OK,
                                [
                                    (SET_COOKIE, "subforge_sid=abc123; Path=/; HttpOnly"),
                                    (CONTENT_TYPE, "text/plain"),
                                ],
                                "reused".to_string(),
                            )
                        } else {
                            (
                                StatusCode::BAD_REQUEST,
                                [
                                    (SET_COOKIE, "subforge_sid=abc123; Path=/; HttpOnly"),
                                    (CONTENT_TYPE, "text/plain"),
                                ],
                                "cookie missing".to_string(),
                            )
                        }
                    },
                ),
            )
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("启动测试 HTTP 服务失败");
        let address: SocketAddr = listener.local_addr().expect("读取监听地址失败");
        let base_url = format!("http://{}", address);
        let server: JoinHandle<()> = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("测试 HTTP 服务运行失败");
        });

        let profile = BrowserChromeProfile::default();
        let client = profile
            .build_client()
            .expect("browser_chrome 客户端构建失败");
        let first = client
            .get(format!("{base_url}/cookie"))
            .send()
            .await
            .expect("第一次请求失败");
        assert_eq!(first.status(), StatusCode::OK);

        let second = client
            .get(format!("{base_url}/cookie"))
            .send()
            .await
            .expect("第二次请求失败");
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(visits.load(Ordering::SeqCst), 2);

        server.abort();
    }

    #[test]
    fn factory_resolves_browser_chrome_profile() {
        let profile =
            NetworkProfileFactory::create("browser_chrome").expect("browser_chrome 档位应可创建");
        assert_eq!(profile.request_delay(), Duration::from_millis(500));
        assert_eq!(profile.max_retries(), 3);
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
