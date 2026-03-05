use appsdesktop_lib::ingest::{start_import_with_pool, StartImportInput};
use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::search_index::{
    ensure_search_index_health_on_startup_with_pool, ensure_search_index_health_with_pool,
    index_root_path_with_pool, process_index_work_queue_with_pool,
    retry_failed_index_work_units_with_pool, ProcessIndexWorkQueueInput,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Barrier;

async fn setup_pool(path: PathBuf) -> sqlx::SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    SqlitePoolOptions::new()
        .max_connections(4)
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
async fn failed_units_can_be_retried_and_marked_recovered() {
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

    let pdf = temp.path().join("recovery-book.pdf");
    write_pdf(&pdf);

    start_import_with_pool(
        StartImportInput {
            paths: vec![pdf.to_string_lossy().to_string()],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    let index_root = index_root_path_with_pool(&pool)
        .await
        .expect("index path should resolve");

    if index_root.exists() {
        if index_root.is_dir() {
            std::fs::remove_dir_all(&index_root).expect("existing index dir should be removed");
        } else {
            std::fs::remove_file(&index_root).expect("existing index file should be removed");
        }
    }

    std::fs::create_dir_all(index_root.parent().expect("index dir parent should exist"))
        .expect("index dir parent should be created");
    std::fs::write(&index_root, b"not-a-directory")
        .expect("index root file should be created to force failure");

    let failed_run = process_index_work_queue_with_pool(
        ProcessIndexWorkQueueInput {
            batch_size: Some(50),
            include_failed: Some(false),
        },
        &pool,
    )
    .await
    .expect("processing should complete with failure");

    assert_eq!(failed_run.failed_count, 1);

    let failed_status: String =
        sqlx::query_scalar("SELECT status FROM index_work_units ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("status query should succeed");
    assert_eq!(failed_status, "failed");

    std::fs::remove_file(&index_root).expect("corrupt index file should be removed");

    let retry = retry_failed_index_work_units_with_pool(None, &pool)
        .await
        .expect("retry marking should succeed");
    assert_eq!(retry.marked_retry_count, 1);

    let recovered_run = process_index_work_queue_with_pool(
        ProcessIndexWorkQueueInput {
            batch_size: Some(50),
            include_failed: Some(false),
        },
        &pool,
    )
    .await
    .expect("recovery processing should succeed");

    assert_eq!(recovered_run.success_count, 1);

    let recovered_status: String =
        sqlx::query_scalar("SELECT status FROM index_work_units ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("status query should succeed");
    assert_eq!(recovered_status, "recovered");
}

#[tokio::test]
async fn missing_index_root_triggers_repair_and_rebuild_queueing() {
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

    let pdf = temp.path().join("repair-book.pdf");
    write_pdf(&pdf);

    start_import_with_pool(
        StartImportInput {
            paths: vec![pdf.to_string_lossy().to_string()],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    let index_root = index_root_path_with_pool(&pool)
        .await
        .expect("index path should resolve");

    if index_root.exists() {
        std::fs::remove_dir_all(&index_root)
            .expect("index root should be removed before health check");
    }

    let health = ensure_search_index_health_with_pool(&pool)
        .await
        .expect("health check should succeed");

    assert!(health.repair_performed);
    let queued_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM index_work_units WHERE status = 'queued'")
            .fetch_one(&pool)
            .await
            .expect("queued count query should succeed");
    assert!(queued_count >= 1);
    assert!(index_root.exists());
    assert!(index_root.is_dir());
}

#[tokio::test]
async fn startup_health_check_recovers_interrupted_running_units() {
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

    let pdf = temp.path().join("startup-recovery-book.pdf");
    write_pdf(&pdf);
    start_import_with_pool(
        StartImportInput {
            paths: vec![pdf.to_string_lossy().to_string()],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    sqlx::query(
        "UPDATE index_work_units SET status = 'running', attempt_count = 1, updated_at = ?",
    )
    .bind("2026-03-05T10:45:00Z")
    .execute(&pool)
    .await
    .expect("work unit should be put in running state");

    let health = ensure_search_index_health_on_startup_with_pool(&pool)
        .await
        .expect("startup health check should succeed");

    assert!(health
        .diagnostic
        .contains("Recovered 1 interrupted running work unit"));

    let status: String = sqlx::query_scalar("SELECT status FROM index_work_units LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("status query should succeed");
    assert_eq!(status, "retry");
}

#[tokio::test]
async fn concurrent_queue_processors_claim_each_unit_once() {
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

    let pdf = temp.path().join("concurrency-book.pdf");
    write_pdf(&pdf);
    start_import_with_pool(
        StartImportInput {
            paths: vec![pdf.to_string_lossy().to_string()],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    let barrier = Arc::new(Barrier::new(3));
    let pool_a = pool.clone();
    let barrier_a = barrier.clone();
    let handle_a = tokio::spawn(async move {
        barrier_a.wait().await;
        process_index_work_queue_with_pool(
            ProcessIndexWorkQueueInput {
                batch_size: Some(50),
                include_failed: Some(false),
            },
            &pool_a,
        )
        .await
        .expect("processor A should succeed")
    });

    let pool_b = pool.clone();
    let barrier_b = barrier.clone();
    let handle_b = tokio::spawn(async move {
        barrier_b.wait().await;
        process_index_work_queue_with_pool(
            ProcessIndexWorkQueueInput {
                batch_size: Some(50),
                include_failed: Some(false),
            },
            &pool_b,
        )
        .await
        .expect("processor B should succeed")
    });

    barrier.wait().await;
    let result_a = handle_a.await.expect("processor A task should join");
    let result_b = handle_b.await.expect("processor B task should join");

    assert_eq!(result_a.processed_count + result_b.processed_count, 1);
    assert_eq!(result_a.failed_count + result_b.failed_count, 0);
}
