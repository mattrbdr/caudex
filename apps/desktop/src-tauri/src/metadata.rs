use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::collections::HashSet;
use std::fmt::Display;

fn report_internal_error(context: &str, error: &dyn Display, user_message: &str) -> String {
    eprintln!("{context}: {error}");
    user_message.to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListLibraryItemsInput {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ListLibraryItemsResult {
    pub page: u32,
    pub page_size: u32,
    pub total: i64,
    pub items: Vec<LibraryItemSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LibraryItemSummary {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub format: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetLibraryItemMetadataInput {
    pub item_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LibraryItemMetadata {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub format: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateLibraryItemMetadataInput {
    pub item_id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct LibraryItemRow {
    id: i64,
    title: String,
    authors: String,
    language: Option<String>,
    published_at: Option<String>,
    format: String,
    source_path: String,
}

fn normalize_pagination(page: Option<u32>, page_size: Option<u32>) -> (u32, u32) {
    let normalized_page = page.unwrap_or(1).max(1);
    let normalized_size = page_size.unwrap_or(50).clamp(1, 200);
    (normalized_page, normalized_size)
}

fn parse_authors_json(authors: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(authors).unwrap_or_default()
}

fn normalize_title(title: &str) -> Result<String, String> {
    let normalized = title.trim();
    if normalized.is_empty() {
        return Err("Title is required.".to_string());
    }
    Ok(normalized.to_string())
}

fn normalize_authors(authors: &[String]) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for author in authors {
        let trimmed = author.trim();
        if trimmed.is_empty() {
            continue;
        }

        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            normalized.push(trimmed.to_string());
        }
    }

    if normalized.is_empty() {
        return Err("At least one author is required.".to_string());
    }

    Ok(normalized)
}

fn is_valid_language_tag(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.is_empty() || parts.len() > 2 {
        return false;
    }

    let primary = parts[0];
    if primary.len() != 2 || !primary.chars().all(|ch| ch.is_ascii_lowercase()) {
        return false;
    }

    if parts.len() == 2 {
        let region = parts[1];
        if region.len() != 2 || !region.chars().all(|ch| ch.is_ascii_uppercase()) {
            return false;
        }
    }

    true
}

fn normalize_language(language: Option<String>) -> Result<Option<String>, String> {
    let Some(language) = language else {
        return Ok(None);
    };

    let trimmed = language.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if !is_valid_language_tag(trimmed) {
        return Err("Language must be an ISO tag like 'en' or 'en-US'.".to_string());
    }

    Ok(Some(trimmed.to_string()))
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn is_valid_iso_date(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    if parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }

    let year = match parts[0].parse::<u32>() {
        Ok(v) if v >= 1 => v,
        _ => return false,
    };
    let month = match parts[1].parse::<u32>() {
        Ok(v) if (1..=12).contains(&v) => v,
        _ => return false,
    };
    let day = match parts[2].parse::<u32>() {
        Ok(v) if v >= 1 => v,
        _ => return false,
    };

    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    };

    day <= max_day
}

fn normalize_published_at(published_at: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = published_at else {
        return Ok(None);
    };

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if !is_valid_iso_date(trimmed) {
        return Err("Published date must be in YYYY-MM-DD format.".to_string());
    }

    Ok(Some(trimmed.to_string()))
}

fn map_item_row(row: LibraryItemRow) -> LibraryItemMetadata {
    LibraryItemMetadata {
        id: row.id,
        title: row.title,
        authors: parse_authors_json(&row.authors),
        language: row.language,
        published_at: row.published_at,
        format: row.format,
        source_path: row.source_path,
    }
}

fn map_summary_row(row: LibraryItemRow) -> LibraryItemSummary {
    LibraryItemSummary {
        id: row.id,
        title: row.title,
        authors: parse_authors_json(&row.authors),
        language: row.language,
        published_at: row.published_at,
        format: row.format,
        source_path: row.source_path,
    }
}

pub async fn list_library_items_with_pool(
    input: ListLibraryItemsInput,
    pool: &SqlitePool,
) -> Result<ListLibraryItemsResult, String> {
    let (page, page_size) = normalize_pagination(input.page, input.page_size);
    let offset = i64::from((page - 1) * page_size);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM library_items")
        .fetch_one(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to count library items",
                &error,
                "Unable to load metadata items.",
            )
        })?;

    let rows = sqlx::query_as::<_, LibraryItemRow>(
        r#"
        SELECT id, title, authors, language, published_at, format, source_path
        FROM library_items
        ORDER BY id ASC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(i64::from(page_size))
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to list library items",
            &error,
            "Unable to load metadata items.",
        )
    })?;

    Ok(ListLibraryItemsResult {
        page,
        page_size,
        total,
        items: rows.into_iter().map(map_summary_row).collect(),
    })
}

pub async fn get_library_item_metadata_with_pool(
    input: GetLibraryItemMetadataInput,
    pool: &SqlitePool,
) -> Result<LibraryItemMetadata, String> {
    let row = sqlx::query_as::<_, LibraryItemRow>(
        r#"
        SELECT id, title, authors, language, published_at, format, source_path
        FROM library_items
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(input.item_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to get library item metadata",
            &error,
            "Unable to load metadata details.",
        )
    })?
    .ok_or_else(|| "Library item not found.".to_string())?;

    Ok(map_item_row(row))
}

pub async fn update_library_item_metadata_with_pool(
    input: UpdateLibraryItemMetadataInput,
    pool: &SqlitePool,
) -> Result<LibraryItemMetadata, String> {
    let title = normalize_title(&input.title)?;
    let authors = normalize_authors(&input.authors)?;
    let language = normalize_language(input.language)?;
    let published_at = normalize_published_at(input.published_at)?;
    let authors_json = serde_json::to_string(&authors).map_err(|error| {
        report_internal_error(
            "Unable to serialize authors",
            &error,
            "Unable to save metadata updates.",
        )
    })?;

    let mut tx = pool.begin().await.map_err(|error| {
        report_internal_error(
            "Unable to start metadata update transaction",
            &error,
            "Unable to save metadata updates.",
        )
    })?;

    let update_result = sqlx::query(
        r#"
        UPDATE library_items
        SET title = ?, authors = ?, language = ?, published_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&title)
    .bind(&authors_json)
    .bind(&language)
    .bind(&published_at)
    .bind(input.item_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to update library item metadata",
            &error,
            "Unable to save metadata updates.",
        )
    })?;

    if update_result.rows_affected() == 0 {
        return Err("Library item not found.".to_string());
    }

    tx.commit().await.map_err(|error| {
        report_internal_error(
            "Unable to commit metadata update transaction",
            &error,
            "Unable to save metadata updates.",
        )
    })?;

    get_library_item_metadata_with_pool(
        GetLibraryItemMetadataInput {
            item_id: input.item_id,
        },
        pool,
    )
    .await
}
