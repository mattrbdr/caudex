use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::metadata::{
    get_library_item_metadata_with_pool, list_library_items_with_pool, GetLibraryItemMetadataInput,
    ListLibraryItemsInput,
};
use appsdesktop_lib::metadata_collections::{
    assign_item_tags_collections_with_pool, list_metadata_collections_with_pool,
    list_metadata_tags_with_pool, AssignItemTagsCollectionsInput,
};
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

async fn seed_item(
    pool: &sqlx::SqlitePool,
    source: &str,
    title: &str,
    authors: &str,
    language: &str,
    published_at: &str,
) -> i64 {
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
        VALUES (1, ?, 'epub', ?, ?, ?, ?, '2026-03-05T10:35:00Z')
        "#,
    )
    .bind(source)
    .bind(title)
    .bind(authors)
    .bind(language)
    .bind(published_at)
    .execute(pool)
    .await
    .expect("item insert should succeed")
    .last_insert_rowid()
}

#[tokio::test]
async fn taxonomy_assignment_and_filtered_listing_work() {
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

    let item_a = seed_item(
        &pool,
        "/tmp/a.epub",
        "Book A",
        r#"["Alice"]"#,
        "en",
        "2024-01-01",
    )
    .await;
    let _item_b = seed_item(
        &pool,
        "/tmp/b.epub",
        "Book B",
        r#"["Bob"]"#,
        "fr",
        "2023-01-01",
    )
    .await;

    assign_item_tags_collections_with_pool(
        AssignItemTagsCollectionsInput {
            item_id: item_a,
            tags: vec!["to-read".to_string(), "To-Read".to_string()],
            collections: vec!["Classics".to_string()],
        },
        &pool,
    )
    .await
    .expect("taxonomy assignment should succeed");

    let tags = list_metadata_tags_with_pool(&pool)
        .await
        .expect("tags should list");
    let collections = list_metadata_collections_with_pool(&pool)
        .await
        .expect("collections should list");

    assert_eq!(tags.names, vec!["to-read".to_string()]);
    assert_eq!(collections.names, vec!["Classics".to_string()]);

    let filtered = list_library_items_with_pool(
        ListLibraryItemsInput {
            page: Some(1),
            page_size: Some(50),
            author: Some("Alice".to_string()),
            language: Some("en".to_string()),
            published_from: Some("2023-01-01".to_string()),
            published_to: Some("2024-12-31".to_string()),
            tag: Some("to-read".to_string()),
            collection: Some("Classics".to_string()),
            sort_by: Some("title".to_string()),
            sort_direction: Some("asc".to_string()),
        },
        &pool,
    )
    .await
    .expect("filtered list should succeed");

    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].id, item_a);
    assert_eq!(filtered.items[0].tags, vec!["to-read".to_string()]);
    assert_eq!(filtered.items[0].collections, vec!["Classics".to_string()]);
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

    let item_id = seed_item(
        &pool,
        "/tmp/a.epub",
        "Book A",
        r#"["Alice"]"#,
        "en",
        "2024-01-01",
    )
    .await;

    let detected = detect_metadata_conflicts_with_pool(
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

    assert_eq!(detected.conflicts.len(), 1);
    assert_eq!(detected.conflicts[0].status, "pending");

    let resolved = resolve_metadata_conflict_with_pool(
        ResolveMetadataConflictInput {
            conflict_id: detected.conflicts[0].id,
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
