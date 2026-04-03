CREATE TABLE IF NOT EXISTS script_logs (
    id TEXT PRIMARY KEY,
    refresh_job_id TEXT NOT NULL,
    source_instance_id TEXT NOT NULL,
    plugin_id TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (refresh_job_id) REFERENCES refresh_jobs (id) ON DELETE CASCADE,
    FOREIGN KEY (source_instance_id) REFERENCES source_instances (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_script_logs_refresh_job_id
    ON script_logs (refresh_job_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_script_logs_source_instance_id
    ON script_logs (source_instance_id, created_at, id);
