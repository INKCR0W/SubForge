use std::collections::BTreeMap;

use crate::{Database, StorageResult};

use super::support::{cleanup_db_files, list_columns, list_tables, unique_test_db_path};

#[test]
fn open_in_memory_runs_migrations() -> StorageResult<()> {
    let db = Database::open_in_memory()?;
    let tables = list_tables(&db)?;

    let expected = vec![
        "app_settings",
        "export_tokens",
        "script_logs",
        "plugins",
        "profile_sources",
        "profiles",
        "refresh_jobs",
        "source_instance_config",
        "source_instances",
        "node_cache",
    ];

    for table in expected {
        assert!(tables.iter().any(|name| name == table), "缺少表：{table}");
    }

    Ok(())
}

#[test]
fn migration_creates_expected_columns() -> StorageResult<()> {
    let db = Database::open_in_memory()?;

    let expected_columns = BTreeMap::from([
        (
            "plugins",
            vec![
                "id",
                "plugin_id",
                "name",
                "version",
                "spec_version",
                "type",
                "status",
                "installed_at",
                "updated_at",
            ],
        ),
        (
            "source_instances",
            vec![
                "id",
                "plugin_id",
                "name",
                "status",
                "state_json",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "source_instance_config",
            vec!["id", "source_instance_id", "key", "value"],
        ),
        (
            "profiles",
            vec!["id", "name", "description", "created_at", "updated_at"],
        ),
        (
            "profile_sources",
            vec!["profile_id", "source_instance_id", "priority"],
        ),
        (
            "refresh_jobs",
            vec![
                "id",
                "source_instance_id",
                "trigger_type",
                "status",
                "started_at",
                "finished_at",
                "node_count",
                "error_code",
                "error_message",
            ],
        ),
        (
            "export_tokens",
            vec![
                "id",
                "profile_id",
                "token",
                "token_type",
                "created_at",
                "expires_at",
            ],
        ),
        (
            "script_logs",
            vec![
                "id",
                "refresh_job_id",
                "source_instance_id",
                "plugin_id",
                "level",
                "message",
                "created_at",
            ],
        ),
        ("app_settings", vec!["key", "value", "updated_at"]),
        (
            "node_cache",
            vec![
                "id",
                "source_instance_id",
                "data_json",
                "fetched_at",
                "expires_at",
            ],
        ),
    ]);

    for (table, expected) in expected_columns {
        let columns = list_columns(&db, table)?;
        assert_eq!(columns, expected, "表字段不匹配：{table}");
    }

    Ok(())
}

#[test]
fn opening_database_twice_is_idempotent() -> StorageResult<()> {
    let db_path = unique_test_db_path();

    let first = Database::open(&db_path)?;
    let first_tables = list_tables(&first)?;
    drop(first);

    let second = Database::open(&db_path)?;
    let second_tables = list_tables(&second)?;

    assert_eq!(first_tables, second_tables);

    cleanup_db_files(&db_path);
    Ok(())
}
