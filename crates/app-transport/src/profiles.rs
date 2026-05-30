use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use reqwest::StatusCode;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;

use crate::error::TransportResult;

const STANDARD_TIMEOUT_SEC: u64 = 30;
const STANDARD_MAX_REDIRECTS: usize = 10;
const STANDARD_DEFAULT_USER_AGENT: &str = "clash.meta";
const BROWSER_CHROME_TIMEOUT_SEC: u64 = 30;
const BROWSER_CHROME_MAX_REDIRECTS: usize = 10;
const BROWSER_CHROME_REQUEST_DELAY_MS: u64 = 500;
const BROWSER_CHROME_MAX_RETRIES: usize = 3;
const BROWSER_CHROME_DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const BROWSER_FIREFOX_TIMEOUT_SEC: u64 = 30;
const BROWSER_FIREFOX_MAX_REDIRECTS: usize = 10;
const BROWSER_FIREFOX_REQUEST_DELAY_MS: u64 = 500;
const BROWSER_FIREFOX_MAX_RETRIES: usize = 3;
const BROWSER_FIREFOX_DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:126.0) Gecko/20100101 Firefox/126.0";
const WEBVIEW_ASSISTED_TIMEOUT_SEC: u64 = 30;
const WEBVIEW_ASSISTED_MAX_REDIRECTS: usize = 10;
const WEBVIEW_ASSISTED_DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) SubForgeWebView/1.0 Safari/537.36";
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
const BROWSER_FIREFOX_HEADER_TEMPLATE: [(&str, &str); 9] = [
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
    ),
    ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
    ("accept-encoding", "gzip, deflate, br"),
    ("upgrade-insecure-requests", "1"),
    ("sec-fetch-dest", "document"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-site", "none"),
    ("sec-fetch-user", "?1"),
    ("te", "trailers"),
];

pub trait TransportProfile: Send + Sync + std::fmt::Debug {
    fn profile_name(&self) -> &'static str;
    fn timeout(&self) -> Duration;
    fn max_redirects(&self) -> usize;
    fn default_user_agent(&self) -> &'static str;
    fn uses_cookie_store(&self) -> bool {
        false
    }
    fn build_client(&self) -> TransportResult<Client> {
        self.build_client_with_limits(self.timeout(), self.max_redirects(), None)
    }

    fn build_client_with_limits(
        &self,
        timeout: Duration,
        max_redirects: usize,
        redirect_policy: Option<Policy>,
    ) -> TransportResult<Client> {
        build_client_with_settings(
            timeout,
            max_redirects,
            self.default_user_agent(),
            self.uses_cookie_store(),
            redirect_policy,
            None,
            Client::builder(),
        )
    }
    fn build_client_with_limits_no_auto_decode(
        &self,
        timeout: Duration,
        max_redirects: usize,
        redirect_policy: Option<Policy>,
    ) -> TransportResult<Client> {
        // 订阅拉取需要自行按 MAX_SUBSCRIPTION_BYTES 做 raw/decode 双重限制；
        // 禁用 reqwest 自动解压，避免底层在限制前完成解压并移除 Content-Encoding。
        let builder = Client::builder()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();
        build_client_with_settings(
            timeout,
            max_redirects,
            self.default_user_agent(),
            self.uses_cookie_store(),
            redirect_policy,
            None,
            builder,
        )
    }
    fn build_client_with_guarded_dns(
        &self,
        timeout: Duration,
        max_redirects: usize,
        redirect_policy: Option<Policy>,
        is_forbidden_ip: fn(IpAddr) -> bool,
    ) -> TransportResult<Client> {
        // 该路径用于插件脚本等不可信网络访问：在 reqwest connector 的 DNS
        // 解析点过滤真实连接地址，避免预解析检查与实际连接之间发生 DNS TOCTOU。
        build_client_with_settings(
            timeout,
            max_redirects,
            self.default_user_agent(),
            self.uses_cookie_store(),
            redirect_policy,
            Some(is_forbidden_ip),
            Client::builder(),
        )
    }
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

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn max_redirects(&self) -> usize {
        self.max_redirects
    }

    fn default_user_agent(&self) -> &'static str {
        self.default_user_agent
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

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn max_redirects(&self) -> usize {
        self.max_redirects
    }

    fn default_user_agent(&self) -> &'static str {
        self.default_user_agent
    }

    fn uses_cookie_store(&self) -> bool {
        true
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

#[derive(Debug, Clone)]
pub struct BrowserFirefoxProfile {
    timeout: Duration,
    max_redirects: usize,
    request_delay: Duration,
    max_retries: usize,
    default_user_agent: &'static str,
}

impl Default for BrowserFirefoxProfile {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(BROWSER_FIREFOX_TIMEOUT_SEC),
            max_redirects: BROWSER_FIREFOX_MAX_REDIRECTS,
            request_delay: Duration::from_millis(BROWSER_FIREFOX_REQUEST_DELAY_MS),
            max_retries: BROWSER_FIREFOX_MAX_RETRIES,
            default_user_agent: BROWSER_FIREFOX_DEFAULT_USER_AGENT,
        }
    }
}

impl TransportProfile for BrowserFirefoxProfile {
    fn profile_name(&self) -> &'static str {
        "browser_firefox"
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn max_redirects(&self) -> usize {
        self.max_redirects
    }

    fn default_user_agent(&self) -> &'static str {
        self.default_user_agent
    }

    fn uses_cookie_store(&self) -> bool {
        true
    }

    fn request_delay(&self) -> Duration {
        self.request_delay
    }

    fn default_headers(&self) -> &[(&'static str, &'static str)] {
        &BROWSER_FIREFOX_HEADER_TEMPLATE
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

#[derive(Debug, Clone)]
pub struct WebviewAssistedProfile {
    timeout: Duration,
    max_redirects: usize,
    request_delay: Duration,
    default_user_agent: &'static str,
}

impl Default for WebviewAssistedProfile {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(WEBVIEW_ASSISTED_TIMEOUT_SEC),
            max_redirects: WEBVIEW_ASSISTED_MAX_REDIRECTS,
            request_delay: Duration::from_millis(0),
            default_user_agent: WEBVIEW_ASSISTED_DEFAULT_USER_AGENT,
        }
    }
}

impl TransportProfile for WebviewAssistedProfile {
    fn profile_name(&self) -> &'static str {
        "webview_assisted"
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn max_redirects(&self) -> usize {
        self.max_redirects
    }

    fn default_user_agent(&self) -> &'static str {
        self.default_user_agent
    }

    fn uses_cookie_store(&self) -> bool {
        true
    }

    fn request_delay(&self) -> Duration {
        self.request_delay
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NetworkProfileFactory;

impl NetworkProfileFactory {
    pub fn create(profile: &str) -> crate::TransportResult<Box<dyn TransportProfile>> {
        let profile = profile.trim();
        match profile {
            "" | "standard" => Ok(Box::new(StandardProfile::default())),
            "browser_chrome" => Ok(Box::new(BrowserChromeProfile::default())),
            "browser_firefox" => Ok(Box::new(BrowserFirefoxProfile::default())),
            "webview_assisted" => Ok(Box::new(WebviewAssistedProfile::default())),
            _ => Err(crate::TransportError::UnsupportedProfile(
                profile.to_string(),
            )),
        }
    }
}

fn build_client_with_settings(
    timeout: Duration,
    max_redirects: usize,
    user_agent: &'static str,
    use_cookie_store: bool,
    redirect_policy: Option<Policy>,
    is_forbidden_ip: Option<fn(IpAddr) -> bool>,
    builder: reqwest::ClientBuilder,
) -> TransportResult<Client> {
    let mut builder = builder
        .redirect(redirect_policy.unwrap_or_else(|| Policy::limited(max_redirects)))
        .timeout(timeout)
        .user_agent(user_agent)
        .danger_accept_invalid_certs(false);
    if let Some(is_forbidden_ip) = is_forbidden_ip {
        builder = builder.dns_resolver(Arc::new(GuardedDnsResolver::new(is_forbidden_ip)));
    }
    if use_cookie_store {
        builder = builder.cookie_store(true);
    }
    let client = builder.build()?;
    Ok(client)
}

#[derive(Debug)]
struct GuardedDnsResolver {
    is_forbidden_ip: fn(IpAddr) -> bool,
}

impl GuardedDnsResolver {
    fn new(is_forbidden_ip: fn(IpAddr) -> bool) -> Self {
        Self { is_forbidden_ip }
    }
}

impl Resolve for GuardedDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let is_forbidden_ip = self.is_forbidden_ip;
        Box::pin(async move {
            let resolved = tokio::task::spawn_blocking(move || {
                (host.as_str(), 0)
                    .to_socket_addrs()
                    .map(|addresses| addresses.collect::<Vec<_>>())
            })
            .await
            .map_err(|error| format!("DNS 解析任务失败：{error}"))?
            .map_err(|error| format!("DNS 解析失败：{error}"))?;

            let safe_addresses = resolved
                .into_iter()
                .filter(|address| !(is_forbidden_ip)(address.ip()))
                .collect::<Vec<_>>();
            if safe_addresses.is_empty() {
                return Err("DNS guard 拦截：目标地址不允许（内网/保留地址）".into());
            }

            Ok(Box::new(safe_addresses.into_iter()) as Addrs)
        })
    }
}
