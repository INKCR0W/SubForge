use std::path::Path;

use serde_json::Value;

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("读取 JSON 文件失败 {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("解析 JSON 文件失败 {}: {err}", path.display()))
}

fn csp_directive(csp: &str, name: &str) -> Option<String> {
    csp.split(';')
        .map(str::trim)
        .find(|segment| segment.starts_with(name))
        .map(ToOwned::to_owned)
}

#[test]
fn tauri_csp_blocks_eval_and_remote_script() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = read_json(&root.join("tauri.conf.json"));
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("tauri.conf.json 缺少 app.security.csp");

    let script_src = csp_directive(csp, "script-src").expect("CSP 缺少 script-src 指令");
    assert!(
        !script_src.contains("unsafe-eval"),
        "script-src 不得包含 unsafe-eval"
    );
    assert!(
        !script_src.contains("unsafe-inline"),
        "script-src 不得包含 unsafe-inline"
    );
    assert!(
        !script_src.contains("http:") && !script_src.contains("https:"),
        "script-src 不得允许外部 http/https 脚本"
    );

    let connect_src = csp_directive(csp, "connect-src").expect("CSP 缺少 connect-src 指令");
    assert!(
        connect_src.contains("ipc:"),
        "connect-src 必须允许 ipc: 以支持 IPC 通道"
    );
    assert!(
        connect_src.contains("http://ipc.localhost"),
        "connect-src 必须仅允许 http://ipc.localhost"
    );
}

#[test]
fn tauri_main_window_disables_devtools_in_config() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = read_json(&root.join("tauri.conf.json"));
    let windows = config["app"]["windows"]
        .as_array()
        .expect("tauri.conf.json 缺少 app.windows");

    let main_window = windows
        .iter()
        .find(|window| window["label"].as_str() == Some("main"))
        .expect("未找到 label=main 的窗口配置");

    assert_eq!(
        main_window["devtools"].as_bool(),
        Some(false),
        "main 窗口必须显式禁用 DevTools"
    );
}

#[test]
fn capability_uses_minimal_permissions_and_blocks_devtools_toggle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let capability = read_json(&root.join("capabilities").join("default.json"));
    let permissions = capability["permissions"]
        .as_array()
        .expect("capabilities/default.json 缺少 permissions");

    let values = permissions
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    assert!(
        !values.iter().any(|entry| entry.starts_with("fs:")),
        "能力配置不应包含 fs 权限"
    );
    assert!(
        !values.iter().any(|entry| entry.starts_with("http:")),
        "能力配置不应包含 http 权限"
    );
    assert!(
        !values.iter().any(|entry| *entry == "core:default"),
        "能力配置不应使用 core:default 粗粒度授权"
    );
    assert!(
        values.contains(&"core:webview:deny-internal-toggle-devtools"),
        "能力配置必须显式拒绝 WebView 内部 DevTools 开关"
    );
}
