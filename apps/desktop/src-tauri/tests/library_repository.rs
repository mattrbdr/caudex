use appsdesktop_lib::library::{fetch_library, insert_library, run_migrations};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::PathBuf;
use tempfile::tempdir;

async fn setup_pool(path: PathBuf) -> sqlx::SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("pool should initialize")
}

#[tokio::test]
async fn migrations_create_libraries_table() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;

    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    let table_exists: Option<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' AND name='libraries'")
            .fetch_optional(&pool)
            .await
            .expect("table lookup should succeed");

    assert!(table_exists.is_some(), "libraries table should exist");
}

#[tokio::test]
async fn migrations_create_import_tables() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;

    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    for table in [
        "library_items",
        "import_jobs",
        "import_job_items",
        "index_work_units",
    ] {
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' AND name = ?")
                .bind(table)
                .fetch_optional(&pool)
                .await
                .expect("table lookup should succeed");

        assert!(exists.is_some(), "{table} table should exist");
    }

    let import_jobs_columns: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM pragma_table_info('import_jobs')")
            .fetch_all(&pool)
            .await
            .expect("pragma should succeed");
    let import_jobs_column_names: Vec<String> = import_jobs_columns
        .into_iter()
        .map(|(name,)| name)
        .collect();
    assert!(
        import_jobs_column_names
            .iter()
            .any(|name| name == "import_mode")
            && import_jobs_column_names
                .iter()
                .any(|name| name == "root_path")
            && import_jobs_column_names
                .iter()
                .any(|name| name == "duplicate_mode")
            && import_jobs_column_names
                .iter()
                .any(|name| name == "dry_run")
            && import_jobs_column_names
                .iter()
                .any(|name| name == "scanned_count"),
        "import_jobs audit columns should exist"
    );
}

#[tokio::test]
async fn repository_can_create_and_read_library() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;

    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    let empty = fetch_library(&pool)
        .await
        .expect("fetch should succeed before insert");
    assert!(empty.is_none(), "library should not exist before insert");

    let created_at = "2026-03-05T10:30:00Z";
    let created = insert_library(&pool, "Main Library", "/tmp/caudex-library", created_at)
        .await
        .expect("insert should succeed");

    let loaded = fetch_library(&pool)
        .await
        .expect("fetch should succeed after insert")
        .expect("library should exist after insert");

    assert_eq!(loaded.id, created.id);
    assert_eq!(loaded.name, "Main Library");
    assert_eq!(loaded.path, "/tmp/caudex-library");
    assert_eq!(loaded.created_at, created_at);
}

#[tokio::test]
async fn repository_rejects_second_library_even_with_different_path() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;

    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        "/tmp/caudex-library",
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("first insert should succeed");

    let second = insert_library(
        &pool,
        "Second Library",
        "/tmp/another-library",
        "2026-03-05T10:35:00Z",
    )
    .await;

    assert!(
        second.is_err(),
        "a second library should be rejected to preserve singleton configuration"
    );
}

#[tokio::test]
async fn singleton_migration_deduplicates_legacy_rows_before_unique_index() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;

    sqlx::query(
        r#"
        CREATE TABLE libraries (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL,
          path TEXT NOT NULL UNIQUE,
          created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .expect("legacy libraries table should be created");

    sqlx::query(
        r#"
        INSERT INTO libraries (name, path, created_at)
        VALUES ('A', '/tmp/a', '2026-03-05T10:30:00Z'),
               ('B', '/tmp/b', '2026-03-05T10:35:00Z');
        "#,
    )
    .execute(&pool)
    .await
    .expect("legacy duplicate rows should be inserted");

    let migration_sql = include_str!("../migrations/0002_enforce_single_library.sql");
    for statement in migration_sql
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("statement should execute");
    }

    let remaining_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM libraries")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");
    assert_eq!(
        remaining_count, 1,
        "migration should keep one canonical row before enforcing singleton index"
    );

    let second = insert_library(
        &pool,
        "Second Library",
        "/tmp/another-library",
        "2026-03-05T10:40:00Z",
    )
    .await;
    assert!(
        second.is_err(),
        "singleton index should reject new second row"
    );
}
