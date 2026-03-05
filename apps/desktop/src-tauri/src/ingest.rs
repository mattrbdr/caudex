use lopdf::Document;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Error as SqlxError, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use walkdir::WalkDir;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartImportInput {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartBulkImportInput {
    pub root_path: String,
    pub duplicate_mode: BulkDuplicateMode,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BulkDuplicateMode {
    SkipDuplicate,
    MergeMetadata,
    ForceImport,
}

impl BulkDuplicateMode {
    fn as_str(self) -> &'static str {
        match self {
            BulkDuplicateMode::SkipDuplicate => "skip_duplicate",
            BulkDuplicateMode::MergeMetadata => "merge_metadata",
            BulkDuplicateMode::ForceImport => "force_import",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportFileStatus {
    Queued,
    Running,
    Success,
    Failed,
    Skipped,
}

impl ImportFileStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ImportFileStatus::Queued => "queued",
            ImportFileStatus::Running => "running",
            ImportFileStatus::Success => "success",
            ImportFileStatus::Failed => "failed",
            ImportFileStatus::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    Epub,
    Mobi,
    Pdf,
}

impl ImportFormat {
    fn as_str(self) -> &'static str {
        match self {
            ImportFormat::Epub => "epub",
            ImportFormat::Mobi => "mobi",
            ImportFormat::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportFileResult {
    pub source_path: String,
    pub status: ImportFileStatus,
    pub format: Option<ImportFormat>,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub item_id: Option<i64>,
    pub index_work_unit_id: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub dedupe_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportJobResult {
    pub job_id: i64,
    pub status: String,
    pub scanned_count: usize,
    pub processed_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub items: Vec<ImportFileResult>,
}

#[derive(Debug, Clone)]
struct ParsedMetadata {
    title: String,
    authors: Vec<String>,
    language: Option<String>,
    published_at: Option<String>,
}

#[derive(Default)]
struct DedupeTracker {
    seen_paths: HashSet<String>,
    seen_precheck_keys: HashSet<String>,
    seen_sizes: HashSet<u64>,
    seen_hashes_by_size: HashMap<u64, HashSet<String>>,
}

impl DedupeTracker {
    fn has_duplicate_hash(&self, size: u64, hash: &str) -> bool {
        self.seen_hashes_by_size
            .get(&size)
            .map(|hashes| hashes.contains(hash))
            .unwrap_or(false)
    }

    fn track(&mut self, path_key: String, precheck_key: String, size: u64, content_hash: String) {
        self.seen_paths.insert(path_key);
        self.seen_precheck_keys.insert(precheck_key);
        self.seen_sizes.insert(size);
        self.seen_hashes_by_size
            .entry(size)
            .or_default()
            .insert(content_hash);
    }
}

struct ImportJobOptions {
    import_mode: &'static str,
    duplicate_mode: BulkDuplicateMode,
    dry_run: bool,
    root_path: Option<String>,
    scanned_count: usize,
}

struct BulkDiscovery {
    candidates: Vec<String>,
    diagnostics: Vec<ImportFileResult>,
    scanned_count: usize,
}

fn report_internal_error(context: &str, error: &dyn Display, user_message: &str) -> String {
    eprintln!("{context}: {error}");
    user_message.to_string()
}

fn now_utc_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| {
            report_internal_error(
                "Unable to format timestamp",
                &error,
                "Unable to process import at this time.",
            )
        })
}

fn is_unique_constraint_error(error: &SqlxError) -> bool {
    match error {
        SqlxError::Database(db_error) => {
            matches!(db_error.code().as_deref(), Some("2067" | "1555"))
                || db_error.message().contains("UNIQUE constraint failed")
        }
        _ => false,
    }
}

fn path_from_input(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Imported path is empty.".to_string());
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Ok(path);
    }

    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| {
            report_internal_error(
                "Unable to resolve current directory",
                &error,
                "Unable to resolve selected path.",
            )
        })
}

fn detect_format(path: &Path, bytes: &[u8]) -> Option<ImportFormat> {
    if bytes.starts_with(b"%PDF-") {
        return Some(ImportFormat::Pdf);
    }

    if bytes.len() >= 68 && bytes[60..68] == *b"BOOKMOBI" {
        return Some(ImportFormat::Mobi);
    }

    if let Some(file_type) = infer::get(bytes) {
        match file_type.mime_type() {
            "application/pdf" => return Some(ImportFormat::Pdf),
            "application/epub+zip" => return Some(ImportFormat::Epub),
            _ => {}
        }
    }

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())?;

    match extension.as_str() {
        "epub" => Some(ImportFormat::Epub),
        "mobi" => Some(ImportFormat::Mobi),
        "pdf" => Some(ImportFormat::Pdf),
        _ => None,
    }
}

fn title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_string())
}

fn parse_pdf(path: &Path, bytes: &[u8]) -> Result<ParsedMetadata, String> {
    if !bytes.starts_with(b"%PDF-") {
        return Err("File appears to be corrupted or unreadable PDF.".to_string());
    }

    let _ = Document::load_mem(bytes);

    Ok(ParsedMetadata {
        title: title_from_path(path),
        authors: Vec::new(),
        language: None,
        published_at: None,
    })
}

fn parse_epub(path: &Path, bytes: &[u8]) -> Result<ParsedMetadata, String> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err("File appears to be corrupted or unreadable EPUB.".to_string());
    }

    Ok(ParsedMetadata {
        title: title_from_path(path),
        authors: Vec::new(),
        language: None,
        published_at: None,
    })
}

fn parse_mobi(path: &Path, bytes: &[u8]) -> Result<ParsedMetadata, String> {
    if bytes.len() < 68 || bytes[60..68] != *b"BOOKMOBI" {
        return Err("File appears to be corrupted or unreadable MOBI.".to_string());
    }

    Ok(ParsedMetadata {
        title: title_from_path(path),
        authors: Vec::new(),
        language: None,
        published_at: None,
    })
}

fn build_precheck_key(path: &Path, size: u64) -> String {
    let normalized_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let modified_seconds = std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    format!("{size}:{normalized_name}:{modified_seconds}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

fn make_failed_result(source_path: String, error_code: &str, error_message: String) -> ImportFileResult {
    ImportFileResult {
        source_path,
        status: ImportFileStatus::Failed,
        format: None,
        title: None,
        authors: Vec::new(),
        language: None,
        published_at: None,
        item_id: None,
        index_work_unit_id: None,
        error_code: Some(error_code.to_string()),
        error_message: Some(error_message),
        dedupe_decision: None,
    }
}

async fn persist_import_job_item(
    pool: &SqlitePool,
    job_id: i64,
    source_path: &str,
    format: Option<ImportFormat>,
    status: &ImportFileStatus,
    error_code: Option<&str>,
    error_message: Option<&str>,
    library_item_id: Option<i64>,
    index_work_unit_id: Option<i64>,
    precheck_key: Option<&str>,
    content_hash: Option<&str>,
    dedupe_decision: Option<&str>,
    queued_at: &str,
    completed_at: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO import_job_items (
          job_id,
          source_path,
          detected_format,
          status,
          error_code,
          error_message,
          library_item_id,
          index_work_unit_id,
          precheck_key,
          content_hash,
          dedupe_decision,
          queued_at,
          completed_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(job_id)
    .bind(source_path)
    .bind(format.map(|value| value.as_str().to_string()))
    .bind(status.as_str())
    .bind(error_code)
    .bind(error_message)
    .bind(library_item_id)
    .bind(index_work_unit_id)
    .bind(precheck_key)
    .bind(content_hash)
    .bind(dedupe_decision)
    .bind(queued_at)
    .bind(completed_at)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to persist import job item",
            &error,
            "Unable to save import results.",
        )
    })?;

    Ok(())
}

async fn create_import_job(
    pool: &SqlitePool,
    library_id: i64,
    options: &ImportJobOptions,
    started_at: &str,
) -> Result<i64, String> {
    sqlx::query(
        r#"
        INSERT INTO import_jobs (
          library_id,
          status,
          started_at,
          import_mode,
          root_path,
          duplicate_mode,
          dry_run,
          scanned_count
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(library_id)
    .bind("running")
    .bind(started_at)
    .bind(options.import_mode)
    .bind(options.root_path.as_deref())
    .bind(options.duplicate_mode.as_str())
    .bind(if options.dry_run { 1 } else { 0 })
    .bind(options.scanned_count as i64)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to create import job",
            &error,
            "Unable to start import job.",
        )
    })
    .map(|result| result.last_insert_rowid())
}

async fn finalize_import_job(
    pool: &SqlitePool,
    job_id: i64,
    status: &str,
    completed_at: &str,
    scanned_count: usize,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE import_jobs
        SET status = ?, completed_at = ?, scanned_count = ?
        WHERE id = ?
        "#,
    )
    .bind(status)
    .bind(completed_at)
    .bind(scanned_count as i64)
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to finalize import job",
            &error,
            "Unable to complete import job.",
        )
    })?;

    Ok(())
}

async fn process_one_file(
    pool: &SqlitePool,
    job_id: i64,
    library_id: i64,
    raw_path: &str,
    tracker: &mut DedupeTracker,
    duplicate_mode: BulkDuplicateMode,
    dry_run: bool,
) -> Result<ImportFileResult, String> {
    let queued_at = now_utc_rfc3339()?;
    let completed_at = now_utc_rfc3339()?;

    let resolved_path = match path_from_input(raw_path) {
        Ok(path) => path,
        Err(message) => {
            persist_import_job_item(
                pool,
                job_id,
                raw_path,
                None,
                &ImportFileStatus::Failed,
                Some("invalid_path"),
                Some(&message),
                None,
                None,
                None,
                None,
                None,
                &queued_at,
                Some(&completed_at),
            )
            .await?;

            return Ok(make_failed_result(raw_path.to_string(), "invalid_path", message));
        }
    };

    let canonical_or_absolute = resolved_path
        .canonicalize()
        .unwrap_or_else(|_| resolved_path.clone());
    let source_path = canonical_or_absolute.to_string_lossy().to_string();
    let path_key = source_path.to_ascii_lowercase();

    if !tracker.seen_paths.insert(path_key.clone()) {
        let message = "Duplicate file path in this import request.".to_string();
        persist_import_job_item(
            pool,
            job_id,
            &source_path,
            None,
            &ImportFileStatus::Skipped,
            Some("duplicate_in_request"),
            Some(&message),
            None,
            None,
            None,
            None,
            Some("skip_duplicate"),
            &queued_at,
            Some(&completed_at),
        )
        .await?;

        return Ok(ImportFileResult {
            source_path,
            status: ImportFileStatus::Skipped,
            format: None,
            title: None,
            authors: Vec::new(),
            language: None,
            published_at: None,
            item_id: None,
            index_work_unit_id: None,
            error_code: Some("duplicate_in_request".to_string()),
            error_message: Some(message),
            dedupe_decision: Some("skip_duplicate".to_string()),
        });
    }

    let file_bytes = match std::fs::read(&canonical_or_absolute) {
        Ok(bytes) => bytes,
        Err(error) => {
            let message = report_internal_error(
                "Unable to read selected file",
                &error,
                "Unable to read selected file.",
            );
            persist_import_job_item(
                pool,
                job_id,
                &source_path,
                None,
                &ImportFileStatus::Failed,
                Some("read_failed"),
                Some(&message),
                None,
                None,
                None,
                None,
                None,
                &queued_at,
                Some(&completed_at),
            )
            .await?;

            return Ok(make_failed_result(source_path, "read_failed", message));
        }
    };

    let detected_format = match detect_format(&canonical_or_absolute, &file_bytes) {
        Some(format) => format,
        None => {
            let message = "Unsupported file format.".to_string();
            persist_import_job_item(
                pool,
                job_id,
                &source_path,
                None,
                &ImportFileStatus::Failed,
                Some("unsupported_format"),
                Some(&message),
                None,
                None,
                None,
                None,
                None,
                &queued_at,
                Some(&completed_at),
            )
            .await?;

            return Ok(make_failed_result(source_path, "unsupported_format", message));
        }
    };

    let metadata = match detected_format {
        ImportFormat::Pdf => parse_pdf(&canonical_or_absolute, &file_bytes),
        ImportFormat::Epub => parse_epub(&canonical_or_absolute, &file_bytes),
        ImportFormat::Mobi => parse_mobi(&canonical_or_absolute, &file_bytes),
    };

    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(message) => {
            persist_import_job_item(
                pool,
                job_id,
                &source_path,
                Some(detected_format),
                &ImportFileStatus::Failed,
                Some("corrupt_file"),
                Some(&message),
                None,
                None,
                None,
                None,
                None,
                &queued_at,
                Some(&completed_at),
            )
            .await?;

            let mut failed = make_failed_result(source_path, "corrupt_file", message);
            failed.format = Some(detected_format);
            return Ok(failed);
        }
    };

    let size = file_bytes.len() as u64;
    let precheck_key = build_precheck_key(&canonical_or_absolute, size);
    let content_hash = sha256_hex(&file_bytes);

    let is_duplicate_candidate =
        tracker.seen_precheck_keys.contains(&precheck_key) || tracker.seen_sizes.contains(&size);
    let duplicate_content = is_duplicate_candidate && tracker.has_duplicate_hash(size, &content_hash);

    let mut dedupe_decision: Option<&str> = None;
    if duplicate_content {
        match duplicate_mode {
            BulkDuplicateMode::SkipDuplicate => {
                let message = "Duplicate content detected and skipped.".to_string();
                persist_import_job_item(
                    pool,
                    job_id,
                    &source_path,
                    Some(detected_format),
                    &ImportFileStatus::Skipped,
                    Some("duplicate_content"),
                    Some(&message),
                    None,
                    None,
                    Some(&precheck_key),
                    Some(&content_hash),
                    Some("skip_duplicate"),
                    &queued_at,
                    Some(&completed_at),
                )
                .await?;

                tracker.track(path_key, precheck_key, size, content_hash);

                return Ok(ImportFileResult {
                    source_path,
                    status: ImportFileStatus::Skipped,
                    format: Some(detected_format),
                    title: Some(metadata.title),
                    authors: metadata.authors,
                    language: metadata.language,
                    published_at: metadata.published_at,
                    item_id: None,
                    index_work_unit_id: None,
                    error_code: Some("duplicate_content".to_string()),
                    error_message: Some(message),
                    dedupe_decision: Some("skip_duplicate".to_string()),
                });
            }
            BulkDuplicateMode::MergeMetadata => {
                let message = "Duplicate content detected and metadata merge was requested.".to_string();
                persist_import_job_item(
                    pool,
                    job_id,
                    &source_path,
                    Some(detected_format),
                    &ImportFileStatus::Skipped,
                    Some("duplicate_content"),
                    Some(&message),
                    None,
                    None,
                    Some(&precheck_key),
                    Some(&content_hash),
                    Some("merge_metadata"),
                    &queued_at,
                    Some(&completed_at),
                )
                .await?;

                tracker.track(path_key, precheck_key, size, content_hash);

                return Ok(ImportFileResult {
                    source_path,
                    status: ImportFileStatus::Skipped,
                    format: Some(detected_format),
                    title: Some(metadata.title),
                    authors: metadata.authors,
                    language: metadata.language,
                    published_at: metadata.published_at,
                    item_id: None,
                    index_work_unit_id: None,
                    error_code: Some("duplicate_content".to_string()),
                    error_message: Some(message),
                    dedupe_decision: Some("merge_metadata".to_string()),
                });
            }
            BulkDuplicateMode::ForceImport => {
                dedupe_decision = Some("force_import");
            }
        }
    }

    let imported_at = now_utc_rfc3339()?;
    let author_json = serde_json::to_string(&metadata.authors).map_err(|error| {
        report_internal_error(
            "Unable to serialize author metadata",
            &error,
            "Unable to process import metadata.",
        )
    })?;

    let mut item_id = None;
    let mut index_work_unit_id = None;

    if !dry_run {
        let insert_item = sqlx::query(
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
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(library_id)
        .bind(&source_path)
        .bind(detected_format.as_str())
        .bind(&metadata.title)
        .bind(author_json)
        .bind(&metadata.language)
        .bind(&metadata.published_at)
        .bind(&imported_at)
        .execute(pool)
        .await;

        let inserted_item_id = match insert_item {
            Ok(result) => result.last_insert_rowid(),
            Err(error) if is_unique_constraint_error(&error) => {
                let message = "File already imported in this library.".to_string();
                persist_import_job_item(
                    pool,
                    job_id,
                    &source_path,
                    Some(detected_format),
                    &ImportFileStatus::Skipped,
                    Some("duplicate_existing"),
                    Some(&message),
                    None,
                    None,
                    Some(&precheck_key),
                    Some(&content_hash),
                    Some("skip_duplicate"),
                    &queued_at,
                    Some(&completed_at),
                )
                .await?;

                tracker.track(path_key, precheck_key, size, content_hash);

                return Ok(ImportFileResult {
                    source_path,
                    status: ImportFileStatus::Skipped,
                    format: Some(detected_format),
                    title: Some(metadata.title),
                    authors: metadata.authors,
                    language: metadata.language,
                    published_at: metadata.published_at,
                    item_id: None,
                    index_work_unit_id: None,
                    error_code: Some("duplicate_existing".to_string()),
                    error_message: Some(message),
                    dedupe_decision: Some("skip_duplicate".to_string()),
                });
            }
            Err(error) => {
                return Err(report_internal_error(
                    "Unable to persist imported item",
                    &error,
                    "Unable to save imported item.",
                ));
            }
        };

        let created_at = now_utc_rfc3339()?;
        let index_id = sqlx::query(
            r#"
            INSERT INTO index_work_units (
              library_item_id,
              status,
              created_at
            )
            VALUES (?, ?, ?)
            "#,
        )
        .bind(inserted_item_id)
        .bind("queued")
        .bind(&created_at)
        .execute(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to enqueue index work unit",
                &error,
                "Unable to queue index update.",
            )
        })?
        .last_insert_rowid();

        item_id = Some(inserted_item_id);
        index_work_unit_id = Some(index_id);
    }

    tracker.track(path_key, precheck_key.clone(), size, content_hash.clone());

    persist_import_job_item(
        pool,
        job_id,
        &source_path,
        Some(detected_format),
        &ImportFileStatus::Success,
        None,
        None,
        item_id,
        index_work_unit_id,
        Some(&precheck_key),
        Some(&content_hash),
        dedupe_decision,
        &queued_at,
        Some(&completed_at),
    )
    .await?;

    Ok(ImportFileResult {
        source_path,
        status: ImportFileStatus::Success,
        format: Some(detected_format),
        title: Some(metadata.title),
        authors: metadata.authors,
        language: metadata.language,
        published_at: metadata.published_at,
        item_id,
        index_work_unit_id,
        error_code: None,
        error_message: None,
        dedupe_decision: dedupe_decision.map(ToString::to_string),
    })
}

fn collect_bulk_candidates(root_path: &Path) -> Result<BulkDiscovery, String> {
    if !root_path.exists() {
        return Err("Selected folder does not exist.".to_string());
    }
    if !root_path.is_dir() {
        return Err("Selected path must be a folder.".to_string());
    }

    let mut candidates: Vec<String> = Vec::new();
    let mut diagnostics: Vec<ImportFileResult> = Vec::new();
    let mut scanned_count = 0usize;

    for entry in WalkDir::new(root_path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
    {
        match entry {
            Ok(dir_entry) => {
                let path = dir_entry.path();

                if dir_entry.file_type().is_symlink() {
                    scanned_count += 1;
                    if std::fs::metadata(path).is_err() {
                        diagnostics.push(make_failed_result(
                            path.to_string_lossy().to_string(),
                            "broken_symlink",
                            "Broken symlink encountered during bulk import scan.".to_string(),
                        ));
                    }
                    continue;
                }

                if dir_entry.file_type().is_file() {
                    scanned_count += 1;
                    candidates.push(path.to_string_lossy().to_string());
                }
            }
            Err(error) => {
                scanned_count += 1;
                let source_path = error
                    .path()
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_else(|| root_path.to_string_lossy().to_string());

                diagnostics.push(make_failed_result(
                    source_path,
                    "walk_error",
                    "Unable to access one or more paths during bulk scan.".to_string(),
                ));
            }
        }
    }

    candidates.sort();

    Ok(BulkDiscovery {
        candidates,
        diagnostics,
        scanned_count,
    })
}

async fn run_import_job(
    pool: &SqlitePool,
    paths: Vec<String>,
    mut initial_diagnostics: Vec<ImportFileResult>,
    options: ImportJobOptions,
) -> Result<ImportJobResult, String> {
    if paths.is_empty() && initial_diagnostics.is_empty() {
        return Err("No import candidates found.".to_string());
    }

    let library_id: i64 = sqlx::query_scalar("SELECT id FROM libraries ORDER BY id ASC LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to load library before import",
                &error,
                "Unable to verify library configuration.",
            )
        })?
        .ok_or_else(|| "Create a library before importing files.".to_string())?;

    let started_at = now_utc_rfc3339()?;
    let job_id = create_import_job(pool, library_id, &options, &started_at).await?;

    let mut tracker = DedupeTracker::default();
    let mut items = Vec::with_capacity(paths.len() + initial_diagnostics.len());

    for diagnostic in initial_diagnostics.drain(..) {
        let queued_at = now_utc_rfc3339()?;
        let completed_at = now_utc_rfc3339()?;
        persist_import_job_item(
            pool,
            job_id,
            &diagnostic.source_path,
            None,
            &diagnostic.status,
            diagnostic.error_code.as_deref(),
            diagnostic.error_message.as_deref(),
            None,
            None,
            None,
            None,
            None,
            &queued_at,
            Some(&completed_at),
        )
        .await?;
        items.push(diagnostic);
    }

    for raw_path in paths {
        let item = process_one_file(
            pool,
            job_id,
            library_id,
            &raw_path,
            &mut tracker,
            options.duplicate_mode,
            options.dry_run,
        )
        .await?;
        items.push(item);
    }

    let success_count = items
        .iter()
        .filter(|item| item.status == ImportFileStatus::Success)
        .count();
    let failed_count = items
        .iter()
        .filter(|item| item.status == ImportFileStatus::Failed)
        .count();
    let skipped_count = items
        .iter()
        .filter(|item| item.status == ImportFileStatus::Skipped)
        .count();
    let processed_count = items.len();

    let job_status = if success_count == 0 && failed_count > 0 {
        "failed"
    } else if failed_count > 0 {
        "partial_success"
    } else {
        "success"
    };

    let completed_at = now_utc_rfc3339()?;
    finalize_import_job(
        pool,
        job_id,
        job_status,
        &completed_at,
        options.scanned_count.max(processed_count),
    )
    .await?;

    Ok(ImportJobResult {
        job_id,
        status: job_status.to_string(),
        scanned_count: options.scanned_count.max(processed_count),
        processed_count,
        success_count,
        failed_count,
        skipped_count,
        items,
    })
}

pub async fn start_import_with_pool(
    input: StartImportInput,
    pool: &SqlitePool,
) -> Result<ImportJobResult, String> {
    if input.paths.is_empty() {
        return Err("Select at least one file to import.".to_string());
    }

    let options = ImportJobOptions {
        import_mode: "single",
        duplicate_mode: BulkDuplicateMode::SkipDuplicate,
        dry_run: false,
        root_path: None,
        scanned_count: input.paths.len(),
    };

    run_import_job(pool, input.paths, Vec::new(), options).await
}

pub async fn start_bulk_import_with_pool(
    input: StartBulkImportInput,
    pool: &SqlitePool,
) -> Result<ImportJobResult, String> {
    let root = path_from_input(&input.root_path)?;
    let root = root.canonicalize().unwrap_or(root);

    let discovery = collect_bulk_candidates(&root)?;
    let options = ImportJobOptions {
        import_mode: "bulk",
        duplicate_mode: input.duplicate_mode,
        dry_run: input.dry_run,
        root_path: Some(root.to_string_lossy().to_string()),
        scanned_count: discovery.scanned_count,
    };

    run_import_job(pool, discovery.candidates, discovery.diagnostics, options).await
}
