use crate::metadata_collections;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};
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
    pub author: Option<String>,
    pub language: Option<String>,
    pub published_from: Option<String>,
    pub published_to: Option<String>,
    pub tag: Option<String>,
    pub collection: Option<String>,
    pub sort_by: Option<String>,
    pub sort_direction: Option<String>,
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
    pub tags: Vec<String>,
    pub collections: Vec<String>,
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
    pub tags: Vec<String>,
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateLibraryItemMetadataInput {
    pub item_id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub collections: Option<Vec<String>>,
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

struct NormalizedListFilters {
    author: Option<String>,
    language: Option<String>,
    published_from: Option<String>,
    published_to: Option<String>,
    tag: Option<String>,
    collection: Option<String>,
    sort_expression: &'static str,
    sort_desc: bool,
}

fn normalize_pagination(page: Option<u32>, page_size: Option<u32>) -> (u32, u32) {
    let normalized_page = page.unwrap_or(1).max(1);
    let normalized_size = page_size.unwrap_or(50).clamp(1, 200);
    (normalized_page, normalized_size)
}

fn normalize_optional_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|candidate| {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn parse_authors_json(authors: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(authors).unwrap_or_default()
}

pub(crate) fn normalize_title(title: &str) -> Result<String, String> {
    let normalized = title.trim();
    if normalized.is_empty() {
        return Err("Title is required.".to_string());
    }
    Ok(normalized.to_string())
}

pub(crate) fn normalize_authors(authors: &[String]) -> Result<Vec<String>, String> {
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

pub(crate) fn normalize_language(language: Option<String>) -> Result<Option<String>, String> {
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

pub(crate) fn normalize_published_at(
    published_at: Option<String>,
) -> Result<Option<String>, String> {
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

fn normalize_list_filters(input: &ListLibraryItemsInput) -> Result<NormalizedListFilters, String> {
    let author = normalize_optional_trimmed(input.author.clone());
    let language = normalize_language(input.language.clone())?;
    let published_from = normalize_published_at(input.published_from.clone())?;
    let published_to = normalize_published_at(input.published_to.clone())?;
    let tag = normalize_optional_trimmed(input.tag.clone());
    let collection = normalize_optional_trimmed(input.collection.clone());

    if let (Some(from), Some(to)) = (&published_from, &published_to) {
        if from > to {
            return Err("Published date range is invalid.".to_string());
        }
    }

    let sort_expression = match input.sort_by.as_deref().map(str::to_ascii_lowercase) {
        Some(value) if value == "title" => "LOWER(li.title)",
        Some(value) if value == "author" => "LOWER(COALESCE(json_extract(li.authors, '$[0]'), ''))",
        Some(value) if value == "language" => "LOWER(COALESCE(li.language, ''))",
        Some(value) if value == "published_at" => "COALESCE(li.published_at, '')",
        _ => "li.id",
    };

    let sort_desc = matches!(
        input
            .sort_direction
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("desc")
    );

    Ok(NormalizedListFilters {
        author,
        language,
        published_from,
        published_to,
        tag,
        collection,
        sort_expression,
        sort_desc,
    })
}

fn append_filter_clauses<'a>(
    builder: &mut QueryBuilder<'a, Sqlite>,
    filters: &'a NormalizedListFilters,
) {
    if let Some(author) = &filters.author {
        builder.push(
            " AND EXISTS (SELECT 1 FROM json_each(li.authors) author WHERE LOWER(TRIM(author.value)) = LOWER(",
        );
        builder.push_bind(author);
        builder.push("))");
    }

    if let Some(language) = &filters.language {
        builder.push(" AND LOWER(li.language) = LOWER(");
        builder.push_bind(language);
        builder.push(")");
    }

    if let Some(published_from) = &filters.published_from {
        builder.push(" AND li.published_at >= ");
        builder.push_bind(published_from);
    }

    if let Some(published_to) = &filters.published_to {
        builder.push(" AND li.published_at <= ");
        builder.push_bind(published_to);
    }

    if let Some(tag) = &filters.tag {
        builder.push(
            " AND EXISTS (SELECT 1 FROM item_tags item_tag JOIN tags tag ON tag.id = item_tag.tag_id WHERE item_tag.library_item_id = li.id AND LOWER(tag.name) = LOWER(",
        );
        builder.push_bind(tag);
        builder.push("))");
    }

    if let Some(collection) = &filters.collection {
        builder.push(
            " AND EXISTS (SELECT 1 FROM item_collections item_collection JOIN collections collection ON collection.id = item_collection.collection_id WHERE item_collection.library_item_id = li.id AND LOWER(collection.name) = LOWER(",
        );
        builder.push_bind(collection);
        builder.push("))");
    }
}

fn map_item_row(
    row: LibraryItemRow,
    tags: Vec<String>,
    collections: Vec<String>,
) -> LibraryItemMetadata {
    LibraryItemMetadata {
        id: row.id,
        title: row.title,
        authors: parse_authors_json(&row.authors),
        language: row.language,
        published_at: row.published_at,
        format: row.format,
        source_path: row.source_path,
        tags,
        collections,
    }
}

fn map_summary_row(
    row: LibraryItemRow,
    tags: Vec<String>,
    collections: Vec<String>,
) -> LibraryItemSummary {
    LibraryItemSummary {
        id: row.id,
        title: row.title,
        authors: parse_authors_json(&row.authors),
        language: row.language,
        published_at: row.published_at,
        format: row.format,
        source_path: row.source_path,
        tags,
        collections,
    }
}

pub async fn list_library_items_with_pool(
    input: ListLibraryItemsInput,
    pool: &SqlitePool,
) -> Result<ListLibraryItemsResult, String> {
    let (page, page_size) = normalize_pagination(input.page, input.page_size);
    let offset = i64::from((page - 1) * page_size);
    let filters = normalize_list_filters(&input)?;

    let mut count_query =
        QueryBuilder::<Sqlite>::new("SELECT COUNT(DISTINCT li.id) FROM library_items li WHERE 1=1");
    append_filter_clauses(&mut count_query, &filters);

    let total = count_query
        .build_query_scalar::<i64>()
        .fetch_one(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to count library items",
                &error,
                "Unable to load metadata items.",
            )
        })?;

    let mut rows_query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT DISTINCT li.id, li.title, li.authors, li.language, li.published_at, li.format, li.source_path
        FROM library_items li
        WHERE 1=1
        "#,
    );
    append_filter_clauses(&mut rows_query, &filters);
    rows_query.push(" ORDER BY ");
    rows_query.push(filters.sort_expression);
    rows_query.push(if filters.sort_desc { " DESC" } else { " ASC" });
    rows_query.push(", li.id ASC");
    rows_query.push(" LIMIT ");
    rows_query.push_bind(i64::from(page_size));
    rows_query.push(" OFFSET ");
    rows_query.push_bind(offset);

    let rows = rows_query
        .build_query_as::<LibraryItemRow>()
        .fetch_all(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to list library items",
                &error,
                "Unable to load metadata items.",
            )
        })?;

    let item_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let tags_map = metadata_collections::load_tags_map(&item_ids, pool).await?;
    let collections_map = metadata_collections::load_collections_map(&item_ids, pool).await?;

    let items = rows
        .into_iter()
        .map(|row| {
            let tags = tags_map.get(&row.id).cloned().unwrap_or_default();
            let collections = collections_map.get(&row.id).cloned().unwrap_or_default();
            map_summary_row(row, tags, collections)
        })
        .collect::<Vec<_>>();

    Ok(ListLibraryItemsResult {
        page,
        page_size,
        total,
        items,
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

    let tags_map = metadata_collections::load_tags_map(&[row.id], pool).await?;
    let collections_map = metadata_collections::load_collections_map(&[row.id], pool).await?;

    Ok(map_item_row(
        row,
        tags_map.get(&input.item_id).cloned().unwrap_or_default(),
        collections_map
            .get(&input.item_id)
            .cloned()
            .unwrap_or_default(),
    ))
}

pub(crate) async fn get_library_item_metadata_with_tx(
    input: GetLibraryItemMetadataInput,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
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
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to get library item metadata",
            &error,
            "Unable to load metadata details.",
        )
    })?
    .ok_or_else(|| "Library item not found.".to_string())?;

    let tags_map = metadata_collections::load_tags_map_with_tx(&[row.id], tx).await?;
    let collections_map = metadata_collections::load_collections_map_with_tx(&[row.id], tx).await?;

    Ok(map_item_row(
        row,
        tags_map.get(&input.item_id).cloned().unwrap_or_default(),
        collections_map
            .get(&input.item_id)
            .cloned()
            .unwrap_or_default(),
    ))
}

pub(crate) async fn update_library_item_metadata_with_tx(
    input: UpdateLibraryItemMetadataInput,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<LibraryItemMetadata, String> {
    let title = normalize_title(&input.title)?;
    let authors = normalize_authors(&input.authors)?;
    let language = normalize_language(input.language)?;
    let published_at = normalize_published_at(input.published_at)?;
    let tags = input
        .tags
        .as_ref()
        .map(|values| metadata_collections::normalize_labels(values, "Tag"))
        .transpose()?;
    let collections = input
        .collections
        .as_ref()
        .map(|values| metadata_collections::normalize_labels(values, "Collection"))
        .transpose()?;

    let authors_json = serde_json::to_string(&authors).map_err(|error| {
        report_internal_error(
            "Unable to serialize authors",
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
    .execute(&mut **tx)
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

    if let Some(tags) = &tags {
        metadata_collections::replace_item_tags_with_tx(input.item_id, tags, tx).await?;
    }
    if let Some(collections) = &collections {
        metadata_collections::replace_item_collections_with_tx(input.item_id, collections, tx)
            .await?;
    }

    get_library_item_metadata_with_tx(
        GetLibraryItemMetadataInput {
            item_id: input.item_id,
        },
        tx,
    )
    .await
}

pub async fn update_library_item_metadata_with_pool(
    input: UpdateLibraryItemMetadataInput,
    pool: &SqlitePool,
) -> Result<LibraryItemMetadata, String> {
    let mut tx = pool.begin().await.map_err(|error| {
        report_internal_error(
            "Unable to start metadata update transaction",
            &error,
            "Unable to save metadata updates.",
        )
    })?;

    let updated = update_library_item_metadata_with_tx(input, &mut tx).await?;

    tx.commit().await.map_err(|error| {
        report_internal_error(
            "Unable to commit metadata update transaction",
            &error,
            "Unable to save metadata updates.",
        )
    })?;

    Ok(updated)
}
