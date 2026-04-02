use super::*;

#[tokio::test]
async fn engine_refresh_source_uses_profile_headers_from_plugin_manifest() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("engine-profile-routing");
    let plugins_dir = temp_root.join("plugins");
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    let standard_plugin_dir = create_static_plugin_with_network_profile(
        &temp_root,
        "standard-plugin",
        "vendor.example.profile-standard",
        "standard",
    );
    let chrome_plugin_dir = create_static_plugin_with_network_profile(
        &temp_root,
        "chrome-plugin",
        "vendor.example.profile-browser-chrome",
        "browser_chrome",
    );
    install_service
        .install_from_dir(&standard_plugin_dir)
        .expect("安装 standard 插件应成功");
    install_service
        .install_from_dir(&chrome_plugin_dir)
        .expect("安装 browser_chrome 插件应成功");

    let (url, total_requests, chrome_requests, server_task) = start_profile_gate_server(
        "/sub",
        BASE64_SUBSCRIPTION_FIXTURE.trim().to_string(),
        "text/plain; charset=utf-8",
    )
    .await;

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut standard_config = BTreeMap::new();
    standard_config.insert("url".to_string(), json!(format!("{url}/sub")));
    let standard_source = source_service
        .create_source(
            "vendor.example.profile-standard",
            "Standard Profile Source",
            standard_config,
        )
        .expect("创建 standard 来源应成功");

    let mut chrome_config = BTreeMap::new();
    chrome_config.insert("url".to_string(), json!(format!("{url}/sub")));
    let chrome_source = source_service
        .create_source(
            "vendor.example.profile-browser-chrome",
            "Browser Chrome Source",
            chrome_config,
        )
        .expect("创建 browser_chrome 来源应成功");

    let engine = Engine::new(&db, &plugins_dir, &secret_store);
    let standard_error = engine
        .refresh_source(&standard_source.source.id, "manual")
        .await
        .expect_err("standard 档位不应通过 Chrome Header 校验");
    assert!(matches!(standard_error, CoreError::SubscriptionFetch(_)));

    let chrome_result = engine
        .refresh_source(&chrome_source.source.id, "manual")
        .await
        .expect("browser_chrome 档位应通过 Header 校验");
    assert_eq!(chrome_result.node_count, 3);
    assert_eq!(total_requests.load(Ordering::SeqCst), 2);
    assert_eq!(chrome_requests.load(Ordering::SeqCst), 1);

    server_task.abort();
    cleanup_dir(&temp_root);
}

#[tokio::test]
async fn engine_refresh_source_records_refresh_job_success() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("engine-refresh");
    let plugins_dir = temp_root.join("plugins");
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(builtins_static_plugin_dir())
        .expect("安装内置插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let (url, server_task) = start_fixture_server(
        "/sub",
        BASE64_SUBSCRIPTION_FIXTURE.trim().to_string(),
        "text/plain; charset=utf-8",
    )
    .await;
    let mut config = BTreeMap::new();
    config.insert("url".to_string(), json!(format!("{url}/sub")));
    let source = source_service
        .create_source("subforge.builtin.static", "Engine Source", config)
        .expect("创建来源应成功");

    let engine = Engine::new(&db, &plugins_dir, &secret_store);
    let refresh_result = engine
        .refresh_source(&source.source.id, "manual")
        .await
        .expect("刷新应成功");
    assert_eq!(refresh_result.source_id, source.source.id);
    assert_eq!(refresh_result.node_count, 3);

    let refresh_repository = RefreshJobRepository::new(&db);
    let jobs = refresh_repository
        .list_by_source(&source.source.id)
        .expect("读取 refresh_jobs 失败");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, refresh_result.refresh_job_id);
    assert_eq!(jobs[0].status, "success");
    assert_eq!(jobs[0].node_count, Some(3));
    assert!(jobs[0].error_code.is_none());

    server_task.abort();
    cleanup_dir(&temp_root);
}

#[test]
fn engine_ensure_profile_export_token_is_idempotent() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("engine-token");
    let plugins_dir = temp_root.join("plugins");
    fs::create_dir_all(&plugins_dir).expect("创建插件目录失败");
    let profile_repository = app_storage::ProfileRepository::new(&db);
    let profile = app_common::Profile {
        id: "profile-engine-token".to_string(),
        name: "Engine Token".to_string(),
        description: None,
        created_at: "2026-04-02T07:00:00Z".to_string(),
        updated_at: "2026-04-02T07:00:00Z".to_string(),
    };
    profile_repository
        .insert(&profile)
        .expect("写入 profile 失败");

    let secret_store = MemorySecretStore::new();
    let engine = Engine::new(&db, &plugins_dir, &secret_store);
    let token_a = engine
        .ensure_profile_export_token(&profile.id)
        .expect("首次生成 token 应成功");
    let token_b = engine
        .ensure_profile_export_token(&profile.id)
        .expect("重复生成应返回已有 token");
    assert_eq!(token_a, token_b);
    assert_eq!(token_a.len(), 43);

    let token_repository = ExportTokenRepository::new(&db);
    let stored = token_repository
        .get_active_token(&profile.id)
        .expect("读取 active token 失败")
        .expect("应存在 active token");
    assert_eq!(stored.token, token_a);

    cleanup_dir(&temp_root);
}
