use lopdf::Document;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Error as SqlxError, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartImportInput {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetImportJobResultInput {
    pub job_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartBulkImportInput {
    pub root_path: String,
    pub duplicate_mode: BulkDuplicateMode,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartImportRetryInput {
    pub job_id: i64,
    pub source_paths: Option<Vec<String>>,
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

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
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

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "epub" => Some(Self::Epub),
            "mobi" => Some(Self::Mobi),
            "pdf" => Some(Self::Pdf),
            _ => None,
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
    seen_paths_by_size: HashMap<u64, Vec<String>>,
    computed_hashes_by_path: HashMap<String, String>,
}

impl DedupeTracker {
    fn has_duplicate_hash(&self, size: u64, hash: &str) -> bool {
        self.seen_hashes_by_size
            .get(&size)
            .map(|hashes| hashes.contains(hash))
            .unwrap_or(false)
    }

    fn ensure_hashes_for_size(&mut self, size: u64) {
        let Some(paths) = self.seen_paths_by_size.get(&size).cloned() else {
            return;
        };

        for source_path in paths {
            if self.computed_hashes_by_path.contains_key(&source_path) {
                continue;
            }

            let bytes = match std::fs::read(&source_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("Unable to read prior file for dedupe hash precheck: {error}");
                    continue;
                }
            };

            let hash = sha256_hex(&bytes);
            self.computed_hashes_by_path
                .insert(source_path.clone(), hash.clone());
            self.seen_hashes_by_size
                .entry(size)
                .or_default()
                .insert(hash);
        }
    }

    fn track(
        &mut self,
        path_key: String,
        precheck_key: String,
        size: u64,
        source_path: String,
        content_hash: Option<String>,
    ) {
        self.seen_paths.insert(path_key);
        self.seen_precheck_keys.insert(precheck_key);
        self.seen_sizes.insert(size);
        self.seen_paths_by_size
            .entry(size)
            .or_default()
            .push(source_path.clone());

        if let Some(hash) = content_hash {
            self.computed_hashes_by_path
                .insert(source_path, hash.clone());
            self.seen_hashes_by_size
                .entry(size)
                .or_default()
                .insert(hash);
        }
    }
}

struct ImportJobOptions {
    import_mode: &'static str,
    duplicate_mode: BulkDuplicateMode,
    dry_run: bool,
    root_path: Option<String>,
    scanned_count: usize,
    retry_source_job_id: Option<i64>,
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
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(|error| {
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

    if Document::load_mem(bytes).is_err() {
        // Some producer outputs still carry broken xref tables; keep a strict fallback
        // that requires core PDF markers before treating the document as readable.
        let has_xref = bytes.windows(4).any(|window| window == b"xref");
        let has_eof = bytes.windows(5).any(|window| window == b"%%EOF");
        if !has_xref || !has_eof {
            return Err("File appears to be corrupted or unreadable PDF.".to_string());
        }
    }

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

    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "File appears to be corrupted or unreadable EPUB.".to_string())?;
    let mut is_epub_archive = false;

    if let Ok(mut mimetype) = archive.by_name("mimetype") {
        let mut mime_value = String::new();
        if mimetype.read_to_string(&mut mime_value).is_ok()
            && mime_value.trim() == "application/epub+zip"
        {
            is_epub_archive = true;
        }
    }

    if !is_epub_archive && archive.by_name("META-INF/container.xml").is_ok() {
        is_epub_archive = true;
    }

    if !is_epub_archive {
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

#[derive(sqlx::FromRow)]
struct LibraryItemMetadataRow {
    id: i64,
    title: String,
    authors: String,
    language: Option<String>,
    published_at: Option<String>,
}

fn is_placeholder_title(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("untitled")
}

fn merge_title(existing: &str, incoming: &str) -> String {
    if is_placeholder_title(existing) && !is_placeholder_title(incoming) {
        return incoming.trim().to_string();
    }

    if !is_placeholder_title(incoming) && incoming.trim().len() > existing.trim().len() {
        return incoming.trim().to_string();
    }

    existing.to_string()
}

fn merge_authors(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut merged = Vec::new();

    for author in existing.iter().chain(incoming.iter()) {
        let normalized = author.trim();
        if normalized.is_empty() {
            continue;
        }
        if merged
            .iter()
            .any(|value: &String| value.eq_ignore_ascii_case(normalized))
        {
            continue;
        }
        merged.push(normalized.to_string());
    }

    merged
}

async fn find_existing_item_id_by_content_hash(
    pool: &SqlitePool,
    library_id: i64,
    content_hash: &str,
) -> Result<Option<i64>, String> {
    sqlx::query_scalar(
        r#"
        SELECT iji.library_item_id
        FROM import_job_items iji
        JOIN library_items li ON li.id = iji.library_item_id
        WHERE li.library_id = ?
          AND iji.content_hash = ?
          AND iji.library_item_id IS NOT NULL
        ORDER BY iji.id DESC
        LIMIT 1
        "#,
    )
    .bind(library_id)
    .bind(content_hash)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to resolve duplicate content target",
            &error,
            "Unable to process duplicate metadata merge.",
        )
    })
}

async fn find_existing_item_by_source_path(
    pool: &SqlitePool,
    library_id: i64,
    source_path: &str,
) -> Result<Option<i64>, String> {
    sqlx::query_scalar(
        r#"
        SELECT id
        FROM library_items
        WHERE library_id = ? AND source_path = ?
        LIMIT 1
        "#,
    )
    .bind(library_id)
    .bind(source_path)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to resolve duplicate source path target",
            &error,
            "Unable to process duplicate metadata merge.",
        )
    })
}

async fn merge_library_item_metadata(
    pool: &SqlitePool,
    library_id: i64,
    item_id: i64,
    incoming: &ParsedMetadata,
) -> Result<bool, String> {
    let current = sqlx::query_as::<_, LibraryItemMetadataRow>(
        r#"
        SELECT id, title, authors, language, published_at
        FROM library_items
        WHERE id = ? AND library_id = ?
        LIMIT 1
        "#,
    )
    .bind(item_id)
    .bind(library_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load library item for metadata merge",
            &error,
            "Unable to process duplicate metadata merge.",
        )
    })?;

    let Some(current) = current else {
        return Ok(false);
    };

    let existing_authors: Vec<String> = serde_json::from_str(&current.authors).unwrap_or_default();
    let merged_title = merge_title(&current.title, &incoming.title);
    let merged_authors = merge_authors(&existing_authors, &incoming.authors);
    let merged_language = current
        .language
        .clone()
        .or_else(|| incoming.language.clone());
    let merged_published_at = current
        .published_at
        .clone()
        .or_else(|| incoming.published_at.clone());

    let merged_authors_json = serde_json::to_string(&merged_authors).map_err(|error| {
        report_internal_error(
            "Unable to serialize merged author metadata",
            &error,
            "Unable to process duplicate metadata merge.",
        )
    })?;

    let has_changes = merged_title != current.title
        || merged_authors_json != current.authors
        || merged_language != current.language
        || merged_published_at != current.published_at;

    if !has_changes {
        return Ok(false);
    }

    sqlx::query(
        r#"
        UPDATE library_items
        SET title = ?, authors = ?, language = ?, published_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&merged_title)
    .bind(&merged_authors_json)
    .bind(&merged_language)
    .bind(&merged_published_at)
    .bind(current.id)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to merge duplicate metadata into existing item",
            &error,
            "Unable to process duplicate metadata merge.",
        )
    })?;

    Ok(true)
}

fn make_failed_result(
    source_path: String,
    error_code: &str,
    error_message: String,
) -> ImportFileResult {
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
          scanned_count,
          retry_source_job_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(options.retry_source_job_id)
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

            return Ok(make_failed_result(
                raw_path.to_string(),
                "invalid_path",
                message,
            ));
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

            return Ok(make_failed_result(
                source_path,
                "unsupported_format",
                message,
            ));
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
    let is_duplicate_candidate =
        tracker.seen_precheck_keys.contains(&precheck_key) || tracker.seen_sizes.contains(&size);
    let mut content_hash: Option<String> = None;
    let mut duplicate_content = false;

    if is_duplicate_candidate || duplicate_mode == BulkDuplicateMode::MergeMetadata {
        // Precheck keeps hashing focused on likely duplicates; prior same-size files are hashed lazily.
        tracker.ensure_hashes_for_size(size);
        let current_hash = sha256_hex(&file_bytes);
        duplicate_content = tracker.has_duplicate_hash(size, &current_hash);
        content_hash = Some(current_hash);
    }

    let mut dedupe_decision: Option<&str> = None;
    if duplicate_content {
        let content_hash_ref = content_hash.as_deref();
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
                    content_hash_ref,
                    Some("skip_duplicate"),
                    &queued_at,
                    Some(&completed_at),
                )
                .await?;

                tracker.track(
                    path_key,
                    precheck_key,
                    size,
                    source_path.clone(),
                    content_hash,
                );

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
                if dry_run {
                    let message =
                        "Duplicate content detected; metadata merge skipped in dry-run mode."
                            .to_string();
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
                        content_hash_ref,
                        Some("merge_metadata"),
                        &queued_at,
                        Some(&completed_at),
                    )
                    .await?;

                    tracker.track(
                        path_key,
                        precheck_key,
                        size,
                        source_path.clone(),
                        content_hash,
                    );

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

                let existing_item_id = if let Some(hash) = content_hash_ref {
                    find_existing_item_id_by_content_hash(pool, library_id, hash).await?
                } else {
                    None
                };

                if let Some(existing_item_id) = existing_item_id {
                    let _ =
                        merge_library_item_metadata(pool, library_id, existing_item_id, &metadata)
                            .await?;
                    persist_import_job_item(
                        pool,
                        job_id,
                        &source_path,
                        Some(detected_format),
                        &ImportFileStatus::Success,
                        None,
                        None,
                        Some(existing_item_id),
                        None,
                        Some(&precheck_key),
                        content_hash_ref,
                        Some("merge_metadata"),
                        &queued_at,
                        Some(&completed_at),
                    )
                    .await?;

                    tracker.track(
                        path_key,
                        precheck_key,
                        size,
                        source_path.clone(),
                        content_hash,
                    );

                    return Ok(ImportFileResult {
                        source_path,
                        status: ImportFileStatus::Success,
                        format: Some(detected_format),
                        title: Some(metadata.title),
                        authors: metadata.authors,
                        language: metadata.language,
                        published_at: metadata.published_at,
                        item_id: Some(existing_item_id),
                        index_work_unit_id: None,
                        error_code: None,
                        error_message: None,
                        dedupe_decision: Some("merge_metadata".to_string()),
                    });
                }

                let message =
                    "Duplicate content detected but no existing item was found for metadata merge."
                        .to_string();
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
                    content_hash_ref,
                    Some("merge_metadata"),
                    &queued_at,
                    Some(&completed_at),
                )
                .await?;

                tracker.track(
                    path_key,
                    precheck_key,
                    size,
                    source_path.clone(),
                    content_hash,
                );

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
                if duplicate_mode == BulkDuplicateMode::MergeMetadata {
                    if let Some(existing_item_id) =
                        find_existing_item_by_source_path(pool, library_id, &source_path).await?
                    {
                        let _ = merge_library_item_metadata(
                            pool,
                            library_id,
                            existing_item_id,
                            &metadata,
                        )
                        .await?;
                        persist_import_job_item(
                            pool,
                            job_id,
                            &source_path,
                            Some(detected_format),
                            &ImportFileStatus::Success,
                            None,
                            None,
                            Some(existing_item_id),
                            None,
                            Some(&precheck_key),
                            content_hash.as_deref(),
                            Some("merge_metadata"),
                            &queued_at,
                            Some(&completed_at),
                        )
                        .await?;

                        tracker.track(
                            path_key,
                            precheck_key,
                            size,
                            source_path.clone(),
                            content_hash,
                        );

                        return Ok(ImportFileResult {
                            source_path,
                            status: ImportFileStatus::Success,
                            format: Some(detected_format),
                            title: Some(metadata.title),
                            authors: metadata.authors,
                            language: metadata.language,
                            published_at: metadata.published_at,
                            item_id: Some(existing_item_id),
                            index_work_unit_id: None,
                            error_code: None,
                            error_message: None,
                            dedupe_decision: Some("merge_metadata".to_string()),
                        });
                    }
                }

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
                    content_hash.as_deref(),
                    Some("skip_duplicate"),
                    &queued_at,
                    Some(&completed_at),
                )
                .await?;

                tracker.track(
                    path_key,
                    precheck_key,
                    size,
                    source_path.clone(),
                    content_hash,
                );

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

    tracker.track(
        path_key,
        precheck_key.clone(),
        size,
        source_path.clone(),
        content_hash.clone(),
    );

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
        content_hash.as_deref(),
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
        retry_source_job_id: None,
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
        retry_source_job_id: None,
    };

    run_import_job(pool, discovery.candidates, discovery.diagnostics, options).await
}

#[derive(sqlx::FromRow)]
struct ImportJobRow {
    id: i64,
    status: String,
    scanned_count: i64,
}

#[derive(sqlx::FromRow)]
struct ImportJobItemRow {
    source_path: String,
    detected_format: Option<String>,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
    library_item_id: Option<i64>,
    index_work_unit_id: Option<i64>,
    dedupe_decision: Option<String>,
    title: Option<String>,
    authors: Option<String>,
    language: Option<String>,
    published_at: Option<String>,
}

fn retry_path_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = PathBuf::from(trimmed);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let canonical = absolute.canonicalize().unwrap_or(absolute);
    Some(canonical.to_string_lossy().to_ascii_lowercase())
}

pub async fn get_import_job_result_with_pool(
    job_id: Option<i64>,
    pool: &SqlitePool,
) -> Result<ImportJobResult, String> {
    let job = if let Some(id) = job_id {
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            SELECT id, status, scanned_count
            FROM import_jobs
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to load import job",
                &error,
                "Unable to load import job.",
            )
        })?
    } else {
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            SELECT id, status, scanned_count
            FROM import_jobs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to load latest import job",
                &error,
                "Unable to load import job.",
            )
        })?
    }
    .ok_or_else(|| "Import job not found.".to_string())?;

    let item_rows = sqlx::query_as::<_, ImportJobItemRow>(
        r#"
        SELECT
          i.source_path,
          i.detected_format,
          i.status,
          i.error_code,
          i.error_message,
          i.library_item_id,
          i.index_work_unit_id,
          i.dedupe_decision,
          li.title,
          li.authors,
          li.language,
          li.published_at
        FROM import_job_items i
        LEFT JOIN library_items li ON li.id = i.library_item_id
        WHERE i.job_id = ?
        ORDER BY i.id ASC
        "#,
    )
    .bind(job.id)
    .fetch_all(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load import job items",
            &error,
            "Unable to load import results.",
        )
    })?;

    let mut items: Vec<ImportFileResult> = Vec::with_capacity(item_rows.len());
    for row in item_rows {
        let status = ImportFileStatus::from_str(&row.status).ok_or_else(|| {
            report_internal_error(
                "Invalid import job item status value",
                &row.status,
                "Unable to load import results.",
            )
        })?;
        let format = match row.detected_format.as_deref() {
            None => None,
            Some(raw) => Some(ImportFormat::from_str(raw).ok_or_else(|| {
                report_internal_error(
                    "Invalid import job item format value",
                    &raw,
                    "Unable to load import results.",
                )
            })?),
        };
        let authors = row
            .authors
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
            .unwrap_or_default();

        items.push(ImportFileResult {
            source_path: row.source_path,
            status,
            format,
            title: row.title,
            authors,
            language: row.language,
            published_at: row.published_at,
            item_id: row.library_item_id,
            index_work_unit_id: row.index_work_unit_id,
            error_code: row.error_code,
            error_message: row.error_message,
            dedupe_decision: row.dedupe_decision,
        });
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

    Ok(ImportJobResult {
        job_id: job.id,
        status: job.status,
        scanned_count: usize::try_from(job.scanned_count).unwrap_or(processed_count),
        processed_count,
        success_count,
        failed_count,
        skipped_count,
        items,
    })
}

pub async fn start_import_retry_with_pool(
    input: StartImportRetryInput,
    pool: &SqlitePool,
) -> Result<ImportJobResult, String> {
    let job_exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM import_jobs
        WHERE id = ?
        "#,
    )
    .bind(input.job_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to validate source import job",
            &error,
            "Unable to start retry.",
        )
    })?;
    if job_exists.is_none() {
        return Err("Import job not found.".to_string());
    }

    let failed_paths: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT source_path
        FROM import_job_items
        WHERE job_id = ? AND status = 'failed'
        ORDER BY id ASC
        "#,
    )
    .bind(input.job_id)
    .fetch_all(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load failed candidates for retry",
            &error,
            "Unable to start retry.",
        )
    })?;

    if failed_paths.is_empty() {
        return Err("No failed items found for retry.".to_string());
    }

    let mut failed_by_key: HashMap<String, String> = HashMap::new();
    for path in failed_paths {
        if let Some(key) = retry_path_key(&path) {
            failed_by_key.entry(key).or_insert(path);
        }
    }
    if failed_by_key.is_empty() {
        return Err("No failed items found for retry.".to_string());
    }

    let mut selected_keys_seen: HashSet<String> = HashSet::new();
    let unique_failed_paths: Vec<String> = if let Some(selected_paths) = input.source_paths.as_ref() {
        let mut retry_paths: Vec<String> = Vec::new();
        for selected in selected_paths {
            let selected_key = retry_path_key(selected)
                .ok_or_else(|| "Retry selection contains invalid paths.".to_string())?;
            let source_path = failed_by_key.get(&selected_key).ok_or_else(|| {
                "Retry selection contains items that are not failed in source job.".to_string()
            })?;
            if selected_keys_seen.insert(selected_key) {
                retry_paths.push(source_path.clone());
            }
        }
        retry_paths
    } else {
        failed_by_key.into_values().collect()
    };

    if unique_failed_paths.is_empty() {
        return Err("No failed items matched retry selection.".to_string());
    }

    let options = ImportJobOptions {
        import_mode: "retry",
        duplicate_mode: BulkDuplicateMode::SkipDuplicate,
        dry_run: false,
        root_path: None,
        scanned_count: unique_failed_paths.len(),
        retry_source_job_id: Some(input.job_id),
    };

    run_import_job(pool, unique_failed_paths, Vec::new(), options).await
}
