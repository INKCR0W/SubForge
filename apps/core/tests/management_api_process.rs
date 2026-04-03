use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::header::CONTENT_TYPE;
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use zip::write::SimpleFileOptions;

const BASE64_SUBSCRIPTION_FIXTURE: &str = "c3M6Ly9ZV1Z6TFRJMU5pMW5ZMjA2Y0dGemMzZHZjbVE9QGhrLmV4YW1wbGUuY29tOjQ0MyNISy1TUwp2bWVzczovL2V5SjJJam9pTWlJc0luQnpJam9pVTBjdFZrMUZVMU1pTENKaFpHUWlPaUp6Wnk1bGVHRnRjR3hsTG1OdmJTSXNJbkJ2Y25RaU9pSTBORE1pTENKcFpDSTZJakV4TVRFeE1URXhMVEV4TVRFdE1URXhNUzB4TVRFeExURXhNVEV4TVRFeE1URXhNU0lzSW1GcFpDSTZJakFpTENKdVpYUWlPaUozY3lJc0luUnNjeUk2SW5Sc2N5SXNJbkJoZEdnaU9pSXZkM01pTENKb2IzTjBJam9pYzJjdVpYaGhiWEJzWlM1amIyMGlmUT09Cm5vdC1hLXVyaS1saW5lCnRyb2phbjovL3Bhc3N3b3JkMTIzQHVzLmV4YW1wbGUuY29tOjQ0Mz9zbmk9dXMuZXhhbXBsZS5jb20jVVMtVHJvamFu";

#[tokio::test]
async fn run_process_management_api_chain_can_refresh_and_export_all_formats() {
    let temp_root = create_temp_dir("management-api-process");
    let data_dir = temp_root.join("data");
    std::fs::create_dir_all(&data_dir).expect("创建 data 目录失败");

    let (upstream_base, upstream_shutdown) = spawn_fixture_upstream().await;
    let listen_port = reserve_available_port().await;
    let core_bin = core_binary_path();
    let child = Command::new(core_bin)
        .arg("run")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(listen_port.to_string())
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--secrets-backend")
        .arg("memory")
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

    let admin_token = std::fs::read_to_string(data_dir.join("admin_token"))
        .expect("读取 admin_token 文件失败")
        .trim()
        .to_string();
    assert_eq!(admin_token.len(), 43, "admin_token 长度应为 43");

    let plugin_zip = build_builtin_plugin_zip_bytes();
    let boundary = "----subforge-process-e2e-boundary";
    let import_body = build_multipart_plugin_body(boundary, &plugin_zip, "builtin-static.zip");
    let import_response = client
        .post(format!("{api_base}/api/plugins/import"))
        .bearer_auth(&admin_token)
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(import_body)
        .send()
        .await
        .expect("导入插件请求执行失败");
    assert_eq!(import_response.status(), reqwest::StatusCode::CREATED);

    let source_response = client
        .post(format!("{api_base}/api/sources"))
        .bearer_auth(&admin_token)
        .json(&json!({
            "plugin_id": "subforge.builtin.static",
            "name": "Process E2E Source",
            "config": {
                "url": format!("{upstream_base}/sub")
            }
        }))
        .send()
        .await
        .expect("创建来源请求执行失败");
    assert_eq!(source_response.status(), reqwest::StatusCode::CREATED);
    let source_payload: Value = source_response.json().await.expect("解析来源响应失败");
    let source_id = source_payload
        .pointer("/source/source/id")
        .and_then(Value::as_str)
        .expect("来源响应缺少 source.id")
        .to_string();

    let profile_response = client
        .post(format!("{api_base}/api/profiles"))
        .bearer_auth(&admin_token)
        .json(&json!({
            "name": "Process E2E Profile",
            "source_ids": [source_id.clone()]
        }))
        .send()
        .await
        .expect("创建 profile 请求执行失败");
    assert_eq!(profile_response.status(), reqwest::StatusCode::CREATED);
    let profile_payload: Value = profile_response
        .json()
        .await
        .expect("解析 profile 响应失败");
    let profile_id = profile_payload
        .pointer("/profile/profile/id")
        .and_then(Value::as_str)
        .expect("profile 响应缺少 id")
        .to_string();
    let export_token = profile_payload
        .pointer("/profile/export_token")
        .and_then(Value::as_str)
        .expect("profile 响应缺少 export_token")
        .to_string();

    let refresh_response = client
        .post(format!("{api_base}/api/sources/{source_id}/refresh"))
        .bearer_auth(&admin_token)
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
    assert_eq!(
        raw_response
            .headers()
            .get("profile-title")
            .and_then(|value| value.to_str().ok()),
        Some("Process E2E Profile")
    );
    let raw_payload: Value = raw_response.json().await.expect("解析 raw 响应失败");
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
    let clash_text = clash_response.text().await.expect("读取 clash 文本失败");
    assert!(clash_text.contains("proxies:"));
    assert!(clash_text.contains("proxy-groups:"));

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
    let singbox_payload: Value = singbox_response
        .json()
        .await
        .expect("解析 sing-box 响应失败");
    assert!(singbox_payload.get("outbounds").is_some());

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
        "base64 解码后内容应包含 ss:// 链接"
    );

    let shutdown_response = client
        .post(format!("{api_base}/api/system/shutdown"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("调用 shutdown 失败");
    assert_eq!(shutdown_response.status(), reqwest::StatusCode::OK);

    let exit_status = wait_for_exit(child.inner_mut(), Duration::from_secs(10))
        .await
        .expect("等待 Core 退出失败");
    assert!(exit_status.success(), "Core 应正常退出");

    child.disarm();
    let _ = upstream_shutdown.send(());
    let _ = std::fs::remove_dir_all(&temp_root);
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
    let address = listener.local_addr().expect("读取上游地址失败");
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
    (format!("http://{address}"), shutdown_tx)
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

fn build_builtin_plugin_zip_bytes() -> Vec<u8> {
    let plugin_dir = builtin_plugin_dir();
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default();
        for file_name in ["plugin.json", "schema.json"] {
            writer
                .start_file(file_name, options)
                .expect("写入 zip 条目失败");
            let bytes = std::fs::read(plugin_dir.join(file_name)).expect("读取插件文件失败");
            writer.write_all(&bytes).expect("写入 zip 数据失败");
        }
        writer.finish().expect("完成 zip 构建失败");
    }
    cursor.into_inner()
}

fn build_multipart_plugin_body(boundary: &str, zip_payload: &[u8], filename: &str) -> Vec<u8> {
    let mut body = Vec::new();
    write!(body, "--{boundary}\r\n").expect("写入 multipart 边界失败");
    write!(
        body,
        "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n"
    )
    .expect("写入 multipart disposition 失败");
    write!(body, "Content-Type: application/zip\r\n\r\n")
        .expect("写入 multipart content-type 失败");
    body.extend_from_slice(zip_payload);
    write!(body, "\r\n--{boundary}--\r\n").expect("写入 multipart 结束边界失败");
    body
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
