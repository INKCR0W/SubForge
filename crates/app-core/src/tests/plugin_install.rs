use super::*;

#[test]
fn install_plugin_copies_files_and_inserts_database_record() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("install-success");
    let plugins_dir = temp_root.join("plugins");
    let service = PluginInstallService::new(&db, &plugins_dir);

    let source = builtins_static_plugin_dir();
    let installed = service
        .install_from_dir(&source)
        .expect("安装内置插件应成功");

    let target_dir = plugins_dir.join("subforge.builtin.static");
    assert!(target_dir.join("plugin.json").is_file());
    assert!(target_dir.join("schema.json").is_file());
    assert_eq!(installed.plugin_id, "subforge.builtin.static");
    assert_eq!(installed.status, "installed");

    let repository = PluginRepository::new(&db);
    let loaded = repository
        .get_by_plugin_id("subforge.builtin.static")
        .expect("查询已安装插件失败")
        .expect("数据库中应存在插件记录");
    assert_eq!(loaded.plugin_id, "subforge.builtin.static");

    cleanup_dir(&temp_root);
}

#[test]
fn install_same_plugin_twice_returns_error() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("install-duplicate");
    let plugins_dir = temp_root.join("plugins");
    let service = PluginInstallService::new(&db, &plugins_dir);
    let source = builtins_static_plugin_dir();

    service.install_from_dir(&source).expect("首次安装应成功");
    let duplicate_error = service
        .install_from_dir(&source)
        .expect_err("重复安装应失败");

    assert!(matches!(
        duplicate_error,
        CoreError::PluginAlreadyInstalled(_)
    ));
    cleanup_dir(&temp_root);
}

#[test]
fn install_higher_version_plugin_treats_as_upgrade() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("install-upgrade");
    let plugins_dir = temp_root.join("plugins");
    let upgraded_source = create_upgraded_plugin_dir(&temp_root);
    let service = PluginInstallService::new(&db, &plugins_dir);
    let source = builtins_static_plugin_dir();

    let installed_v1 = service.install_from_dir(&source).expect("首次安装应成功");
    assert_eq!(installed_v1.version, "1.0.0");

    let installed_v2 = service
        .install_from_dir(&upgraded_source)
        .expect("升级安装应成功");
    assert_eq!(installed_v2.version, "1.0.1");

    let repository = PluginRepository::new(&db);
    let loaded = repository
        .get_by_plugin_id("subforge.builtin.static")
        .expect("查询升级后插件失败")
        .expect("升级后插件记录应存在");
    assert_eq!(loaded.version, "1.0.1");
    assert!(
        fs::read_to_string(
            plugins_dir
                .join("subforge.builtin.static")
                .join("plugin.json")
        )
        .expect("读取升级后 plugin.json 失败")
        .contains(r#""version": "1.0.1""#),
        "升级成功后目录内容应切换到新版本"
    );
    assert_no_plugin_staging_dirs(&plugins_dir);

    cleanup_dir(&temp_root);
}

#[test]
fn failed_upgrade_copy_preserves_existing_plugin_dir_and_database_record() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("install-upgrade-copy-failure");
    let plugins_dir = temp_root.join("plugins");
    let failing_source = create_upgraded_plugin_dir(&temp_root);
    let service = PluginInstallService::new(&db, &plugins_dir);
    let source = builtins_static_plugin_dir();

    service.install_from_dir(&source).expect("首次安装应成功");
    let target_dir = plugins_dir.join("subforge.builtin.static");
    fs::write(target_dir.join("old-only.txt"), "old-version").expect("写入旧插件哨兵文件失败");

    let locked_file = failing_source.join("copy-blocked.bin");
    fs::write(&locked_file, "copy should fail").expect("写入复制失败样本失败");
    let copy_failure_guard = block_file_copy(&locked_file);

    let error = service
        .install_from_dir(&failing_source)
        .expect_err("升级复制失败时安装应失败");
    assert!(matches!(error, CoreError::Io(_)));

    let repository = PluginRepository::new(&db);
    let loaded = repository
        .get_by_plugin_id("subforge.builtin.static")
        .expect("查询旧插件记录失败")
        .expect("复制失败后旧插件数据库记录应保留");
    assert_eq!(loaded.version, "1.0.0");
    assert_eq!(
        fs::read_to_string(target_dir.join("old-only.txt")).expect("旧插件目录应保留"),
        "old-version"
    );
    assert!(
        target_dir.join("plugin.json").is_file(),
        "复制失败后旧插件目录不能被删除"
    );
    assert!(
        fs::read_to_string(target_dir.join("plugin.json"))
            .expect("读取旧 plugin.json 失败")
            .contains(r#""version": "1.0.0""#),
        "复制失败后目录内容应仍是旧版本"
    );
    assert_no_plugin_staging_dirs(&plugins_dir);

    drop(copy_failure_guard);
    cleanup_dir(&temp_root);
}

#[test]
fn install_invalid_plugin_keeps_target_directory_clean() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("install-invalid");
    let plugins_dir = temp_root.join("plugins");
    let bad_plugin_dir = create_bad_plugin_dir(&temp_root);
    let service = PluginInstallService::new(&db, &plugins_dir);

    let error = service
        .install_from_dir(&bad_plugin_dir)
        .expect_err("非法插件安装应失败");
    assert!(matches!(error, CoreError::PluginRuntime(_)));

    let entries = fs::read_dir(&plugins_dir)
        .ok()
        .into_iter()
        .flat_map(|iter| iter.filter_map(Result::ok))
        .collect::<Vec<_>>();
    assert!(entries.is_empty(), "非法插件不应留下安装目录");

    cleanup_dir(&temp_root);
}

fn assert_no_plugin_staging_dirs(plugins_dir: &Path) {
    let staging_entries = fs::read_dir(plugins_dir)
        .expect("读取插件目录失败")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.contains(".tmp.") || name.contains(".backup."))
        .collect::<Vec<_>>();
    assert!(
        staging_entries.is_empty(),
        "插件安装不应留下临时/备份目录：{staging_entries:?}"
    );
}

#[cfg(windows)]
fn block_file_copy(path: &Path) -> fs::File {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt as _;

    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .expect("应能以独占共享模式打开文件")
}

#[cfg(unix)]
fn block_file_copy(path: &Path) -> std::os::unix::net::UnixListener {
    let _ = fs::remove_file(path);
    std::os::unix::net::UnixListener::bind(path).expect("应能创建 Unix socket 作为复制失败样本")
}
