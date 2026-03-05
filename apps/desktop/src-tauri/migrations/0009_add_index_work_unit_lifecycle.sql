ALTER TABLE index_work_units ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE index_work_units ADD COLUMN last_error TEXT;
ALTER TABLE index_work_units ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';
ALTER TABLE index_work_units ADD COLUMN completed_at TEXT;

UPDATE index_work_units
SET updated_at = created_at
WHERE updated_at = '';

CREATE INDEX IF NOT EXISTS idx_index_work_units_status ON index_work_units(status);
