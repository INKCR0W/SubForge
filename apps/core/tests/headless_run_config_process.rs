use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::header::CONTENT_TYPE;
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const BASE64_SUBSCRIPTION_FIXTURE: &str = "c3M6Ly9ZV1Z6TFRJMU5pMW5ZMjA2Y0dGemMzZHZjbVE9QGhrLmV4YW1wbGUuY29tOjQ0MyNISy1TUwp2bWVzczovL2V5SjJJam9pTWlJc0luQnpJam9pVTBjdFZrMUZVMU1pTENKaFpHUWlPaUp6Wnk1bGVHRnRjR3hsTG1OdmJTSXNJbkJ2Y25RaU9pSTBORE1pTENKcFpDSTZJakV4TVRFeE1URXhMVEV4TVRFdE1URXhNUzB4TVRFeExURXhNVEV4TVRFeE1URXhNU0lzSW1GcFpDSTZJakFpTENKdVpYUWlPaUozY3lJc0luUnNjeUk2SW5Sc2N5SXNJbkJoZEdnaU9pSXZkM01pTENKb2IzTjBJam9pYzJjdVpYaGhiWEJzWlM1amIyMGlmUT09Cm5vdC1hLXVyaS1saW5lCnRyb2phbjovL3Bhc3N3b3JkMTIzQHVzLmV4YW1wbGUuY29tOjQ0Mz9zbmk9dXMuZXhhbXBsZS5jb20jVVMtVHJvamFu";

#[tokio::test]
async fn run_with_config_file_can_refresh_and_export_subscriptions() {
    let temp_root = create_temp_dir("headless-run-c-process");
    let data_dir = temp_root.join("data");
    std::fs::create_dir_all(&data_dir).expect("创建 data 目录失败");

    let listen_port = reserve_available_port().await;
    let admin_token = "headless-process-admin-token";
    let export_token = "headless-process-export-token";
    let config_path = temp_root.join("subforge.toml");

    let (upstream_base, upstream_shutdown) = spawn_fixture_upstream().await;
    std::fs::write(
        &config_path,
        format!(
            r#"
[server]
listen = "127.0.0.1:{listen_port}"
admin_token = "{admin_token}"

[secrets]
backend = "memory"

[plugins]
dirs = ["{plugin_dir}"]

[[sources]]
name = "headless-static"
plugin = "subforge.builtin.static"
[sources.config]
url = "{upstream_base}/sub"

[[profiles]]
name = "headless-profile"
sources = ["headless-static"]
export_token = "{export_token}"
"#,
            plugin_dir = path_to_toml_string(&builtin_plugin_dir()),
            upstream_base = upstream_base,
        ),
    )
    .expect("写入配置文件失败");

    let core_bin = core_binary_path();
    let child = Command::new(core_bin)
        .arg("run")
        .arg("-c")
        .arg(&config_path)
        .arg("--data-dir")
        .arg(&data_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 subforge-core 失败");
    let mut child = ChildGuard::new(child);

    let api_base = format!("http://127.0.0.1:{listen_port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("创建 HTTP 客户端失败");

    wait_until_healthy(&client, &api_base).await;

    let sources_response = client
        .get(format!("{api_base}/api/sources"))
        .bearer_auth(admin_token)
        .send()
        .await
        .expect("读取来源列表失败");
    assert_eq!(sources_response.status(), reqwest::StatusCode::OK);
    let sources_payload: Value = sources_response.json().await.expect("解析来源列表响应失败");
    let source_id = sources_payload
        .get("sources")
        .and_then(Value::as_array)
        .and_then(|sources| sources.first())
        .and_then(|item| item.get("source"))
        .and_then(|source| source.get("id"))
        .and_then(Value::as_str)
        .expect("来源列表中应包含 source.id")
        .to_string();

    let profiles_response = client
        .get(format!("{api_base}/api/profiles"))
        .bearer_auth(admin_token)
        .send()
        .await
        .expect("读取 profile 列表失败");
    assert_eq!(profiles_response.status(), reqwest::StatusCode::OK);
    let profiles_payload: Value = profiles_response
        .json()
        .await
        .expect("解析 profile 列表响应失败");
    let profile_item = profiles_payload
        .get("profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| profiles.first())
        .expect("应存在自动创建的 profile");
    let profile_id = profile_item
        .get("profile")
        .and_then(|profile| profile.get("id"))
        .and_then(Value::as_str)
        .expect("profile.id 应存在")
        .to_string();
    let loaded_export_token = profile_item
        .get("export_token")
        .and_then(Value::as_str)
        .expect("profile.export_token 应存在")
        .to_string();
    assert_eq!(loaded_export_token, export_token);

    let refresh_response = client
        .post(format!("{api_base}/api/sources/{source_id}/refresh"))
        .bearer_auth(admin_token)
        .send()
        .await
        .expect("触发来源刷新失败");
    assert_eq!(refresh_response.status(), reqwest::StatusCode::OK);
    let refresh_payload: Value = refresh_response.json().await.expect("解析刷新响应失败");
    assert_eq!(
        refresh_payload.get("node_count").and_then(Value::as_u64),
        Some(3)
    );

    let raw_response = client
        .get(format!(
            "{api_base}/api/profiles/{profile_id}/raw?token={export_token}"
        ))
        .send()
        .await
        .expect("读取 raw 导出失败");
    assert_eq!(raw_response.status(), reqwest::StatusCode::OK);
    let raw_payload: Value = raw_response.json().await.expect("解析 raw 响应失败");
    assert_eq!(
        raw_payload.get("profile_id").and_then(Value::as_str),
        Some(profile_id.as_str())
    );
    assert_eq!(
        raw_payload.get("node_count").and_then(Value::as_u64),
        Some(3)
    );

    let clash_response = client
        .get(format!(
            "{api_base}/api/profiles/{profile_id}/clash?token={export_token}"
        ))
        .send()
        .await
        .expect("读取 clash 导出失败");
    assert_eq!(clash_response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        clash_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/yaml; charset=utf-8")
    );

    let singbox_response = client
        .get(format!(
            "{api_base}/api/profiles/{profile_id}/sing-box?token={export_token}"
        ))
        .send()
        .await
        .expect("读取 sing-box 导出失败");
    assert_eq!(singbox_response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        singbox_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json; charset=utf-8")
    );

    let base64_response = client
        .get(format!(
            "{api_base}/api/profiles/{profile_id}/base64?token={export_token}"
        ))
        .send()
        .await
        .expect("读取 base64 导出失败");
    assert_eq!(base64_response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        base64_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; charset=utf-8")
    );
    let encoded = base64_response.text().await.expect("读取 base64 文本失败");
    let decoded = BASE64_STANDARD
        .decode(encoded.as_bytes())
        .expect("base64 响应应可解码");
    let decoded_text = String::from_utf8(decoded).expect("base64 解码结果应为 UTF-8");
    assert!(
        decoded_text.lines().any(|line| line.starts_with("ss://")),
        "导出内容应包含 ss:// 链接"
    );

    let shutdown_response = client
        .post(format!("{api_base}/api/system/shutdown"))
        .bearer_auth(admin_token)
        .send()
        .await
        .expect("调用 shutdown 失败");
    assert_eq!(shutdown_response.status(), reqwest::StatusCode::OK);

    let wait_result = wait_for_exit(child.inner_mut(), Duration::from_secs(10))
        .await
        .expect("读取 Core 退出状态失败");
    assert!(wait_result.success(), "Core 退出状态应为成功");

    child.disarm();
    let _ = upstream_shutdown.send(());
    let _ = std::fs::remove_dir_all(&temp_root);
}

async fn wait_until_healthy(client: &reqwest::Client, api_base: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let ready = client
            .get(format!("{api_base}/health"))
            .send()
            .await
            .map(|response| response.status() == reqwest::StatusCode::OK)
            .unwrap_or(false);
        if ready {
            return;
        }
        assert!(Instant::now() < deadline, "Core 启动超时，/health 未就绪");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn spawn_fixture_upstream() -> (String, oneshot::Sender<()>) {
    let app = Router::new().route(
        "/sub",
        get(|| async {
            (
                [(CONTENT_TYPE, "text/plain; charset=utf-8")],
                BASE64_SUBSCRIPTION_FIXTURE,
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("启动上游 fixture 服务失败");
    let addr = listener.local_addr().expect("读取上游地址失败");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let server = axum::serve(listener, app);
        let graceful = server.with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        if let Err(error) = graceful.await {
            panic!("上游 fixture 服务异常退出: {error}");
        }
    });
    (format!("http://{addr}"), shutdown_tx)
}

async fn reserve_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("分配可用端口失败");
    listener.local_addr().expect("读取可用端口失败").port()
}

async fn wait_for_exit(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "等待子进程退出超时",
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn core_binary_path() -> PathBuf {
    for key in ["CARGO_BIN_EXE_subforge-core", "CARGO_BIN_EXE_subforge_core"] {
        if let Ok(path) = std::env::var(key) {
            return PathBuf::from(path);
        }
    }

    let current_exe = std::env::current_exe().expect("读取当前测试进程路径失败");
    let debug_dir = current_exe
        .parent()
        .and_then(Path::parent)
        .expect("推断 target/debug 路径失败");
    let mut candidate = debug_dir.join("subforge-core");
    if cfg!(windows) {
        candidate.set_extension("exe");
    }
    assert!(
        candidate.exists(),
        "未找到 subforge-core 可执行文件，候选路径: {}",
        candidate.display()
    );
    candidate
}

fn builtin_plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/builtins/static")
        .canonicalize()
        .expect("解析内置静态插件目录失败")
}

fn path_to_toml_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn create_temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "subforge-{prefix}-{}",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn inner_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("子进程句柄不存在")
    }

    fn disarm(&mut self) {
        let _ = self.child.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
