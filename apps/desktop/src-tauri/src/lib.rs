use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::fs;
use std::path::PathBuf;
use tauri::{Manager, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub mod library;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct CreateLibraryInput {
    name: String,
    path: String,
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

async fn initialize_pool(app_handle: &tauri::AppHandle) -> Result<SqlitePool, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|error| format!("Unable to resolve app data directory: {error}"))?;

    fs::create_dir_all(&app_data_dir)
        .map_err(|error| format!("Unable to create app data directory: {error}"))?;

    let db_path = app_data_dir.join("caudex.db");
    let connect_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options)
        .await
        .map_err(|error| format!("Unable to connect to sqlite database: {error}"))?;

    library::run_migrations(&pool)
        .await
        .map_err(|error| format!("Unable to apply database migrations: {error}"))?;

    Ok(pool)
}

#[tauri::command]
async fn get_library(state: State<'_, AppState>) -> Result<Option<library::Library>, String> {
    library::fetch_library(&state.pool)
        .await
        .map_err(|error| format!("Unable to load library: {error}"))
}

#[tauri::command]
async fn create_library(
    input: CreateLibraryInput,
    state: State<'_, AppState>,
) -> Result<library::Library, String> {
    let (name, path_buf) = normalize_create_input(input)?;
    fs::create_dir_all(&path_buf)
        .map_err(|error| format!("Unable to create library directory: {error}"))?;

    let canonical_path = path_buf
        .canonicalize()
        .unwrap_or(path_buf)
        .to_string_lossy()
        .to_string();

    if library::fetch_library(&state.pool)
        .await
        .map_err(|error| format!("Unable to verify existing library: {error}"))?
        .is_some()
    {
        return Err("A library is already configured for this installation.".to_string());
    }

    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("Unable to format creation timestamp: {error}"))?;

    library::insert_library(&state.pool, &name, &canonical_path, &created_at)
        .await
        .map_err(|error| format!("Unable to persist library configuration: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let pool = tauri::async_runtime::block_on(initialize_pool(app.handle()))?;
            app.manage(AppState { pool });
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_library, create_library])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
