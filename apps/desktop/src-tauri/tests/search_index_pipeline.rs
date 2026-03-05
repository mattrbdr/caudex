use appsdesktop_lib::ingest::{start_import_with_pool, StartImportInput};
use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::search_index::{
    get_index_queue_status_with_pool, process_index_work_queue_with_pool,
    search_index_documents_with_pool, ProcessIndexWorkQueueInput,
};
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

fn write_pdf(path: &std::path::Path) {
    std::fs::write(
        path,
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\nxref\n0 1\n0000000000 65535 f \ntrailer\n<<>>\nstartxref\n0\n%%EOF",
    )
    .expect("pdf fixture should be created");
}

#[tokio::test]
async fn process_queue_indexes_imported_items_and_marks_success() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let pdf = temp.path().join("indexed-book.pdf");
    write_pdf(&pdf);

    start_import_with_pool(
        StartImportInput {
            paths: vec![pdf.to_string_lossy().to_string()],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    let process_result = process_index_work_queue_with_pool(
        ProcessIndexWorkQueueInput {
            batch_size: Some(50),
            include_failed: Some(false),
        },
        &pool,
    )
    .await
    .expect("queue processing should succeed");

    assert_eq!(process_result.processed_count, 1);
    assert_eq!(process_result.success_count, 1);
    assert_eq!(process_result.failed_count, 0);

    let queue_status = get_index_queue_status_with_pool(&pool)
        .await
        .expect("status should load");
    assert_eq!(queue_status.queued_count, 0);
    assert_eq!(queue_status.failed_count, 0);
    assert_eq!(queue_status.success_count, 1);

    let hits = search_index_documents_with_pool("indexed", 10, &pool)
        .await
        .expect("search should succeed");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "indexed-book");
}

#[tokio::test]
async fn process_queue_is_idempotent_and_keeps_latest_state_for_same_item() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let pdf = temp.path().join("first-title.pdf");
    write_pdf(&pdf);

    start_import_with_pool(
        StartImportInput {
            paths: vec![pdf.to_string_lossy().to_string()],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    process_index_work_queue_with_pool(
        ProcessIndexWorkQueueInput {
            batch_size: Some(50),
            include_failed: Some(false),
        },
        &pool,
    )
    .await
    .expect("first queue processing should succeed");

    let item_id: i64 = sqlx::query_scalar("SELECT id FROM library_items LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("item should exist");

    sqlx::query("UPDATE library_items SET title = ? WHERE id = ?")
        .bind("zzq-new-title")
        .bind(item_id)
        .execute(&pool)
        .await
        .expect("title update should succeed");

    sqlx::query(
        "INSERT INTO index_work_units (library_item_id, status, created_at, updated_at) VALUES (?, 'queued', ?, ?)",
    )
    .bind(item_id)
    .bind("2026-03-05T11:00:00Z")
    .bind("2026-03-05T11:00:00Z")
    .execute(&pool)
    .await
    .expect("new work unit should be queued");

    process_index_work_queue_with_pool(
        ProcessIndexWorkQueueInput {
            batch_size: Some(50),
            include_failed: Some(false),
        },
        &pool,
    )
    .await
    .expect("second queue processing should succeed");

    let hits = search_index_documents_with_pool("zzq", 10, &pool)
        .await
        .expect("search should succeed");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].library_item_id, item_id);
    assert_eq!(hits[0].title, "zzq-new-title");
}
