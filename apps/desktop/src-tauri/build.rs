use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    prepare_external_core_sidecar();
    tauri_build::build();
}

fn prepare_external_core_sidecar() {
    let manifest_dir = match env::var("CARGO_MANIFEST_DIR") {
        Ok(value) => PathBuf::from(value),
        Err(_) => return,
    };
    let target = match env::var("TARGET") {
        Ok(value) => value,
        Err(_) => return,
    };
    let profile = env::var("PROFILE").unwrap_or_default();

    let mut sidecar_name = format!("subforge-core-{target}");
    if target.contains("windows") {
        sidecar_name.push_str(".exe");
    }
    let sidecar_path = manifest_dir.join("binaries").join(sidecar_name);

    if sidecar_path.exists() {
        return;
    }

    if profile == "release" {
        panic!("缺少 Core sidecar，请先准备 {}", sidecar_path.display());
    }

    if let Some(parent) = sidecar_path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        panic!("创建 sidecar 目录失败 {}: {}", parent.display(), error);
    }

    if let Err(error) = fs::write(&sidecar_path, b"subforge-core sidecar placeholder") {
        panic!(
            "创建 sidecar 占位文件失败 {}: {}",
            sidecar_path.display(),
            error
        );
    }

    println!(
        "cargo:warning=检测到调试构建缺少 sidecar，已创建占位文件 {}",
        sidecar_path.display()
    );
}
