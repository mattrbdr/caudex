use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{Manager, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub mod ingest;
pub mod library;
pub mod metadata;
pub mod metadata_batch;
pub mod metadata_collections;
pub mod metadata_conflicts;
pub mod metadata_enrichment;
pub mod providers;
pub mod search_index;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct CreateLibraryInput {
    name: String,
    path: String,
}

fn report_internal_error(context: &str, error: &dyn Display, user_message: &str) -> String {
    eprintln!("{context}: {error}");
    user_message.to_string()
}

fn normalize_create_input(input: CreateLibraryInput) -> Result<(String, PathBuf), String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Library name is required.".to_string());
    }

    let raw_path = input.path.trim();
    if raw_path.is_empty() {
        return Err("Library path is required.".to_string());
    }

    Ok((name.to_string(), PathBuf::from(raw_path)))
}

fn absolutize_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| {
            report_internal_error(
                "Unable to resolve current directory",
                &error,
                "Unable to resolve library path.",
            )
        })
}

fn is_unique_constraint_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db_error) => {
            matches!(db_error.code().as_deref(), Some("2067" | "1555"))
                || db_error.message().contains("UNIQUE constraint failed")
        }
        _ => false,
    }
}

async fn create_library_with_pool<F>(
    input: CreateLibraryInput,
    pool: &SqlitePool,
    create_dir_all: F,
) -> Result<library::Library, String>
where
    F: Fn(&Path) -> std::io::Result<()>,
{
    let (name, path_buf) = normalize_create_input(input)?;

    if library::fetch_library(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to verify existing library",
                &error,
                "Unable to verify existing library configuration.",
            )
        })?
        .is_some()
    {
        return Err("A library is already configured for this installation.".to_string());
    }

    create_dir_all(&path_buf).map_err(|error| {
        report_internal_error(
            "Unable to prepare library directory",
            &error,
            "Unable to prepare the selected library location.",
        )
    })?;

    let resolved_path = match path_buf.canonicalize() {
        Ok(path) => path,
        Err(_) => absolutize_path(&path_buf)?,
    };

    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| {
            report_internal_error(
                "Unable to format creation timestamp",
                &error,
                "Unable to create library at this time.",
            )
        })?;

    library::insert_library(&pool, &name, &resolved_path.to_string_lossy(), &created_at)
        .await
        .map_err(|error| {
            if is_unique_constraint_error(&error) {
                "A library is already configured for this installation.".to_string()
            } else {
                report_internal_error(
                    "Unable to persist library configuration",
                    &error,
                    "Unable to save library configuration.",
                )
            }
        })
}

async fn initialize_pool(app_handle: &tauri::AppHandle) -> Result<SqlitePool, String> {
    let app_data_dir = app_handle.path().app_data_dir().map_err(|error| {
        report_internal_error(
            "Unable to resolve app data directory",
            &error,
            "Unable to initialize local storage.",
        )
    })?;

    fs::create_dir_all(&app_data_dir).map_err(|error| {
        report_internal_error(
            "Unable to create app data directory",
            &error,
            "Unable to initialize local storage.",
        )
    })?;

    let db_path = app_data_dir.join("caudex.db");
    let connect_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to connect to sqlite database",
                &error,
                "Unable to initialize local storage.",
            )
        })?;

    library::run_migrations(&pool).await.map_err(|error| {
        report_internal_error(
            "Unable to apply database migrations",
            &error,
            "Unable to initialize local storage.",
        )
    })?;

    Ok(pool)
}

#[tauri::command]
async fn get_library(state: State<'_, AppState>) -> Result<Option<library::Library>, String> {
    library::fetch_library(&state.pool).await.map_err(|error| {
        report_internal_error(
            "Unable to load library",
            &error,
            "Unable to load library state.",
        )
    })
}

#[tauri::command]
async fn create_library(
    input: CreateLibraryInput,
    state: State<'_, AppState>,
) -> Result<library::Library, String> {
    create_library_with_pool(input, &state.pool, |path| fs::create_dir_all(path)).await
}

#[tauri::command]
async fn start_import(
    input: ingest::StartImportInput,
    state: State<'_, AppState>,
) -> Result<ingest::ImportJobResult, String> {
    ingest::start_import_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn start_bulk_import(
    input: ingest::StartBulkImportInput,
    state: State<'_, AppState>,
) -> Result<ingest::ImportJobResult, String> {
    ingest::start_bulk_import_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn get_import_job_result(
    input: ingest::GetImportJobResultInput,
    state: State<'_, AppState>,
) -> Result<ingest::ImportJobResult, String> {
    ingest::get_import_job_result_with_pool(input.job_id, &state.pool).await
}

#[tauri::command]
async fn start_import_retry(
    input: ingest::StartImportRetryInput,
    state: State<'_, AppState>,
) -> Result<ingest::ImportJobResult, String> {
    ingest::start_import_retry_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn list_library_items(
    input: metadata::ListLibraryItemsInput,
    state: State<'_, AppState>,
) -> Result<metadata::ListLibraryItemsResult, String> {
    metadata::list_library_items_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn get_library_item_metadata(
    input: metadata::GetLibraryItemMetadataInput,
    state: State<'_, AppState>,
) -> Result<metadata::LibraryItemMetadata, String> {
    metadata::get_library_item_metadata_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn update_library_item_metadata(
    input: metadata::UpdateLibraryItemMetadataInput,
    state: State<'_, AppState>,
) -> Result<metadata::LibraryItemMetadata, String> {
    metadata::update_library_item_metadata_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn enrich_library_item_metadata(
    input: metadata_enrichment::EnrichLibraryItemMetadataInput,
    state: State<'_, AppState>,
) -> Result<metadata_enrichment::EnrichmentRunResult, String> {
    metadata_enrichment::enrich_library_item_metadata_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn preview_batch_metadata_update(
    input: metadata_batch::BatchMetadataUpdateInput,
    state: State<'_, AppState>,
) -> Result<metadata_batch::BatchMetadataRunResult, String> {
    metadata_batch::preview_batch_metadata_update_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn execute_batch_metadata_update(
    input: metadata_batch::BatchMetadataUpdateInput,
    state: State<'_, AppState>,
) -> Result<metadata_batch::BatchMetadataRunResult, String> {
    metadata_batch::execute_batch_metadata_update_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn list_metadata_enrichment_proposals(
    input: metadata_enrichment::ListMetadataEnrichmentProposalsInput,
    state: State<'_, AppState>,
) -> Result<metadata_enrichment::ListMetadataEnrichmentProposalsResult, String> {
    metadata_enrichment::list_metadata_enrichment_proposals_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn apply_metadata_enrichment_proposal(
    input: metadata_enrichment::ApplyMetadataEnrichmentProposalInput,
    state: State<'_, AppState>,
) -> Result<metadata_enrichment::ApplyMetadataEnrichmentProposalResult, String> {
    metadata_enrichment::apply_metadata_enrichment_proposal_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn assign_item_tags_collections(
    input: metadata_collections::AssignItemTagsCollectionsInput,
    state: State<'_, AppState>,
) -> Result<metadata_collections::AssignItemTagsCollectionsResult, String> {
    metadata_collections::assign_item_tags_collections_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn list_item_tags_collections(
    input: metadata_collections::ListItemTagsCollectionsInput,
    state: State<'_, AppState>,
) -> Result<metadata_collections::ItemTagsCollections, String> {
    metadata_collections::list_item_tags_collections_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn list_metadata_tags(
    state: State<'_, AppState>,
) -> Result<metadata_collections::ListMetadataNamesResult, String> {
    metadata_collections::list_metadata_tags_with_pool(&state.pool).await
}

#[tauri::command]
async fn list_metadata_collections(
    state: State<'_, AppState>,
) -> Result<metadata_collections::ListMetadataNamesResult, String> {
    metadata_collections::list_metadata_collections_with_pool(&state.pool).await
}

#[tauri::command]
async fn detect_metadata_conflicts(
    input: metadata_conflicts::DetectMetadataConflictsInput,
    state: State<'_, AppState>,
) -> Result<metadata_conflicts::DetectMetadataConflictsResult, String> {
    metadata_conflicts::detect_metadata_conflicts_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn list_metadata_conflicts(
    input: metadata_conflicts::ListMetadataConflictsInput,
    state: State<'_, AppState>,
) -> Result<metadata_conflicts::ListMetadataConflictsResult, String> {
    metadata_conflicts::list_metadata_conflicts_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn resolve_metadata_conflict(
    input: metadata_conflicts::ResolveMetadataConflictInput,
    state: State<'_, AppState>,
) -> Result<metadata_conflicts::ResolveMetadataConflictResult, String> {
    metadata_conflicts::resolve_metadata_conflict_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn get_index_queue_status(
    state: State<'_, AppState>,
) -> Result<search_index::IndexQueueStatus, String> {
    search_index::get_index_queue_status_with_pool(&state.pool).await
}

#[tauri::command]
async fn process_index_work_queue(
    input: search_index::ProcessIndexWorkQueueInput,
    state: State<'_, AppState>,
) -> Result<search_index::ProcessIndexWorkQueueResult, String> {
    search_index::process_index_work_queue_with_pool(input, &state.pool).await
}

#[tauri::command]
async fn retry_failed_index_work_units(
    input: search_index::RetryFailedIndexWorkUnitsInput,
    state: State<'_, AppState>,
) -> Result<search_index::RetryFailedIndexWorkUnitsResult, String> {
    search_index::retry_failed_index_work_units_with_pool(input.limit, &state.pool).await
}

#[tauri::command]
async fn ensure_search_index_health(
    state: State<'_, AppState>,
) -> Result<search_index::EnsureIndexHealthResult, String> {
    search_index::ensure_search_index_health_with_pool(&state.pool).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let pool = tauri::async_runtime::block_on(initialize_pool(app.handle()))?;
            if let Err(error) = tauri::async_runtime::block_on(
                search_index::ensure_search_index_health_on_startup_with_pool(&pool),
            ) {
                eprintln!("Search index health check failed during startup: {error}");
            }
            app.manage(AppState { pool });
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_library,
            create_library,
            start_import,
            start_bulk_import,
            get_import_job_result,
            start_import_retry,
            list_library_items,
            get_library_item_metadata,
            update_library_item_metadata,
            preview_batch_metadata_update,
            execute_batch_metadata_update,
            enrich_library_item_metadata,
            list_metadata_enrichment_proposals,
            apply_metadata_enrichment_proposal,
            assign_item_tags_collections,
            list_item_tags_collections,
            list_metadata_tags,
            list_metadata_collections,
            detect_metadata_conflicts,
            list_metadata_conflicts,
            resolve_metadata_conflict,
            get_index_queue_status,
            process_index_work_queue,
            retry_failed_index_work_units,
            ensure_search_index_health
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
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
    async fn rejected_create_does_not_call_directory_creation_when_library_exists() {
        let temp = tempdir().expect("temp dir should be created");
        let pool = setup_pool(temp.path().join("caudex.db")).await;
        library::run_migrations(&pool)
            .await
            .expect("migrations should apply");

        library::insert_library(
            &pool,
            "Main Library",
            "/tmp/caudex-library",
            "2026-03-05T10:30:00Z",
        )
        .await
        .expect("first insert should succeed");

        let create_calls = Arc::new(AtomicUsize::new(0));
        let create_calls_clone = Arc::clone(&create_calls);

        let result = create_library_with_pool(
            CreateLibraryInput {
                name: "Second Library".to_string(),
                path: "/tmp/another-library".to_string(),
            },
            &pool,
            move |_| {
                create_calls_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;

        assert!(
            result.is_err(),
            "create should fail when library already exists"
        );
        assert_eq!(
            create_calls.load(Ordering::SeqCst),
            0,
            "directory creation must not be attempted when singleton guard rejects create"
        );
    }

    #[tokio::test]
    async fn create_returns_sanitized_message_for_filesystem_failures() {
        let temp = tempdir().expect("temp dir should be created");
        let pool = setup_pool(temp.path().join("caudex.db")).await;
        library::run_migrations(&pool)
            .await
            .expect("migrations should apply");

        let result = create_library_with_pool(
            CreateLibraryInput {
                name: "Main Library".to_string(),
                path: "/forbidden/library".to_string(),
            },
            &pool,
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "permission denied: /forbidden/library",
                ))
            },
        )
        .await;

        assert_eq!(
            result,
            Err("Unable to prepare the selected library location.".to_string())
        );
    }
}
