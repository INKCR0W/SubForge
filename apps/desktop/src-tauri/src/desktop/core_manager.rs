use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::helpers::{
    abort_events_task, parse_gui_close_behavior, read_bootstrap_line, resolve_core_data_dir,
    resolve_workspace_root, spawn_log_reader, terminate_child,
};
use super::types::{CoreState, CoreStatusPayload, GuiCloseBehavior};

pub(crate) struct CoreManager {
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) core_data_dir: PathBuf,
    pub(super) state: Mutex<CoreState>,
    pub(super) client: reqwest::Client,
}

impl CoreManager {
    pub(crate) fn new() -> Result<Self> {
        let workspace_root = resolve_workspace_root();
        let core_data_dir = resolve_core_data_dir(workspace_root.as_deref())?;
        fs::create_dir_all(&core_data_dir).with_context(|| {
            format!(
                "创建 Desktop Core 数据目录失败: {}",
                core_data_dir.display()
            )
        })?;

        Ok(Self {
            workspace_root,
            core_data_dir,
            state: Mutex::new(CoreState::default()),
            client: reqwest::Client::new(),
        })
    }

    pub(crate) async fn start_core(&self, app_handle: &AppHandle) -> Result<CoreStatusPayload> {
        let already_running = {
            let mut state = self.lock_state()?;
            self.reap_child_if_exited(&mut state)?;
            state.child.is_some()
        };

        if already_running {
            return self.compose_status_payload().await;
        }

        let mut command = self
            .build_core_launch_command(app_handle)
            .context("构建 Core 启动命令失败")?;
        command
            .arg("--host")
            .arg(super::types::DEFAULT_CORE_HOST)
            .arg("--port")
            .arg(super::types::DEFAULT_CORE_PORT.to_string())
            .arg("--gui-mode")
            .arg("--data-dir")
            .arg(&self.core_data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_desktop_secret_backend_args(&mut command);
        apply_windows_spawn_flags(&mut command);

        let (bootstrap, child) = match launch_child_and_read_bootstrap(command) {
            Ok(result) => result,
            Err(error) => {
                self.clear_started_child_state();
                return Err(error);
            }
        };
        self.store_started_child_state(bootstrap, child)?;

        if let Err(error) = self.wait_until_healthy(Duration::from_secs(5)).await {
            self.clear_started_child_state();
            return Err(error);
        }
        self.compose_status_payload().await
    }

    pub(crate) async fn stop_core(&self) -> Result<CoreStatusPayload> {
        let (mut maybe_child, base_url, admin_token) = {
            let mut state = self.lock_state()?;
            self.reap_child_if_exited(&mut state)?;
            self.try_restore_admin_token(&mut state);
            abort_events_task(&mut state);
            (
                state.child.take(),
                state.base_url.clone(),
                state.admin_token.clone(),
            )
        };

        if let Some(child) = maybe_child.as_mut() {
            terminate_child(child).context("停止 Core 进程失败")?;
        } else if let Some(token) = admin_token {
            let _ = self.request_remote_shutdown(&base_url, &token).await;
        }

        {
            let mut state = self.lock_state()?;
            state.admin_token = None;
            state.pid = None;
            state.version = None;
        }

        self.compose_status_payload().await
    }

    pub(crate) async fn compose_status_payload(&self) -> Result<CoreStatusPayload> {
        let (base_url, pid, fallback_version) = {
            let mut state = self.lock_state()?;
            self.reap_child_if_exited(&mut state)?;
            self.try_restore_admin_token(&mut state);
            (state.base_url.clone(), state.pid, state.version.clone())
        };

        let current_token = self.current_admin_token()?;
        let authenticated_version = match current_token.as_deref() {
            Some(token) => {
                self.fetch_authenticated_status_version(&base_url, token)
                    .await
            }
            None => None,
        };
        let fallback_health_version = match current_token {
            Some(_) => None,
            None => self.fetch_health_version(&base_url).await,
        };
        let running = authenticated_version.is_some() || fallback_health_version.is_some();
        if authenticated_version.is_some() {
            let mut state = self.lock_state()?;
            self.try_restore_admin_token(&mut state);
        }

        Ok(CoreStatusPayload {
            running,
            base_url,
            version: authenticated_version
                .or(fallback_health_version)
                .or(fallback_version),
            pid,
        })
    }

    pub(super) async fn resolve_gui_close_behavior(&self) -> GuiCloseBehavior {
        let settings = match self.fetch_system_settings().await {
            Ok(settings) => settings,
            Err(_) => return GuiCloseBehavior::TrayMinimize,
        };
        parse_gui_close_behavior(&settings)
    }

    async fn wait_until_healthy(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let (base_url, admin_token) = {
                let state = self.lock_state()?;
                (state.base_url.clone(), state.admin_token.clone())
            };
            let admin_token =
                admin_token.ok_or_else(|| anyhow!("Core 启动失败：缺少管理 token"))?;

            if self
                .fetch_authenticated_status_version(&base_url, &admin_token)
                .await
                .is_some()
            {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(anyhow!("Core 启动超时，认证状态接口未就绪"));
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub(super) async fn fetch_authenticated_status_version(
        &self,
        base_url: &str,
        admin_token: &str,
    ) -> Option<String> {
        fetch_authenticated_status_version_with_client(&self.client, base_url, admin_token).await
    }

    pub(super) async fn fetch_health_version(&self, base_url: &str) -> Option<String> {
        let url = format!("{base_url}/health");
        let response = self.client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }

        let value = response.json::<Value>().await.ok()?;
        value
            .get("version")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    pub(super) fn current_admin_token(&self) -> Result<Option<String>> {
        let mut state = self.lock_state()?;
        self.reap_child_if_exited(&mut state)?;
        self.try_restore_admin_token(&mut state);
        Ok(state.admin_token.clone())
    }

    pub(super) async fn ensure_authenticated_core_available(
        &self,
        base_url: &str,
        admin_token: Option<&str>,
    ) -> Result<()> {
        let token = admin_token.ok_or_else(|| {
            anyhow!("当前会话没有管理 token，请先通过 GUI 启动 Core 再调用管理 API")
        })?;
        if self
            .fetch_authenticated_status_version(base_url, token)
            .await
            .is_some()
        {
            return Ok(());
        }
        Err(anyhow!("Core 未运行、不可达或身份校验失败"))
    }

    pub(super) fn clear_started_child_state(&self) {
        let mut maybe_child = {
            let mut state = match self.lock_state() {
                Ok(state) => state,
                Err(_) => return,
            };
            abort_events_task(&mut state);
            let child = state.child.take();
            *state = CoreState::default();
            child
        };

        if let Some(child) = maybe_child.as_mut() {
            let _ = terminate_child(child);
        }
    }

    fn store_started_child_state(
        &self,
        bootstrap: super::types::CoreBootstrapLine,
        mut child: std::process::Child,
    ) -> Result<()> {
        let base_url = format!("http://{}:{}", bootstrap.listen_addr, bootstrap.port);
        let version = bootstrap.version;
        let admin_token = bootstrap.admin_token;
        let pid = child.id();

        let mut state = match self.lock_state() {
            Ok(state) => state,
            Err(error) => {
                let _ = terminate_child(&mut child);
                return Err(error);
            }
        };
        state.base_url = base_url;
        state.version = Some(version);
        state.pid = Some(pid);
        state.admin_token = Some(admin_token);
        state.child = Some(child);
        Ok(())
    }

    pub(super) fn lock_state(&self) -> Result<MutexGuard<'_, CoreState>> {
        self.state
            .lock()
            .map_err(|_| anyhow!("CoreManager 状态锁异常"))
    }

    pub(super) fn reap_child_if_exited(&self, state: &mut CoreState) -> Result<()> {
        if let Some(child) = state.child.as_mut()
            && child
                .try_wait()
                .context("读取 Core 进程状态失败")?
                .is_some()
        {
            state.child = None;
            state.admin_token = None;
            state.pid = None;
            abort_events_task(state);
        }
        Ok(())
    }

    pub(super) fn try_restore_admin_token(&self, state: &mut CoreState) {
        if state.admin_token.is_some() {
            return;
        }
        // 磁盘上的 admin_token 只能补全当前会话已持有的 child/PID 状态。
        // 空状态下读取持久 token 无法证明默认端口属于我们启动的 Core，可能泄露给抢占端口的假服务。
        if state.child.is_none() || state.pid.is_none() {
            return;
        }

        let token_path = self.core_data_dir.join("admin_token");
        let token = fs::read_to_string(&token_path)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if let Some(token) = token {
            state.admin_token = Some(token);
        }
    }

    pub(super) async fn fetch_system_settings(&self) -> Result<BTreeMap<String, String>> {
        let response = self
            .proxy_api_call(super::types::CoreApiRequest {
                method: "GET".to_string(),
                path: "/api/system/settings".to_string(),
                body: None,
            })
            .await?;

        if response.status != 200 {
            return Err(anyhow!(
                "读取 /api/system/settings 失败，HTTP 状态码: {}",
                response.status
            ));
        }

        let payload: super::types::SettingsResponse =
            serde_json::from_str(&response.body).context("解析系统设置响应失败")?;
        Ok(payload.settings)
    }

    async fn request_remote_shutdown(&self, base_url: &str, admin_token: &str) -> Result<()> {
        let url = format!("{base_url}/api/system/shutdown");
        let response = self
            .client
            .post(&url)
            .bearer_auth(admin_token)
            .send()
            .await
            .with_context(|| format!("请求 Core 远程关闭失败: {url}"))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Core 远程关闭返回非成功状态: {}",
                response.status()
            ));
        }
        Ok(())
    }

    fn build_core_launch_command(&self, app_handle: &AppHandle) -> Result<Command> {
        if cfg!(debug_assertions)
            && let Some(workspace_root) = self.workspace_root.as_ref()
        {
            if let Some(binary_path) = resolve_workspace_core_binary_path(workspace_root) {
                let mut command = Command::new(binary_path);
                command.arg("run");
                return Ok(command);
            }

            let mut command = Command::new("cargo");
            command
                .current_dir(workspace_root)
                .arg("run")
                .arg("-p")
                .arg("subforge-core")
                .arg("--")
                .arg("run");
            return Ok(command);
        }

        if let Some(sidecar_path) = self.resolve_sidecar_path(app_handle) {
            let mut command = Command::new(&sidecar_path);
            command.arg("run");
            return Ok(command);
        }

        if let Some(workspace_root) = self.workspace_root.as_ref() {
            let mut command = Command::new("cargo");
            command
                .current_dir(workspace_root)
                .arg("run")
                .arg("-p")
                .arg("subforge-core")
                .arg("--")
                .arg("run");
            return Ok(command);
        }

        Err(anyhow!(
            "未找到可用的 subforge-core（既无 sidecar，也无可用 workspace）"
        ))
    }

    fn resolve_sidecar_path(&self, app_handle: &AppHandle) -> Option<PathBuf> {
        if let Ok(explicit) = std::env::var("SUBFORGE_CORE_BINARY") {
            let path = PathBuf::from(explicit.trim());
            if path.is_file() {
                return Some(path);
            }
        }

        let mut candidate_dirs = Vec::new();
        if let Ok(current_exe) = std::env::current_exe()
            && let Some(parent) = current_exe.parent()
        {
            candidate_dirs.push(parent.to_path_buf());

            #[cfg(target_os = "macos")]
            {
                if let Some(resources_dir) = parent.parent().map(|path| path.join("Resources")) {
                    candidate_dirs.push(resources_dir);
                }
            }
        }
        if let Ok(resource_dir) = app_handle.path().resource_dir() {
            candidate_dirs.push(resource_dir);
        }

        let mut candidate_file_names = Vec::new();
        let target_triple = option_env!("TARGET").unwrap_or("unknown-target");
        #[cfg(windows)]
        {
            candidate_file_names.push("subforge-core.exe".to_string());
            candidate_file_names.push(format!("subforge-core-{target_triple}.exe"));
        }
        #[cfg(not(windows))]
        {
            candidate_file_names.push("subforge-core".to_string());
            candidate_file_names.push(format!("subforge-core-{target_triple}"));
        }

        for dir in candidate_dirs {
            for file_name in &candidate_file_names {
                let candidate = dir.join(file_name);
                if candidate.is_file() && !is_placeholder_sidecar(&candidate) {
                    return Some(candidate);
                }
            }
        }

        None
    }
}

fn resolve_workspace_core_binary_path(workspace_root: &std::path::Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let path = workspace_root
        .join("target")
        .join("debug")
        .join("subforge-core.exe");
    #[cfg(not(windows))]
    let path = workspace_root
        .join("target")
        .join("debug")
        .join("subforge-core");

    if path.is_file() { Some(path) } else { None }
}

fn is_placeholder_sidecar(path: &std::path::Path) -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }

    const PLACEHOLDER_BYTES: &[u8] = b"subforge-core sidecar placeholder";
    fs::read(path)
        .map(|bytes| bytes == PLACEHOLDER_BYTES)
        .unwrap_or(false)
}

fn launch_child_and_read_bootstrap(
    mut command: Command,
) -> Result<(super::types::CoreBootstrapLine, std::process::Child)> {
    let mut child = command.spawn().context("启动 subforge-core 失败")?;
    match read_bootstrap_from_child(&mut child) {
        Ok(bootstrap) => Ok((bootstrap, child)),
        Err(error) => {
            terminate_child(&mut child).ok();
            Err(error)
        }
    }
}

fn read_bootstrap_from_child(
    child: &mut std::process::Child,
) -> Result<super::types::CoreBootstrapLine> {
    let stdout = child
        .stdout
        .take()
        .context("读取 subforge-core stdout 失败")?;
    let stderr = child
        .stderr
        .take()
        .context("读取 subforge-core stderr 失败")?;

    let bootstrap = read_bootstrap_line(stdout, Duration::from_secs(10))?;
    spawn_log_reader(stderr, "core-stderr");
    Ok(bootstrap)
}

pub(super) async fn fetch_authenticated_status_version_with_client(
    client: &reqwest::Client,
    base_url: &str,
    admin_token: &str,
) -> Option<String> {
    let url = format!("{base_url}/api/system/status");
    let response = client.get(url).bearer_auth(admin_token).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }

    let value = response.json::<Value>().await.ok()?;
    let status_is_ok = value
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("ok"));
    if !status_is_ok {
        return None;
    }

    value
        .get("version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(windows)]
fn apply_windows_spawn_flags(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_windows_spawn_flags(_command: &mut Command) {}

fn apply_desktop_secret_backend_args(command: &mut Command) {
    if let Ok(backend) = std::env::var("SUBFORGE_DESKTOP_SECRETS_BACKEND") {
        let backend = backend.trim();
        if !backend.is_empty() {
            command.arg("--secrets-backend").arg(backend);
            if backend.eq_ignore_ascii_case("file")
                && let Ok(secret_key) = std::env::var("SUBFORGE_DESKTOP_SECRET_KEY")
            {
                let secret_key = secret_key.trim();
                if !secret_key.is_empty() {
                    command.arg("--secret-key").arg(secret_key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreManager, apply_desktop_secret_backend_args};
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;

    use crate::desktop::types::CoreState;

    static DESKTOP_SECRET_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct DesktopSecretEnvGuard {
        _lock: MutexGuard<'static, ()>,
        previous_backend: Option<OsString>,
        previous_secret_key: Option<OsString>,
    }

    impl DesktopSecretEnvGuard {
        fn set(backend: Option<&str>, secret_key: Option<&str>) -> Self {
            let lock = DESKTOP_SECRET_ENV_LOCK
                .lock()
                .expect("Desktop secret 环境变量测试锁异常");
            let guard = Self {
                _lock: lock,
                previous_backend: std::env::var_os("SUBFORGE_DESKTOP_SECRETS_BACKEND"),
                previous_secret_key: std::env::var_os("SUBFORGE_DESKTOP_SECRET_KEY"),
            };

            set_env_var("SUBFORGE_DESKTOP_SECRETS_BACKEND", backend.map(OsStr::new));
            set_env_var("SUBFORGE_DESKTOP_SECRET_KEY", secret_key.map(OsStr::new));

            guard
        }
    }

    impl Drop for DesktopSecretEnvGuard {
        fn drop(&mut self) {
            set_env_var(
                "SUBFORGE_DESKTOP_SECRETS_BACKEND",
                self.previous_backend.as_deref(),
            );
            set_env_var(
                "SUBFORGE_DESKTOP_SECRET_KEY",
                self.previous_secret_key.as_deref(),
            );
        }
    }

    fn set_env_var(name: &str, value: Option<&OsStr>) {
        // SAFETY: 这些测试通过 DESKTOP_SECRET_ENV_LOCK 串行化对相关进程环境变量的读写，
        // 并在 guard drop 时恢复原值，避免同组测试并发修改全局环境。
        unsafe {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    fn command_args_with_secret_env(
        backend: Option<&str>,
        secret_key: Option<&str>,
    ) -> Vec<String> {
        let _env = DesktopSecretEnvGuard::set(backend, secret_key);
        let mut command = Command::new("subforge-core");
        apply_desktop_secret_backend_args(&mut command);
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应晚于 UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "subforge-desktop-{prefix}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn compose_status_does_not_send_persisted_admin_token_to_untrusted_port() {
        let captured_authorizations = Arc::new(Mutex::new(Vec::<String>::new()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("启动假 Core 服务失败");
        let address = listener.local_addr().expect("读取假 Core 地址失败");
        let server_captured_authorizations = Arc::clone(&captured_authorizations);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _peer)) = listener.accept().await else {
                    break;
                };
                let server_captured_authorizations = Arc::clone(&server_captured_authorizations);
                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 4096];
                    let Ok(read_bytes) = stream.read(&mut buffer).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buffer[..read_bytes]);
                    if let Some(header_value) = request
                        .lines()
                        .find_map(|line| line.strip_prefix("authorization: "))
                        .or_else(|| {
                            request
                                .lines()
                                .find_map(|line| line.strip_prefix("Authorization: "))
                        })
                    {
                        server_captured_authorizations
                            .lock()
                            .expect("捕获列表锁异常")
                            .push(header_value.to_string());
                    }

                    let first_line = request.lines().next().unwrap_or_default();
                    let path = first_line.split_whitespace().nth(1).unwrap_or_default();
                    let (status, body) = match path {
                        "/health" => (
                            "200 OK",
                            json!({ "status": "ok", "version": "fake-core" }).to_string(),
                        ),
                        "/api/system/status" => ("401 Unauthorized", String::new()),
                        _ => ("404 Not Found", String::new()),
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });

        let core_data_dir = unique_temp_dir("persisted-admin-token");
        std::fs::create_dir_all(&core_data_dir).expect("创建测试数据目录失败");
        std::fs::write(core_data_dir.join("admin_token"), "persisted-admin-token\n")
            .expect("写入测试 admin_token 失败");

        let manager = CoreManager {
            workspace_root: None,
            core_data_dir: core_data_dir.clone(),
            state: Mutex::new(CoreState {
                base_url: format!("http://{address}"),
                ..CoreState::default()
            }),
            client: reqwest::Client::new(),
        };

        let _status = manager
            .compose_status_payload()
            .await
            .expect("读取 Core 状态不应失败");

        assert!(
            captured_authorizations
                .lock()
                .expect("捕获列表锁异常")
                .is_empty(),
            "没有可信 child/PID/instance proof 时，不得向抢占端口发送持久化 admin_token"
        );

        let _ = std::fs::remove_dir_all(core_data_dir);
    }

    #[test]
    fn default_launch_does_not_pass_hardcoded_file_secret_key() {
        let args = command_args_with_secret_env(None, None);

        assert!(
            !args.iter().any(|arg| arg == "subforge-desktop-secret-key"),
            "默认 Desktop 启动参数不得包含源码硬编码的 file SecretStore 主密钥"
        );
        assert!(
            !args.iter().any(|arg| arg == "--secrets-backend"),
            "默认 Desktop 应让 Core 使用默认 secret backend，而不是强制 file backend"
        );
        assert!(
            !args.iter().any(|arg| arg == "--secret-key"),
            "默认 Desktop 不应传递 secret-key 参数"
        );
    }

    #[test]
    fn explicit_file_backend_without_secret_key_does_not_invent_default_key() {
        let args = command_args_with_secret_env(Some("file"), None);

        assert_eq!(args, vec!["--secrets-backend", "file"]);
        assert!(
            !args.iter().any(|arg| arg == "subforge-desktop-secret-key"),
            "显式 file backend 未提供密钥时，应让 Core 报错，不得补硬编码主密钥"
        );
    }

    #[test]
    fn explicit_file_backend_with_secret_key_passes_override_key() {
        let args = command_args_with_secret_env(Some("file"), Some("dev-random-key"));

        assert_eq!(
            args,
            vec![
                "--secrets-backend",
                "file",
                "--secret-key",
                "dev-random-key"
            ]
        );
    }

    #[test]
    fn explicit_non_file_backend_ignores_desktop_secret_key_env() {
        let args = command_args_with_secret_env(Some("keyring"), Some("unused-key"));

        assert_eq!(args, vec!["--secrets-backend", "keyring"]);
    }
}
