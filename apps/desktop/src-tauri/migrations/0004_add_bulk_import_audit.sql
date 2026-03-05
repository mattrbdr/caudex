ALTER TABLE import_jobs ADD COLUMN import_mode TEXT NOT NULL DEFAULT 'single';
ALTER TABLE import_jobs ADD COLUMN root_path TEXT;
ALTER TABLE import_jobs ADD COLUMN duplicate_mode TEXT NOT NULL DEFAULT 'skip_duplicate';
ALTER TABLE import_jobs ADD COLUMN dry_run INTEGER NOT NULL DEFAULT 0;
ALTER TABLE import_jobs ADD COLUMN scanned_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE import_job_items ADD COLUMN precheck_key TEXT;
ALTER TABLE import_job_items ADD COLUMN content_hash TEXT;
ALTER TABLE import_job_items ADD COLUMN dedupe_decision TEXT;
