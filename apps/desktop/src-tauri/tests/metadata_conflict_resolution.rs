use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::metadata::{get_library_item_metadata_with_pool, GetLibraryItemMetadataInput};
use appsdesktop_lib::metadata_conflicts::{
    detect_metadata_conflicts_with_pool, list_metadata_conflicts_with_pool,
    resolve_metadata_conflict_with_pool, DetectMetadataConflictsInput, ListMetadataConflictsInput,
    MetadataConflictCandidateInput, ResolveMetadataConflictInput,
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

async fn seed_item(pool: &sqlx::SqlitePool) -> i64 {
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
        VALUES (1, '/tmp/a.epub', 'epub', 'Book A', '["Alice"]', 'en', '2024-01-01', '2026-03-05T10:35:00Z')
        "#,
    )
    .execute(pool)
    .await
    .expect("item insert should succeed")
    .last_insert_rowid()
}

#[tokio::test]
async fn conflict_detection_and_resolution_are_explicit_and_auditable() {
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

    let item_id = seed_item(&pool).await;

    let first_detect = detect_metadata_conflicts_with_pool(
        DetectMetadataConflictsInput {
            item_id,
            candidate: MetadataConflictCandidateInput {
                title: Some("Book A Revised".to_string()),
                authors: None,
                language: None,
                published_at: None,
            },
            source: Some("manual_edit".to_string()),
        },
        &pool,
    )
    .await
    .expect("detection should succeed");

    let second_detect = detect_metadata_conflicts_with_pool(
        DetectMetadataConflictsInput {
            item_id,
            candidate: MetadataConflictCandidateInput {
                title: Some("Book A Revised".to_string()),
                authors: None,
                language: None,
                published_at: None,
            },
            source: Some("manual_edit".to_string()),
        },
        &pool,
    )
    .await
    .expect("duplicate detection should succeed");

    assert_eq!(first_detect.conflicts.len(), 1);
    assert_eq!(second_detect.conflicts.len(), 1);
    assert_eq!(first_detect.conflicts[0].id, second_detect.conflicts[0].id);

    let resolved = resolve_metadata_conflict_with_pool(
        ResolveMetadataConflictInput {
            conflict_id: first_detect.conflicts[0].id,
            resolution: "use_candidate".to_string(),
            rationale: Some("Curator approved candidate title".to_string()),
        },
        &pool,
    )
    .await
    .expect("resolution should succeed");

    assert_eq!(resolved.conflict.status, "resolved_use_candidate");
    assert_eq!(resolved.item.title, "Book A Revised");

    let refreshed =
        get_library_item_metadata_with_pool(GetLibraryItemMetadataInput { item_id }, &pool)
            .await
            .expect("item should be updated");
    assert_eq!(refreshed.title, "Book A Revised");

    let pending = list_metadata_conflicts_with_pool(
        ListMetadataConflictsInput {
            item_id,
            status: Some("pending".to_string()),
        },
        &pool,
    )
    .await
    .expect("pending list should succeed");
    assert!(pending.conflicts.is_empty());

    let all = list_metadata_conflicts_with_pool(
        ListMetadataConflictsInput {
            item_id,
            status: None,
        },
        &pool,
    )
    .await
    .expect("full list should succeed");
    assert_eq!(all.conflicts.len(), 1);
    assert_eq!(all.conflicts[0].status, "resolved_use_candidate");
}
