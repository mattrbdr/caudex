DELETE FROM libraries
WHERE id NOT IN (
  SELECT id
  FROM libraries
  ORDER BY id ASC
  LIMIT 1
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_libraries_singleton_guard ON libraries ((1));
