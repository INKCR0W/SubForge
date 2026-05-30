use std::io::Read as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[tokio::test]
async fn gui_mode_does_not_emit_bootstrap_when_bind_fails() {
    let temp_root = create_temp_dir("gui-bootstrap-process");
    let data_dir = temp_root.join("data");
    std::fs::create_dir_all(&data_dir).expect("创建 data 目录失败");

    let occupied_listener = TcpListener::bind("127.0.0.1:0").expect("预占本地端口失败");
    let occupied_port = occupied_listener
        .local_addr()
        .expect("读取预占端口失败")
        .port();

    let core_bin = core_binary_path();
    let child = Command::new(core_bin)
        .arg("run")
        .arg("--gui-mode")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(occupied_port.to_string())
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--secrets-backend")
        .arg("memory")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动 subforge-core 失败");
    let mut child = ChildGuard::new(child);

    let exit_status = wait_for_exit(child.inner_mut(), Duration::from_secs(10))
        .await
        .expect("等待 Core 绑定失败退出超时");
    assert!(!exit_status.success(), "端口被占用时 Core 应启动失败");

    let mut stdout_output = String::new();
    if let Some(mut stdout) = child.take_stdout() {
        stdout
            .read_to_string(&mut stdout_output)
            .expect("读取 stdout 失败");
    }

    assert!(
        !stdout_output.contains("admin_token"),
        "绑定监听成功前不得输出包含 admin_token 的 GUI bootstrap，stdout={stdout_output}"
    );

    child.disarm();
    drop(occupied_listener);
    let _ = std::fs::remove_dir_all(&temp_root);
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

    fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.as_mut().and_then(|child| child.stdout.take())
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
