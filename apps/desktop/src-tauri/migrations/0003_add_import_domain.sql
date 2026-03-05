CREATE TABLE IF NOT EXISTS library_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  library_id INTEGER NOT NULL,
  source_path TEXT NOT NULL,
  format TEXT NOT NULL,
  title TEXT NOT NULL,
  authors TEXT NOT NULL DEFAULT '[]',
  language TEXT,
  published_at TEXT,
  imported_at TEXT NOT NULL,
  FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
  UNIQUE (library_id, source_path)
);

CREATE TABLE IF NOT EXISTS import_jobs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  library_id INTEGER NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS index_work_units (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  library_item_id INTEGER NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS import_job_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  job_id INTEGER NOT NULL,
  source_path TEXT NOT NULL,
  detected_format TEXT,
  status TEXT NOT NULL,
  error_code TEXT,
  error_message TEXT,
  library_item_id INTEGER,
  index_work_unit_id INTEGER,
  queued_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (job_id) REFERENCES import_jobs(id) ON DELETE CASCADE,
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE SET NULL,
  FOREIGN KEY (index_work_unit_id) REFERENCES index_work_units(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_library_items_library ON library_items(library_id);
CREATE INDEX IF NOT EXISTS idx_import_jobs_library ON import_jobs(library_id);
CREATE INDEX IF NOT EXISTS idx_import_job_items_job ON import_job_items(job_id);
CREATE INDEX IF NOT EXISTS idx_import_job_items_status ON import_job_items(status);
CREATE INDEX IF NOT EXISTS idx_index_work_units_item ON index_work_units(library_item_id);
