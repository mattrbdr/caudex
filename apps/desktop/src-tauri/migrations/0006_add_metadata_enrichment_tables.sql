CREATE TABLE IF NOT EXISTS metadata_enrichment_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  library_item_id INTEGER NOT NULL,
  status TEXT NOT NULL,
  diagnostic TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS metadata_enrichment_proposals (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  library_item_id INTEGER NOT NULL,
  provider TEXT NOT NULL,
  confidence REAL NOT NULL,
  title TEXT,
  authors TEXT NOT NULL DEFAULT '[]',
  language TEXT,
  published_at TEXT,
  raw_payload TEXT,
  diagnostic TEXT,
  created_at TEXT NOT NULL,
  applied_at TEXT,
  FOREIGN KEY (run_id) REFERENCES metadata_enrichment_runs(id) ON DELETE CASCADE,
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_metadata_enrichment_runs_item
  ON metadata_enrichment_runs(library_item_id);

CREATE INDEX IF NOT EXISTS idx_metadata_enrichment_proposals_run
  ON metadata_enrichment_proposals(run_id);

CREATE INDEX IF NOT EXISTS idx_metadata_enrichment_proposals_item
  ON metadata_enrichment_proposals(library_item_id);
