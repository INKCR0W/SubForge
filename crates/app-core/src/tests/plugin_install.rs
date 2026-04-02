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
