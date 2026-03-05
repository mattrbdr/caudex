use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::metadata::{
    get_library_item_metadata_with_pool, list_library_items_with_pool,
    update_library_item_metadata_with_pool, GetLibraryItemMetadataInput, ListLibraryItemsInput,
    UpdateLibraryItemMetadataInput,
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

async fn seed_library_item(pool: &sqlx::SqlitePool) -> i64 {
    insert_library(
        pool,
        "Main Library",
        "/tmp/caudex-library",
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library should be created");

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
        VALUES (1, ?, 'epub', 'Old Title', ?, 'en', '2024-01-01', '2026-03-05T10:35:00Z')
        "#,
    )
    .bind("/tmp/book.epub")
    .bind(r#"["Alice","Bob"]"#)
    .execute(pool)
    .await
    .expect("item insert should succeed")
    .last_insert_rowid()
}

#[tokio::test]
async fn list_items_returns_stable_paginated_order() {
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

    for idx in 0..3 {
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
        .bind(format!("/tmp/book-{idx}.epub"))
        .bind(format!("Book {idx}"))
        .bind(r#"["Alice"]"#)
        .execute(&pool)
        .await
        .expect("item insert should succeed");
    }

    let page_one = list_library_items_with_pool(
        ListLibraryItemsInput {
            page: Some(1),
            page_size: Some(2),
            author: None,
            language: None,
            published_from: None,
            published_to: None,
            tag: None,
            collection: None,
            sort_by: None,
            sort_direction: None,
        },
        &pool,
    )
    .await
    .expect("list should succeed");
    let page_two = list_library_items_with_pool(
        ListLibraryItemsInput {
            page: Some(2),
            page_size: Some(2),
            author: None,
            language: None,
            published_from: None,
            published_to: None,
            tag: None,
            collection: None,
            sort_by: None,
            sort_direction: None,
        },
        &pool,
    )
    .await
    .expect("list should succeed");

    assert_eq!(page_one.items.len(), 2);
    assert_eq!(page_two.items.len(), 1);
    assert_eq!(page_one.total, 3);
    assert!(page_one.items[0].id < page_one.items[1].id);
    assert!(page_one.items[1].id < page_two.items[0].id);
}

#[tokio::test]
async fn update_metadata_succeeds_and_normalizes_authors() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    let item_id = seed_library_item(&pool).await;

    let updated = update_library_item_metadata_with_pool(
        UpdateLibraryItemMetadataInput {
            item_id,
            title: "  New Title  ".to_string(),
            authors: vec![
                " Alice ".to_string(),
                "".to_string(),
                "alice".to_string(),
                "Bob".to_string(),
            ],
            language: Some("fr".to_string()),
            published_at: Some("2024-12-31".to_string()),
            tags: None,
            collections: None,
        },
        &pool,
    )
    .await
    .expect("update should succeed");

    assert_eq!(updated.title, "New Title");
    assert_eq!(
        updated.authors,
        vec!["Alice".to_string(), "Bob".to_string()]
    );
    assert_eq!(updated.language.as_deref(), Some("fr"));
    assert_eq!(updated.published_at.as_deref(), Some("2024-12-31"));
}

#[tokio::test]
async fn update_metadata_rejects_invalid_values() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    let item_id = seed_library_item(&pool).await;

    let title_err = update_library_item_metadata_with_pool(
        UpdateLibraryItemMetadataInput {
            item_id,
            title: "   ".to_string(),
            authors: vec!["Alice".to_string()],
            language: Some("en".to_string()),
            published_at: Some("2024-01-01".to_string()),
            tags: None,
            collections: None,
        },
        &pool,
    )
    .await
    .expect_err("empty title should fail");
    assert!(title_err.contains("Title is required"));

    let lang_err = update_library_item_metadata_with_pool(
        UpdateLibraryItemMetadataInput {
            item_id,
            title: "Valid".to_string(),
            authors: vec!["Alice".to_string()],
            language: Some("english".to_string()),
            published_at: Some("2024-01-01".to_string()),
            tags: None,
            collections: None,
        },
        &pool,
    )
    .await
    .expect_err("invalid language should fail");
    assert!(lang_err.contains("Language must be"));

    let date_err = update_library_item_metadata_with_pool(
        UpdateLibraryItemMetadataInput {
            item_id,
            title: "Valid".to_string(),
            authors: vec!["Alice".to_string()],
            language: Some("en".to_string()),
            published_at: Some("2024-31-12".to_string()),
            tags: None,
            collections: None,
        },
        &pool,
    )
    .await
    .expect_err("invalid date should fail");
    assert!(date_err.contains("Published date must be"));
}

#[tokio::test]
async fn no_op_update_keeps_metadata_stable() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    let item_id = seed_library_item(&pool).await;

    let original =
        get_library_item_metadata_with_pool(GetLibraryItemMetadataInput { item_id }, &pool)
            .await
            .expect("should fetch item");

    let updated = update_library_item_metadata_with_pool(
        UpdateLibraryItemMetadataInput {
            item_id,
            title: original.title.clone(),
            authors: original.authors.clone(),
            language: original.language.clone(),
            published_at: original.published_at.clone(),
            tags: None,
            collections: None,
        },
        &pool,
    )
    .await
    .expect("no-op update should still succeed");

    assert_eq!(updated.title, original.title);
    assert_eq!(updated.authors, original.authors);
    assert_eq!(updated.language, original.language);
    assert_eq!(updated.published_at, original.published_at);
}

#[tokio::test]
async fn concurrent_updates_do_not_corrupt_row_shape() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    let item_id = seed_library_item(&pool).await;

    let pool_a = pool.clone();
    let pool_b = pool.clone();

    let first = tokio::spawn(async move {
        update_library_item_metadata_with_pool(
            UpdateLibraryItemMetadataInput {
                item_id,
                title: "Concurrent A".to_string(),
                authors: vec!["Alice".to_string()],
                language: Some("en".to_string()),
                published_at: Some("2024-06-01".to_string()),
                tags: None,
                collections: None,
            },
            &pool_a,
        )
        .await
    });

    let second = tokio::spawn(async move {
        update_library_item_metadata_with_pool(
            UpdateLibraryItemMetadataInput {
                item_id,
                title: "Concurrent B".to_string(),
                authors: vec!["Bob".to_string(), "Bob".to_string()],
                language: Some("fr".to_string()),
                published_at: Some("2024-06-02".to_string()),
                tags: None,
                collections: None,
            },
            &pool_b,
        )
        .await
    });

    let _ = first.await.expect("first task should complete");
    let _ = second.await.expect("second task should complete");

    let final_row =
        get_library_item_metadata_with_pool(GetLibraryItemMetadataInput { item_id }, &pool)
            .await
            .expect("final fetch should succeed");

    assert!(!final_row.title.trim().is_empty());
    assert!(!final_row.authors.is_empty());
    assert!(final_row.language.is_some());
    assert!(final_row.published_at.is_some());
}
