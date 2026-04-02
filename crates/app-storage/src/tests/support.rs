use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use app_common::{
    Plugin, Profile, ProfileSource, ProxyNode, ProxyProtocol, ProxyTransport, SourceInstance,
    TlsConfig,
};

use crate::{Database, StorageResult};

pub(super) fn list_tables(db: &Database) -> StorageResult<Vec<String>> {
    db.with_connection(|connection| {
        let mut statement = connection.prepare(
            "SELECT name \
                 FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
        )?;

        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(names)
    })
}

pub(super) fn list_columns(db: &Database, table: &str) -> StorageResult<Vec<String>> {
    db.with_connection(|connection| {
        let mut statement =
            connection.prepare("SELECT name FROM pragma_table_info(?) ORDER BY cid")?;

        let names = statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(names)
    })
}

pub(super) fn list_profile_sources(
    db: &Database,
    profile_id: &str,
) -> StorageResult<Vec<ProfileSource>> {
    db.with_connection(|connection| {
        let mut statement = connection.prepare(
            "SELECT profile_id, source_instance_id, priority
                 FROM profile_sources
                 WHERE profile_id = ?1
                 ORDER BY source_instance_id",
        )?;

        let records = statement
            .query_map([profile_id], |row| {
                Ok(ProfileSource {
                    profile_id: row.get("profile_id")?,
                    source_instance_id: row.get("source_instance_id")?,
                    priority: row.get("priority")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    })
}

pub(super) fn sample_plugin(id: &str, plugin_id: &str) -> Plugin {
    Plugin {
        id: id.to_string(),
        plugin_id: plugin_id.to_string(),
        name: "Example Plugin".to_string(),
        version: "1.0.0".to_string(),
        spec_version: "1.0".to_string(),
        plugin_type: "static".to_string(),
        status: "enabled".to_string(),
        installed_at: "2026-04-02T01:00:00Z".to_string(),
        updated_at: "2026-04-02T01:00:00Z".to_string(),
    }
}

pub(super) fn sample_source(id: &str, plugin_id: &str) -> SourceInstance {
    SourceInstance {
        id: id.to_string(),
        plugin_id: plugin_id.to_string(),
        name: format!("Source {id}"),
        status: "healthy".to_string(),
        state_json: None,
        created_at: "2026-04-02T01:10:00Z".to_string(),
        updated_at: "2026-04-02T01:10:00Z".to_string(),
    }
}

pub(super) fn sample_proxy_node(id: &str, server: &str, port: u16) -> ProxyNode {
    ProxyNode {
        id: id.to_string(),
        name: format!("{server}:{port}"),
        protocol: ProxyProtocol::Ss,
        server: server.to_string(),
        port,
        transport: ProxyTransport::Tcp,
        tls: TlsConfig {
            enabled: true,
            server_name: Some(server.to_string()),
        },
        extra: BTreeMap::new(),
        source_id: "source-cache-1".to_string(),
        tags: Vec::new(),
        region: None,
        updated_at: "2026-04-02T04:00:00Z".to_string(),
    }
}

pub(super) fn sample_profile(id: &str) -> Profile {
    Profile {
        id: id.to_string(),
        name: "Default Profile".to_string(),
        description: Some("默认配置".to_string()),
        created_at: "2026-04-02T01:20:00Z".to_string(),
        updated_at: "2026-04-02T01:20:00Z".to_string(),
    }
}

pub(super) fn unique_test_db_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间异常")
        .as_nanos();
    std::env::temp_dir().join(format!("subforge-app-storage-{nanos}.db"))
}

pub(super) fn cleanup_db_files(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
}
