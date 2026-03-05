use crate::metadata;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::fmt::Display;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

fn report_internal_error(context: &str, error: &dyn Display, user_message: &str) -> String {
    eprintln!("{context}: {error}");
    user_message.to_string()
}

fn now_utc_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(|error| {
        report_internal_error(
            "Unable to format timestamp",
            &error,
            "Unable to process metadata conflict workflow.",
        )
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MetadataConflictCandidateInput {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    pub language: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DetectMetadataConflictsInput {
    pub item_id: i64,
    pub candidate: MetadataConflictCandidateInput,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListMetadataConflictsInput {
    pub item_id: i64,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolveMetadataConflictInput {
    pub conflict_id: i64,
    pub resolution: String,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MetadataConflictRecord {
    pub id: i64,
    pub item_id: i64,
    pub field_name: String,
    pub current_value: String,
    pub candidate_value: String,
    pub candidate_source: String,
    pub status: String,
    pub rationale: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DetectMetadataConflictsResult {
    pub item_id: i64,
    pub conflicts: Vec<MetadataConflictRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ListMetadataConflictsResult {
    pub item_id: i64,
    pub conflicts: Vec<MetadataConflictRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolveMetadataConflictResult {
    pub conflict: MetadataConflictRecord,
    pub item: metadata::LibraryItemMetadata,
}

#[derive(Debug, Clone, FromRow)]
struct MetadataConflictRow {
    id: i64,
    library_item_id: i64,
    field_name: String,
    current_value: String,
    candidate_value: String,
    candidate_source: String,
    status: String,
    rationale: Option<String>,
    created_at: String,
    resolved_at: Option<String>,
}

impl From<MetadataConflictRow> for MetadataConflictRecord {
    fn from(value: MetadataConflictRow) -> Self {
        Self {
            id: value.id,
            item_id: value.library_item_id,
            field_name: value.field_name,
            current_value: value.current_value,
            candidate_value: value.candidate_value,
            candidate_source: value.candidate_source,
            status: value.status,
            rationale: value.rationale,
            created_at: value.created_at,
            resolved_at: value.resolved_at,
        }
    }
}

async fn insert_conflict(
    item_id: i64,
    field_name: &str,
    current_value: String,
    candidate_value: String,
    source: &str,
    pool: &SqlitePool,
) -> Result<i64, String> {
    let created_at = now_utc_rfc3339()?;
    sqlx::query(
        r#"
        INSERT INTO metadata_conflicts (
          library_item_id,
          field_name,
          current_value,
          candidate_value,
          candidate_source,
          status,
          rationale,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, 'pending', NULL, ?)
        "#,
    )
    .bind(item_id)
    .bind(field_name)
    .bind(current_value)
    .bind(candidate_value)
    .bind(source)
    .bind(created_at)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to insert metadata conflict record",
            &error,
            "Unable to persist metadata conflict.",
        )
    })
    .map(|result| result.last_insert_rowid())
}

async fn find_pending_conflict_id(
    item_id: i64,
    field_name: &str,
    current_value: &str,
    candidate_value: &str,
    source: &str,
    pool: &SqlitePool,
) -> Result<Option<i64>, String> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM metadata_conflicts
        WHERE library_item_id = ?
          AND field_name = ?
          AND current_value = ?
          AND candidate_value = ?
          AND candidate_source = ?
          AND status = 'pending'
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(item_id)
    .bind(field_name)
    .bind(current_value)
    .bind(candidate_value)
    .bind(source)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to detect duplicate metadata conflict",
            &error,
            "Unable to persist metadata conflict.",
        )
    })
}

async fn load_conflict(conflict_id: i64, pool: &SqlitePool) -> Result<MetadataConflictRow, String> {
    sqlx::query_as::<_, MetadataConflictRow>(
        r#"
        SELECT
          id,
          library_item_id,
          field_name,
          current_value,
          candidate_value,
          candidate_source,
          status,
          rationale,
          created_at,
          resolved_at
        FROM metadata_conflicts
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(conflict_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load metadata conflict",
            &error,
            "Unable to load metadata conflict.",
        )
    })?
    .ok_or_else(|| "Metadata conflict not found.".to_string())
}

pub async fn detect_metadata_conflicts_with_pool(
    input: DetectMetadataConflictsInput,
    pool: &SqlitePool,
) -> Result<DetectMetadataConflictsResult, String> {
    let source = input
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_string();
    let current = metadata::get_library_item_metadata_with_pool(
        metadata::GetLibraryItemMetadataInput {
            item_id: input.item_id,
        },
        pool,
    )
    .await?;

    let candidate_title = input
        .candidate
        .title
        .map(|value| metadata::normalize_title(&value))
        .transpose()?;
    let candidate_authors = input
        .candidate
        .authors
        .map(|values| metadata::normalize_authors(&values))
        .transpose()?;
    let candidate_language = metadata::normalize_language(input.candidate.language)?;
    let candidate_published_at = metadata::normalize_published_at(input.candidate.published_at)?;

    let mut conflict_ids = Vec::new();

    if let Some(title) = candidate_title {
        if title != current.title {
            let id = if let Some(existing) = find_pending_conflict_id(
                input.item_id,
                "title",
                &current.title,
                &title,
                &source,
                pool,
            )
            .await?
            {
                existing
            } else {
                insert_conflict(
                    input.item_id,
                    "title",
                    current.title.clone(),
                    title,
                    &source,
                    pool,
                )
                .await?
            };
            conflict_ids.push(id);
        }
    }

    if let Some(authors) = candidate_authors {
        if authors != current.authors {
            let current_authors =
                serde_json::to_string(&current.authors).unwrap_or_else(|_| "[]".to_string());
            let candidate_authors =
                serde_json::to_string(&authors).unwrap_or_else(|_| "[]".to_string());
            let id = if let Some(existing) = find_pending_conflict_id(
                input.item_id,
                "authors",
                &current_authors,
                &candidate_authors,
                &source,
                pool,
            )
            .await?
            {
                existing
            } else {
                insert_conflict(
                    input.item_id,
                    "authors",
                    current_authors,
                    candidate_authors,
                    &source,
                    pool,
                )
                .await?
            };
            conflict_ids.push(id);
        }
    }

    if let Some(language) = candidate_language {
        if Some(language.clone()) != current.language {
            let current_language = current.language.clone().unwrap_or_default();
            let id = if let Some(existing) = find_pending_conflict_id(
                input.item_id,
                "language",
                &current_language,
                &language,
                &source,
                pool,
            )
            .await?
            {
                existing
            } else {
                insert_conflict(
                    input.item_id,
                    "language",
                    current_language,
                    language,
                    &source,
                    pool,
                )
                .await?
            };
            conflict_ids.push(id);
        }
    }

    if let Some(published_at) = candidate_published_at {
        if Some(published_at.clone()) != current.published_at {
            let current_published_at = current.published_at.clone().unwrap_or_default();
            let id = if let Some(existing) = find_pending_conflict_id(
                input.item_id,
                "published_at",
                &current_published_at,
                &published_at,
                &source,
                pool,
            )
            .await?
            {
                existing
            } else {
                insert_conflict(
                    input.item_id,
                    "published_at",
                    current_published_at,
                    published_at,
                    &source,
                    pool,
                )
                .await?
            };
            conflict_ids.push(id);
        }
    }

    let mut conflicts = Vec::new();
    for conflict_id in conflict_ids {
        conflicts.push(load_conflict(conflict_id, pool).await?.into());
    }

    Ok(DetectMetadataConflictsResult {
        item_id: input.item_id,
        conflicts,
    })
}

pub async fn list_metadata_conflicts_with_pool(
    input: ListMetadataConflictsInput,
    pool: &SqlitePool,
) -> Result<ListMetadataConflictsResult, String> {
    let _ = metadata::get_library_item_metadata_with_pool(
        metadata::GetLibraryItemMetadataInput {
            item_id: input.item_id,
        },
        pool,
    )
    .await?;

    let status = input
        .status
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| !value.is_empty());

    if let Some(status) = status.as_deref() {
        if !matches!(
            status,
            "pending" | "resolved_keep_current" | "resolved_use_candidate"
        ) {
            return Err("Invalid conflict status filter.".to_string());
        }
    }

    let rows = if let Some(status) = status {
        sqlx::query_as::<_, MetadataConflictRow>(
            r#"
            SELECT
              id,
              library_item_id,
              field_name,
              current_value,
              candidate_value,
              candidate_source,
              status,
              rationale,
              created_at,
              resolved_at
            FROM metadata_conflicts
            WHERE library_item_id = ? AND status = ?
            ORDER BY id DESC
            "#,
        )
        .bind(input.item_id)
        .bind(status)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, MetadataConflictRow>(
            r#"
            SELECT
              id,
              library_item_id,
              field_name,
              current_value,
              candidate_value,
              candidate_source,
              status,
              rationale,
              created_at,
              resolved_at
            FROM metadata_conflicts
            WHERE library_item_id = ?
            ORDER BY id DESC
            "#,
        )
        .bind(input.item_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|error| {
        report_internal_error(
            "Unable to list metadata conflicts",
            &error,
            "Unable to load metadata conflicts.",
        )
    })?;

    Ok(ListMetadataConflictsResult {
        item_id: input.item_id,
        conflicts: rows.into_iter().map(Into::into).collect(),
    })
}

pub async fn resolve_metadata_conflict_with_pool(
    input: ResolveMetadataConflictInput,
    pool: &SqlitePool,
) -> Result<ResolveMetadataConflictResult, String> {
    let resolution = input.resolution.trim().to_ascii_lowercase();
    if !matches!(resolution.as_str(), "keep_current" | "use_candidate") {
        return Err("Resolution must be 'keep_current' or 'use_candidate'.".to_string());
    }

    let mut tx = pool.begin().await.map_err(|error| {
        report_internal_error(
            "Unable to start conflict resolution transaction",
            &error,
            "Unable to resolve metadata conflict.",
        )
    })?;

    let conflict = sqlx::query_as::<_, MetadataConflictRow>(
        r#"
        SELECT
          id,
          library_item_id,
          field_name,
          current_value,
          candidate_value,
          candidate_source,
          status,
          rationale,
          created_at,
          resolved_at
        FROM metadata_conflicts
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(input.conflict_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load metadata conflict",
            &error,
            "Unable to load metadata conflict.",
        )
    })?
    .ok_or_else(|| "Metadata conflict not found.".to_string())?;

    if conflict.status != "pending" {
        return Err("Only pending conflicts can be resolved.".to_string());
    }

    let item = if resolution == "use_candidate" {
        let current = metadata::get_library_item_metadata_with_tx(
            metadata::GetLibraryItemMetadataInput {
                item_id: conflict.library_item_id,
            },
            &mut tx,
        )
        .await?;

        let mut update = metadata::UpdateLibraryItemMetadataInput {
            item_id: conflict.library_item_id,
            title: current.title,
            authors: current.authors,
            language: current.language,
            published_at: current.published_at,
            tags: None,
            collections: None,
        };

        match conflict.field_name.as_str() {
            "title" => {
                update.title = conflict.candidate_value.clone();
            }
            "authors" => {
                update.authors =
                    serde_json::from_str(&conflict.candidate_value).map_err(|error| {
                        report_internal_error(
                            "Unable to parse authors candidate payload",
                            &error,
                            "Unable to apply metadata conflict resolution.",
                        )
                    })?;
            }
            "language" => {
                update.language = if conflict.candidate_value.trim().is_empty() {
                    None
                } else {
                    Some(conflict.candidate_value.clone())
                };
            }
            "published_at" => {
                update.published_at = if conflict.candidate_value.trim().is_empty() {
                    None
                } else {
                    Some(conflict.candidate_value.clone())
                };
            }
            _ => return Err("Unsupported conflict field.".to_string()),
        }

        metadata::update_library_item_metadata_with_tx(update, &mut tx).await?
    } else {
        metadata::get_library_item_metadata_with_tx(
            metadata::GetLibraryItemMetadataInput {
                item_id: conflict.library_item_id,
            },
            &mut tx,
        )
        .await?
    };

    let resolved_status = if resolution == "use_candidate" {
        "resolved_use_candidate"
    } else {
        "resolved_keep_current"
    };
    let resolved_at = now_utc_rfc3339()?;
    let rationale = input
        .rationale
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let update_resolution_result = sqlx::query(
        r#"
        UPDATE metadata_conflicts
        SET status = ?, rationale = ?, resolved_at = ?
        WHERE id = ? AND status = 'pending'
        "#,
    )
    .bind(resolved_status)
    .bind(rationale)
    .bind(resolved_at)
    .bind(input.conflict_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to resolve metadata conflict",
            &error,
            "Unable to resolve metadata conflict.",
        )
    })?;

    if update_resolution_result.rows_affected() != 1 {
        return Err("Metadata conflict is no longer pending.".to_string());
    }

    tx.commit().await.map_err(|error| {
        report_internal_error(
            "Unable to commit conflict resolution transaction",
            &error,
            "Unable to resolve metadata conflict.",
        )
    })?;

    let refreshed = load_conflict(input.conflict_id, pool).await?;

    Ok(ResolveMetadataConflictResult {
        conflict: refreshed.into(),
        item,
    })
}
