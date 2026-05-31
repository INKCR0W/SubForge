use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use app_transport::{StandardProfile, TransportProfile};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn guarded_dns_client_ignores_system_http_proxy() {
    assert_guarded_dns_builder_ignores_system_http_proxy(|profile| {
        profile.build_client_with_guarded_dns(Duration::from_millis(200), 0, None, |_ip| false)
    })
    .await;
}

#[tokio::test]
async fn guarded_dns_no_auto_decode_client_ignores_system_http_proxy() {
    assert_guarded_dns_builder_ignores_system_http_proxy(|profile| {
        profile.build_client_with_guarded_dns_no_auto_decode(
            Duration::from_millis(200),
            0,
            None,
            |_ip| false,
        )
    })
    .await;
}

async fn assert_guarded_dns_builder_ignores_system_http_proxy(
    build_client: impl FnOnce(&StandardProfile) -> app_transport::TransportResult<reqwest::Client>,
) {
    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let proxy_hits_for_task = Arc::clone(&proxy_hits);
    let (proxy_address_tx, proxy_address_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let proxy_task = tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("测试代理应可监听");
        proxy_address_tx
            .send(listener.local_addr().expect("读取测试代理地址失败"))
            .expect("测试代理地址应可发送");

        tokio::select! {
            accepted = listener.accept() => {
                let (_stream, _peer) = accepted.expect("测试代理 accept 失败");
                proxy_hits_for_task.fetch_add(1, Ordering::SeqCst);
            }
            _ = shutdown_rx => {}
        }
    });
    let proxy_address = proxy_address_rx.await.expect("应收到测试代理地址");

    let proxy_url = format!("http://{proxy_address}");
    let client = {
        let _lock = ENV_TEST_LOCK.lock().expect("代理环境变量测试锁不应中毒");
        let env_guard = EnvVarGuard::apply_many(&[
            ("HTTP_PROXY", Some(proxy_url.as_str())),
            ("http_proxy", Some(proxy_url.as_str())),
            ("HTTPS_PROXY", Some(proxy_url.as_str())),
            ("https_proxy", Some(proxy_url.as_str())),
            ("ALL_PROXY", Some(proxy_url.as_str())),
            ("all_proxy", Some(proxy_url.as_str())),
            ("NO_PROXY", Some("")),
            ("no_proxy", Some("")),
            ("REQUEST_METHOD", None),
        ]);
        let profile = StandardProfile::default();
        let client = build_client(&profile).expect("带安全 DNS guard 的客户端应可构建");
        drop(env_guard);
        client
    };

    let _ = client
        .get("http://subforge-proxy-bypass.invalid/should-not-hit-proxy")
        .send()
        .await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        proxy_hits.load(Ordering::SeqCst),
        0,
        "不可信 guarded DNS 客户端不得把请求交给系统代理"
    );

    let _ = shutdown_tx.send(());
    let _ = proxy_task.await;
}

struct EnvVarGuard {
    originals: Vec<(&'static str, Option<String>)>,
}

impl EnvVarGuard {
    fn apply_many(vars: &[(&'static str, Option<&str>)]) -> Self {
        let originals = vars
            .iter()
            .map(|(name, _value)| (*name, std::env::var(name).ok()))
            .collect::<Vec<_>>();
        for (name, value) in vars {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
        Self { originals }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (name, value) in self.originals.iter().rev() {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}
