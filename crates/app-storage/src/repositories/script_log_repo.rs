use rusqlite::{params, params_from_iter};

use crate::mappers::map_script_log_row;
use crate::{Database, ScriptLog, StorageResult};

#[derive(Debug, Clone, Copy)]
pub struct ScriptLogRepository<'a> {
    db: &'a Database,
}

impl<'a> ScriptLogRepository<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn insert(&self, log: &ScriptLog) -> StorageResult<()> {
        self.db.with_connection(|connection| {
            connection.execute(
                "INSERT INTO script_logs
                 (id, refresh_job_id, source_instance_id, plugin_id, level, message, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    log.id,
                    log.refresh_job_id,
                    log.source_instance_id,
                    log.plugin_id,
                    log.level,
                    log.message,
                    log.created_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_by_refresh_job_ids(
        &self,
        refresh_job_ids: &[String],
        per_job_limit: usize,
    ) -> StorageResult<Vec<ScriptLog>> {
        if refresh_job_ids.is_empty() || per_job_limit == 0 {
            return Ok(Vec::new());
        }

        self.db.with_connection(|connection| {
            let placeholders = (0..refresh_job_ids.len())
                .map(|index| format!("?{}", index + 1))
                .collect::<Vec<_>>()
                .join(", ");
            let query = format!(
                "SELECT id, refresh_job_id, source_instance_id, plugin_id, level, message, created_at
                 FROM script_logs
                 WHERE refresh_job_id IN ({placeholders})
                 ORDER BY created_at ASC, id ASC"
            );
            let mut statement = connection.prepare(&query)?;
            let rows = statement
                .query_map(params_from_iter(refresh_job_ids.iter()), map_script_log_row)?
                .collect::<Result<Vec<_>, _>>()?;

            let mut counts = std::collections::HashMap::<String, usize>::new();
            let mut filtered = Vec::new();
            for row in rows {
                let counter = counts.entry(row.refresh_job_id.clone()).or_default();
                if *counter < per_job_limit {
                    filtered.push(row);
                    *counter += 1;
                }
            }
            Ok(filtered)
        })
    }
}
