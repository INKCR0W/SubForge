use std::collections::BTreeSet;
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

fn csp_sources(csp: &str, name: &str) -> Option<Vec<String>> {
    csp_directive(csp, name).map(|directive| {
        directive
            .split_whitespace()
            .skip(1)
            .map(ToOwned::to_owned)
            .collect()
    })
}

#[test]
fn tauri_csp_blocks_eval_and_remote_script() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = read_json(&root.join("tauri.conf.json"));
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("tauri.conf.json 缺少 app.security.csp");

    let default_src = csp_sources(csp, "default-src").expect("CSP 缺少 default-src 指令");
    assert_eq!(
        default_src,
        vec!["'self'".to_string()],
        "default-src 仅允许 'self'"
    );

    let script_src = csp_sources(csp, "script-src").expect("CSP 缺少 script-src 指令");
    assert!(
        script_src == vec!["'self'".to_string()],
        "script-src 必须严格为 'self'，以阻断 eval/inline/外链脚本"
    );

    let connect_src = csp_sources(csp, "connect-src").expect("CSP 缺少 connect-src 指令");
    let connect_src_set = connect_src.into_iter().collect::<BTreeSet<_>>();
    let expected_set = ["ipc:", "http://ipc.localhost"]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        connect_src_set, expected_set,
        "connect-src 必须仅允许 ipc: 与 http://ipc.localhost"
    );
}

#[test]
fn tauri_all_windows_disable_devtools_in_config() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = read_json(&root.join("tauri.conf.json"));
    let windows = config["app"]["windows"]
        .as_array()
        .expect("tauri.conf.json 缺少 app.windows");

    assert!(!windows.is_empty(), "至少需要一个窗口配置");
    for window in windows {
        let label = window["label"].as_str().unwrap_or("<unnamed>");
        assert_eq!(
            window["devtools"].as_bool(),
            Some(false),
            "窗口 {label} 必须显式禁用 DevTools"
        );
    }
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
        !values.contains(&"core:default"),
        "能力配置不应使用 core:default 粗粒度授权"
    );
    assert!(
        values.contains(&"core:webview:deny-internal-toggle-devtools"),
        "能力配置必须显式拒绝 WebView 内部 DevTools 开关"
    );
    assert!(
        !values.contains(&"core:webview:allow-internal-toggle-devtools"),
        "能力配置不得包含允许 internal-toggle-devtools 的权限"
    );
    let toggle_permissions = values
        .iter()
        .filter(|entry| entry.contains("toggle-devtools"))
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(
        toggle_permissions,
        vec!["core:webview:deny-internal-toggle-devtools"],
        "toggle-devtools 相关权限只允许 deny 项"
    );
}
