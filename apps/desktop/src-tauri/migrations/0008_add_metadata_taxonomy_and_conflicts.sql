CREATE TABLE IF NOT EXISTS tags (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE
);

CREATE TABLE IF NOT EXISTS collections (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE
);

CREATE TABLE IF NOT EXISTS item_tags (
  library_item_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL,
  PRIMARY KEY (library_item_id, tag_id),
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE CASCADE,
  FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS item_collections (
  library_item_id INTEGER NOT NULL,
  collection_id INTEGER NOT NULL,
  PRIMARY KEY (library_item_id, collection_id),
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE CASCADE,
  FOREIGN KEY (collection_id) REFERENCES collections(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS metadata_conflicts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  library_item_id INTEGER NOT NULL,
  field_name TEXT NOT NULL,
  current_value TEXT NOT NULL,
  candidate_value TEXT NOT NULL,
  candidate_source TEXT NOT NULL,
  status TEXT NOT NULL,
  rationale TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  FOREIGN KEY (library_item_id) REFERENCES library_items(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_item_tags_item
  ON item_tags(library_item_id);

CREATE INDEX IF NOT EXISTS idx_item_collections_item
  ON item_collections(library_item_id);

CREATE INDEX IF NOT EXISTS idx_metadata_conflicts_item
  ON metadata_conflicts(library_item_id);

CREATE INDEX IF NOT EXISTS idx_metadata_conflicts_status
  ON metadata_conflicts(status);
