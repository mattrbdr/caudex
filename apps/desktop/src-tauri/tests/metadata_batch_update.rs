use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::metadata::{get_library_item_metadata_with_pool, GetLibraryItemMetadataInput};
use appsdesktop_lib::metadata_batch::{
    execute_batch_metadata_update_with_pool, preview_batch_metadata_update_with_pool,
    BatchMetadataPatchInput, BatchMetadataUpdateInput,
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

async fn seed_item(pool: &sqlx::SqlitePool, source: &str, title: &str) -> i64 {
    sqlx::query(
        r#"
        INSERT INTO library_items (
          library_id,
          source_path,
          format,
          title,
          authors,
          language,
          published_at,
          imported_at
        )
        VALUES (1, ?, 'epub', ?, ?, 'en', '2024-01-01', '2026-03-05T10:35:00Z')
        "#,
    )
    .bind(source)
    .bind(title)
    .bind(r#"["Alice"]"#)
    .execute(pool)
    .await
    .expect("item insert should succeed")
    .last_insert_rowid()
}

#[tokio::test]
async fn preview_run_id_is_deterministic_for_same_payload() {
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
    .expect("library should be created");

    let item_a = seed_item(&pool, "/tmp/a.epub", "Title A").await;
    let item_b = seed_item(&pool, "/tmp/b.epub", "Title B").await;

    let input = BatchMetadataUpdateInput {
        item_ids: vec![item_b, item_a],
        patch: BatchMetadataPatchInput {
            title: Some("Batch Title".to_string()),
            authors: None,
            language: None,
            published_at: None,
            tags: None,
            collections: None,
        },
    };

    let first = preview_batch_metadata_update_with_pool(input.clone(), &pool)
        .await
        .expect("preview should succeed");
    let second = preview_batch_metadata_update_with_pool(input, &pool)
        .await
        .expect("preview should succeed");

    assert_eq!(first.run_id, second.run_id);
    assert_eq!(first.updated_count, 2);
    assert_eq!(first.failed_count, 0);
}

#[tokio::test]
async fn execute_reports_partial_failures_and_persists_results() {
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
    .expect("library should be created");

    let item_a = seed_item(&pool, "/tmp/a.epub", "Title A").await;

    let result = execute_batch_metadata_update_with_pool(
        BatchMetadataUpdateInput {
            item_ids: vec![item_a, 999_999],
            patch: BatchMetadataPatchInput {
                title: Some("Batch Updated".to_string()),
                authors: None,
                language: None,
                published_at: None,
                tags: Some(vec!["urgent".to_string(), "Urgent".to_string()]),
                collections: Some(vec!["Sprint".to_string()]),
            },
        },
        &pool,
    )
    .await
    .expect("execute should succeed");

    assert_eq!(result.status, "partial_success");
    assert_eq!(result.updated_count, 1);
    assert_eq!(result.failed_count, 1);

    let updated =
        get_library_item_metadata_with_pool(GetLibraryItemMetadataInput { item_id: item_a }, &pool)
            .await
            .expect("updated item should be available");
    assert_eq!(updated.title, "Batch Updated");
    assert_eq!(updated.tags, vec!["urgent".to_string()]);
    assert_eq!(updated.collections, vec!["Sprint".to_string()]);

    let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metadata_batch_runs")
        .fetch_one(&pool)
        .await
        .expect("run count query should succeed");
    let result_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM metadata_batch_results")
        .fetch_one(&pool)
        .await
        .expect("result count query should succeed");

    assert_eq!(run_count, 1);
    assert_eq!(result_count, 2);

    let queued_units: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM index_work_units WHERE library_item_id = ? AND status = 'queued'",
    )
    .bind(item_a)
    .fetch_one(&pool)
    .await
    .expect("batch update should enqueue index refresh");
    assert_eq!(queued_units, 1);
}
