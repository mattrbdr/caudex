CREATE TABLE IF NOT EXISTS metadata_batch_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_key TEXT NOT NULL,
  mode TEXT NOT NULL,
  status TEXT NOT NULL,
  target_scope TEXT NOT NULL,
  patch_payload TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS metadata_batch_results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  library_item_id INTEGER NOT NULL,
  status TEXT NOT NULL,
  reason TEXT,
  retry_eligible INTEGER NOT NULL DEFAULT 0,
  before_snapshot TEXT,
  after_snapshot TEXT,
  FOREIGN KEY (run_id) REFERENCES metadata_batch_runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_metadata_batch_runs_key
  ON metadata_batch_runs(run_key);

CREATE INDEX IF NOT EXISTS idx_metadata_batch_results_run
  ON metadata_batch_results(run_id);

CREATE INDEX IF NOT EXISTS idx_metadata_batch_results_item
  ON metadata_batch_results(library_item_id);
