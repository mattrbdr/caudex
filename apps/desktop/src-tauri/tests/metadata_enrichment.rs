use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::metadata::{get_library_item_metadata_with_pool, GetLibraryItemMetadataInput};
use appsdesktop_lib::metadata_enrichment::{
    apply_metadata_enrichment_proposal_with_pool, enrich_library_item_metadata_with_providers,
    list_metadata_enrichment_proposals_with_pool, ApplyMetadataEnrichmentProposalInput,
    EnrichLibraryItemMetadataInput, ListMetadataEnrichmentProposalsInput, MetadataCandidate,
    MetadataProvider, ProviderFuture,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

async fn seed_library(pool: &sqlx::SqlitePool) {
    insert_library(
        pool,
        "Main Library",
        "/tmp/caudex-library",
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library should be created");
}

async fn seed_item(pool: &sqlx::SqlitePool, title: &str) -> i64 {
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
    .bind(format!("/tmp/{title}.epub"))
    .bind(title)
    .bind(r#"["Alice"]"#)
    .execute(pool)
    .await
    .expect("item insert should succeed")
    .last_insert_rowid()
}

#[derive(Clone)]
struct FakeProvider {
    name: &'static str,
    isbn_responses: Arc<Mutex<Vec<Result<Option<MetadataCandidate>, String>>>>,
    title_responses: Arc<Mutex<Vec<Result<Option<MetadataCandidate>, String>>>>,
}

impl FakeProvider {
    fn new(
        name: &'static str,
        isbn_responses: Vec<Result<Option<MetadataCandidate>, String>>,
        title_responses: Vec<Result<Option<MetadataCandidate>, String>>,
    ) -> Self {
        Self {
            name,
            isbn_responses: Arc::new(Mutex::new(isbn_responses)),
            title_responses: Arc::new(Mutex::new(title_responses)),
        }
    }

    fn pop_response(
        responses: &Arc<Mutex<Vec<Result<Option<MetadataCandidate>, String>>>>,
    ) -> Result<Option<MetadataCandidate>, String> {
        let mut guard = responses.lock().expect("lock should not be poisoned");
        if guard.is_empty() {
            return Ok(None);
        }
        guard.remove(0)
    }
}

impl MetadataProvider for FakeProvider {
    fn provider_name(&self) -> &'static str {
        self.name
    }

    fn lookup_by_isbn<'a>(&'a self, _isbn: &'a str) -> ProviderFuture<'a> {
        Box::pin(async move { Self::pop_response(&self.isbn_responses) })
    }

    fn lookup_by_title_author<'a>(
        &'a self,
        _title: &'a str,
        _authors: &'a [String],
    ) -> ProviderFuture<'a> {
        Box::pin(async move { Self::pop_response(&self.title_responses) })
    }
}

fn candidate(provider_payload: &str, confidence: f64) -> MetadataCandidate {
    MetadataCandidate {
        title: Some("Enriched Title".to_string()),
        authors: vec!["Alice".to_string(), "Bob".to_string()],
        language: Some("fr".to_string()),
        published_at: Some("2024-12-31".to_string()),
        confidence,
        raw_payload: provider_payload.to_string(),
    }
}

#[tokio::test]
async fn fallback_provider_used_when_primary_fails() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_id = seed_item(&pool, "book-9781234567890").await;

    let primary = FakeProvider::new(
        "google_books",
        vec![Err("timeout".to_string()), Err("timeout".to_string()), Err("timeout".to_string())],
        vec![],
    );
    let fallback = FakeProvider::new(
        "open_library",
        vec![Ok(Some(candidate("{\"provider\":\"open\"}", 0.55)))],
        vec![],
    );

    let providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(primary), Box::new(fallback)];
    let result = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id },
        providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should complete");

    assert_eq!(result.status, "degraded");
    assert_eq!(result.proposals.len(), 1);
    assert_eq!(result.proposals[0].provider, "open_library");
    assert!(
        result
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("google_books")
    );
}

#[tokio::test]
async fn retry_exhaustion_marks_run_failed() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_id = seed_item(&pool, "book-9781234567890").await;

    let primary = FakeProvider::new(
        "google_books",
        vec![Err("timeout".to_string()), Err("timeout".to_string()), Err("timeout".to_string())],
        vec![],
    );
    let fallback = FakeProvider::new("open_library", vec![Ok(None)], vec![]);
    let providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(primary), Box::new(fallback)];

    let result = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id },
        providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should complete");

    assert_eq!(result.status, "failed");
    assert_eq!(result.proposals.len(), 0);
}

#[tokio::test]
async fn enrichment_persists_provenance_and_confidence() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_id = seed_item(&pool, "book-9781234567890").await;

    let provider = FakeProvider::new(
        "google_books",
        vec![Ok(Some(candidate("{\"provider\":\"google\"}", 0.91)))],
        vec![],
    );
    let providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(provider)];

    let result = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id },
        providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should complete");

    assert_eq!(result.status, "success");
    assert_eq!(result.proposals.len(), 1);
    assert_eq!(result.proposals[0].provider, "google_books");
    assert_eq!(result.proposals[0].confidence, 0.91);

    let persisted = list_metadata_enrichment_proposals_with_pool(
        ListMetadataEnrichmentProposalsInput { item_id },
        &pool,
    )
    .await
    .expect("proposals should be listed");
    assert!(!persisted.proposals.is_empty());
    assert_eq!(persisted.proposals[0].provider, "google_books");
}

#[tokio::test]
async fn apply_proposal_updates_item_and_marks_applied() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_id = seed_item(&pool, "book-9781234567890").await;

    let provider = FakeProvider::new(
        "google_books",
        vec![Ok(Some(candidate("{\"provider\":\"google\"}", 0.91)))],
        vec![],
    );
    let providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(provider)];

    let run = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id },
        providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should complete");

    let proposal_id = run.proposals[0].id;
    let apply_result = apply_metadata_enrichment_proposal_with_pool(
        ApplyMetadataEnrichmentProposalInput { proposal_id },
        &pool,
    )
    .await
    .expect("proposal should apply");

    assert_eq!(apply_result.item.title, "Enriched Title");
    assert_eq!(apply_result.item.language.as_deref(), Some("fr"));
    assert_eq!(apply_result.item.published_at.as_deref(), Some("2024-12-31"));

    let refreshed = get_library_item_metadata_with_pool(
        GetLibraryItemMetadataInput { item_id },
        &pool,
    )
    .await
    .expect("item should be loadable");
    assert_eq!(refreshed.title, "Enriched Title");

    let applied_at: Option<String> =
        sqlx::query_scalar("SELECT applied_at FROM metadata_enrichment_proposals WHERE id = ?")
            .bind(proposal_id)
            .fetch_one(&pool)
            .await
            .expect("proposal should be queryable");
    assert!(applied_at.is_some());
}

#[tokio::test]
async fn failed_enrichment_for_one_item_does_not_block_another_item() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_a = seed_item(&pool, "book-a-9781234567890").await;
    let item_b = seed_item(&pool, "book-b-9781234567890").await;

    let failing_provider = FakeProvider::new(
        "google_books",
        vec![Err("timeout".to_string()), Err("timeout".to_string()), Err("timeout".to_string())],
        vec![],
    );
    let empty_provider = FakeProvider::new("open_library", vec![Ok(None)], vec![]);
    let failed_providers: Vec<Box<dyn MetadataProvider>> =
        vec![Box::new(failing_provider), Box::new(empty_provider)];
    let failed_result = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id: item_a },
        failed_providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should return failed state");
    assert_eq!(failed_result.status, "failed");

    let success_provider = FakeProvider::new(
        "google_books",
        vec![Ok(Some(candidate("{\"provider\":\"google\"}", 0.88)))],
        vec![],
    );
    let success_providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(success_provider)];
    let success_result = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id: item_b },
        success_providers.as_slice(),
        &pool,
    )
    .await
    .expect("second enrichment should still succeed");
    assert_eq!(success_result.status, "success");
    assert_eq!(success_result.proposals.len(), 1);
}

#[tokio::test]
async fn title_author_lookup_is_used_when_isbn_lookup_returns_none() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_id = seed_item(&pool, "book-9781234567890").await;

    let provider = FakeProvider::new(
        "google_books",
        vec![Ok(None)],
        vec![Ok(Some(candidate("{\"provider\":\"google\"}", 0.77)))],
    );
    let providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(provider)];

    let result = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id },
        providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should complete");

    assert_eq!(result.status, "success");
    assert_eq!(result.proposals.len(), 1);
    assert_eq!(result.proposals[0].provider, "google_books");
    assert_eq!(result.proposals[0].confidence, 0.77);
}

#[tokio::test]
async fn apply_proposal_is_idempotent_and_rejects_second_apply() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");
    seed_library(&pool).await;
    let item_id = seed_item(&pool, "book-9781234567890").await;

    let provider = FakeProvider::new(
        "google_books",
        vec![Ok(Some(candidate("{\"provider\":\"google\"}", 0.91)))],
        vec![],
    );
    let providers: Vec<Box<dyn MetadataProvider>> = vec![Box::new(provider)];

    let run = enrich_library_item_metadata_with_providers(
        EnrichLibraryItemMetadataInput { item_id },
        providers.as_slice(),
        &pool,
    )
    .await
    .expect("enrichment should complete");

    let proposal_id = run.proposals[0].id;
    apply_metadata_enrichment_proposal_with_pool(ApplyMetadataEnrichmentProposalInput { proposal_id }, &pool)
        .await
        .expect("first apply should succeed");

    let second_apply =
        apply_metadata_enrichment_proposal_with_pool(ApplyMetadataEnrichmentProposalInput { proposal_id }, &pool)
            .await
            .expect_err("second apply should fail");

    assert!(second_apply.contains("already applied"));
}
