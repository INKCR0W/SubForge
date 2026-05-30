use super::*;

#[test]
fn create_source_routes_secret_fields_to_secret_store() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-create");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config = BTreeMap::new();
    config.insert(
        "url".to_string(),
        json!("https://example.com/subscription.txt"),
    );
    config.insert("token".to_string(), json!("token-value"));
    config.insert("region".to_string(), json!("sg"));

    let created = source_service
        .create_source("vendor.example.secure-static", "Secure Source", config)
        .expect("创建来源应成功");

    let config_repository = SourceConfigRepository::new(&db);
    let persisted_config = config_repository
        .get_all(&created.source.id)
        .expect("查询来源配置失败");
    assert!(persisted_config.contains_key("url"));
    assert!(persisted_config.contains_key("region"));
    assert!(!persisted_config.contains_key("token"));

    let secret = secret_store
        .get(&format!("source:{}", created.source.id), "token")
        .expect("secret 字段应进入 SecretStore");
    assert_eq!(secret.as_str(), "token-value");
    assert_eq!(
        created.config.get("token"),
        Some(&Value::String("••••••".to_string()))
    );

    let fetched = source_service
        .get_source(&created.source.id)
        .expect("读取来源应成功")
        .expect("来源应存在");
    assert_eq!(
        fetched.config.get("token"),
        Some(&Value::String("••••••".to_string()))
    );

    let listed = source_service.list_sources().expect("列出来源应成功");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].config.get("token"),
        Some(&Value::String("••••••".to_string()))
    );

    cleanup_dir(&temp_root);
}

#[test]
fn same_plugin_sources_keep_secret_values_isolated_for_runtime() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-secret-isolation");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config_a = BTreeMap::new();
    config_a.insert("url".to_string(), json!("https://example.com/a"));
    config_a.insert("token".to_string(), json!("a-secret"));
    let source_a = source_service
        .create_source("vendor.example.secure-static", "Source A", config_a)
        .expect("创建来源 A 应成功");

    let mut config_b = BTreeMap::new();
    config_b.insert("url".to_string(), json!("https://example.com/b"));
    config_b.insert("token".to_string(), json!("b-secret"));
    let source_b = source_service
        .create_source("vendor.example.secure-static", "Source B", config_b)
        .expect("创建来源 B 应成功");

    let runtime_a = source_service
        .get_source_for_runtime(&source_a.source.id)
        .expect("读取来源 A runtime 配置应成功")
        .expect("来源 A 应存在");
    let runtime_b = source_service
        .get_source_for_runtime(&source_b.source.id)
        .expect("读取来源 B runtime 配置应成功")
        .expect("来源 B 应存在");

    assert_eq!(
        runtime_a.config.get("token"),
        Some(&Value::String("a-secret".to_string()))
    );
    assert_eq!(
        runtime_b.config.get("token"),
        Some(&Value::String("b-secret".to_string()))
    );

    cleanup_dir(&temp_root);
}

#[test]
fn deleting_one_source_keeps_other_source_secret_for_same_plugin() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-delete-secret-isolation");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config_a = BTreeMap::new();
    config_a.insert("url".to_string(), json!("https://example.com/a"));
    config_a.insert("token".to_string(), json!("a-secret"));
    let source_a = source_service
        .create_source("vendor.example.secure-static", "Source A", config_a)
        .expect("创建来源 A 应成功");

    let mut config_b = BTreeMap::new();
    config_b.insert("url".to_string(), json!("https://example.com/b"));
    config_b.insert("token".to_string(), json!("b-secret"));
    let source_b = source_service
        .create_source("vendor.example.secure-static", "Source B", config_b)
        .expect("创建来源 B 应成功");

    source_service
        .delete_source(&source_a.source.id)
        .expect("删除来源 A 应成功");

    let runtime_b = source_service
        .get_source_for_runtime(&source_b.source.id)
        .expect("读取来源 B runtime 配置应成功")
        .expect("来源 B 应存在");
    assert_eq!(
        runtime_b.config.get("token"),
        Some(&Value::String("b-secret".to_string()))
    );

    cleanup_dir(&temp_root);
}

#[test]
fn runtime_config_migrates_legacy_plugin_secret_to_source_scope() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-legacy-plugin-secret");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let source = sample_source("legacy-source", "vendor.example.secure-static");
    SourceRepository::new(&db)
        .insert(&source)
        .expect("写入 legacy source 失败");
    let mut stored_config = BTreeMap::new();
    stored_config.insert(
        "url".to_string(),
        "\"https://example.com/legacy\"".to_string(),
    );
    SourceConfigRepository::new(&db)
        .replace_all(&source.id, &stored_config)
        .expect("写入 legacy source config 失败");

    let secret_store = MemorySecretStore::new();
    secret_store
        .set(
            "plugin:vendor.example.secure-static",
            "token",
            "legacy-secret",
        )
        .expect("预置 legacy plugin scope secret 应成功");
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);

    let runtime = source_service
        .get_source_for_runtime(&source.id)
        .expect("读取 legacy runtime 配置应成功")
        .expect("legacy 来源应存在");
    assert_eq!(
        runtime.config.get("token"),
        Some(&Value::String("legacy-secret".to_string()))
    );

    let migrated = secret_store
        .get("source:legacy-source", "token")
        .expect("runtime 读取 legacy secret 后应迁移到 source scope");
    assert_eq!(migrated.as_str(), "legacy-secret");

    cleanup_dir(&temp_root);
}

#[test]
fn source_config_validation_error_returns_e_config_invalid() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-invalid-config");
    let plugins_dir = temp_root.join("plugins");
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(builtins_static_plugin_dir())
        .expect("安装内置插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let error = source_service
        .create_source("subforge.builtin.static", "Broken Source", BTreeMap::new())
        .expect_err("缺少必填字段时应失败");

    assert!(matches!(error, CoreError::ConfigInvalid(_)));
    assert_eq!(error.code(), "E_CONFIG_INVALID");
    cleanup_dir(&temp_root);
}

#[test]
fn config_schema_without_additional_properties_allows_extra_scalar_fields() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-allow-extra-default-open");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config = BTreeMap::new();
    config.insert(
        "url".to_string(),
        json!("https://example.com/subscription.txt"),
    );
    config.insert("token".to_string(), json!("token-value"));
    config.insert("extra_flag".to_string(), json!(true));

    let created = source_service
        .create_source("vendor.example.secure-static", "Open Schema Source", config)
        .expect("省略 additionalProperties 时应允许额外标量字段");

    assert_eq!(created.config.get("extra_flag"), Some(&Value::Bool(true)));
    cleanup_dir(&temp_root);
}

#[test]
fn config_schema_with_additional_properties_true_allows_extra_scalar_fields() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-allow-extra-open");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let installed_plugin_dir = plugins_dir.join("vendor.example.secure-static");
    let schema_path = installed_plugin_dir.join("schema.json");
    let mut schema: Value =
        serde_json::from_str(&fs::read_to_string(&schema_path).expect("读取 schema 失败"))
            .expect("解析 schema 失败");
    schema["additionalProperties"] = Value::Bool(true);
    fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("序列化 schema 失败"),
    )
    .expect("写入 schema 失败");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config = BTreeMap::new();
    config.insert(
        "url".to_string(),
        json!("https://example.com/subscription.txt"),
    );
    config.insert("token".to_string(), json!("token-value"));
    config.insert("extra_mode".to_string(), json!("strict"));

    let created = source_service
        .create_source(
            "vendor.example.secure-static",
            "Open Explicit Source",
            config,
        )
        .expect("additionalProperties=true 时应允许额外标量字段");

    assert_eq!(
        created.config.get("extra_mode"),
        Some(&Value::String("strict".to_string()))
    );
    cleanup_dir(&temp_root);
}

#[test]
fn config_schema_with_additional_properties_false_rejects_extra_fields() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-reject-extra-closed");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let installed_plugin_dir = plugins_dir.join("vendor.example.secure-static");
    let schema_path = installed_plugin_dir.join("schema.json");
    let mut schema: Value =
        serde_json::from_str(&fs::read_to_string(&schema_path).expect("读取 schema 失败"))
            .expect("解析 schema 失败");
    schema["additionalProperties"] = Value::Bool(false);
    fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("序列化 schema 失败"),
    )
    .expect("写入 schema 失败");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config = BTreeMap::new();
    config.insert(
        "url".to_string(),
        json!("https://example.com/subscription.txt"),
    );
    config.insert("token".to_string(), json!("token-value"));
    config.insert("extra_mode".to_string(), json!("strict"));

    let error = source_service
        .create_source(
            "vendor.example.secure-static",
            "Closed Schema Source",
            config,
        )
        .expect_err("additionalProperties=false 时应拒绝额外字段");

    assert!(matches!(error, CoreError::ConfigInvalid(_)));
    assert_eq!(error.code(), "E_CONFIG_INVALID");
    cleanup_dir(&temp_root);
}

#[test]
fn builtin_static_source_defaults_user_agent_to_clash_meta() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-default-user-agent");
    let plugins_dir = temp_root.join("plugins");
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(builtins_static_plugin_dir())
        .expect("安装内置插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config = BTreeMap::new();
    config.insert(
        "url".to_string(),
        json!("https://example.com/subscription.txt"),
    );

    let created = source_service
        .create_source("subforge.builtin.static", "Builtin Source", config)
        .expect("创建内置静态来源应成功");

    assert_eq!(
        created.config.get("user_agent"),
        Some(&Value::String("clash.meta".to_string()))
    );

    let fetched = source_service
        .get_source(&created.source.id)
        .expect("读取来源应成功")
        .expect("来源应存在");
    assert_eq!(
        fetched.config.get("user_agent"),
        Some(&Value::String("clash.meta".to_string()))
    );

    cleanup_dir(&temp_root);
}

#[test]
fn delete_source_cleans_plugin_secret() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-delete");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut config = BTreeMap::new();
    config.insert("url".to_string(), json!("https://example.com/a"));
    config.insert("token".to_string(), json!("token-a"));

    let created = source_service
        .create_source("vendor.example.secure-static", "Secure Source", config)
        .expect("创建来源应成功");
    source_service
        .delete_source(&created.source.id)
        .expect("删除来源应成功");

    let source_repository = SourceRepository::new(&db);
    assert!(
        source_repository
            .get_by_id(&created.source.id)
            .expect("查询来源失败")
            .is_none()
    );

    let error = secret_store
        .get(&format!("source:{}", created.source.id), "token")
        .expect_err("删除来源后应清理对应 secret");
    assert_eq!(error.code(), "E_SECRET_MISSING");
    cleanup_dir(&temp_root);
}

#[test]
fn update_source_config_allows_secret_placeholder_to_keep_existing_secret() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-update-secret-placeholder");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let secret_store = MemorySecretStore::new();
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);
    let mut create_config = BTreeMap::new();
    create_config.insert("url".to_string(), json!("https://example.com/a"));
    create_config.insert("token".to_string(), json!("token-initial"));
    create_config.insert("region".to_string(), json!("hk"));
    let created = source_service
        .create_source("vendor.example.secure-static", "Source A", create_config)
        .expect("创建来源应成功");

    let mut update_config = BTreeMap::new();
    update_config.insert("url".to_string(), json!("https://example.com/b"));
    update_config.insert("token".to_string(), json!("••••••"));
    update_config.insert("region".to_string(), json!("sg"));

    let updated = source_service
        .update_source_config(&created.source.id, update_config)
        .expect("使用占位符更新来源应成功");
    assert_eq!(
        updated.config.get("token"),
        Some(&Value::String("••••••".to_string()))
    );
    assert_eq!(
        updated.config.get("region"),
        Some(&Value::String("sg".to_string()))
    );

    let secret = secret_store
        .get(&format!("source:{}", created.source.id), "token")
        .expect("secret 应保留");
    assert_eq!(secret.as_str(), "token-initial");

    cleanup_dir(&temp_root);
}

#[test]
fn failed_update_with_legacy_placeholder_does_not_create_source_secret() {
    let db = Database::open_in_memory().expect("内存数据库初始化失败");
    let temp_root = create_temp_dir("source-update-legacy-placeholder-rollback");
    let plugins_dir = temp_root.join("plugins");
    let plugin_source_dir = create_secret_static_plugin_dir(&temp_root);
    let install_service = PluginInstallService::new(&db, &plugins_dir);
    install_service
        .install_from_dir(&plugin_source_dir)
        .expect("安装带密钥字段插件应成功");

    let source = sample_source("legacy-update-source", "vendor.example.secure-static");
    SourceRepository::new(&db)
        .insert(&source)
        .expect("写入 legacy source 失败");
    let mut stored_config = BTreeMap::new();
    stored_config.insert("url".to_string(), "\"https://example.com/a\"".to_string());
    SourceConfigRepository::new(&db)
        .replace_all(&source.id, &stored_config)
        .expect("写入 legacy source config 失败");

    let secret_store = MemorySecretStore::new();
    secret_store
        .set(
            "plugin:vendor.example.secure-static",
            "token",
            "legacy-secret",
        )
        .expect("预置 legacy plugin scope secret 应成功");
    let source_service = SourceService::new(&db, &plugins_dir, &secret_store);

    let mut update_config = BTreeMap::new();
    update_config.insert("url".to_string(), json!(""));
    update_config.insert("token".to_string(), json!("••••••"));
    let error = source_service
        .update_source_config(&source.id, update_config)
        .expect_err("非法 url 应导致更新失败");
    assert_eq!(error.code(), "E_CONFIG_INVALID");

    let source_secret = secret_store
        .get("source:legacy-update-source", "token")
        .expect_err("更新失败不应把 legacy secret 写入 source scope");
    assert_eq!(source_secret.code(), "E_SECRET_MISSING");
    let legacy_secret = secret_store
        .get("plugin:vendor.example.secure-static", "token")
        .expect("legacy plugin scope secret 应保留");
    assert_eq!(legacy_secret.as_str(), "legacy-secret");

    cleanup_dir(&temp_root);
}
