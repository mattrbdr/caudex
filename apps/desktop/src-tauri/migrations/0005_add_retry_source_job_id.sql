ALTER TABLE import_jobs ADD COLUMN retry_source_job_id INTEGER;

CREATE INDEX IF NOT EXISTS idx_import_jobs_retry_source ON import_jobs(retry_source_job_id);
