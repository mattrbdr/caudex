use crate::metadata;
use crate::metadata_collections;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool};
use std::fmt::Display;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

fn report_internal_error(context: &str, error: &dyn Display, user_message: &str) -> String {
    eprintln!("{context}: {error}");
    user_message.to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchMetadataPatchInput {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub tags: Option<Vec<String>>,
    pub collections: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchMetadataUpdateInput {
    pub item_ids: Vec<i64>,
    pub patch: BatchMetadataPatchInput,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchMetadataOutcome {
    pub item_id: i64,
    pub status: String,
    pub reason: Option<String>,
    pub retry_eligible: bool,
    pub before: Option<metadata::LibraryItemMetadata>,
    pub after: Option<metadata::LibraryItemMetadata>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchMetadataRunResult {
    pub run_id: String,
    pub mode: String,
    pub status: String,
    pub total_targets: usize,
    pub updated_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub outcomes: Vec<BatchMetadataOutcome>,
}

#[derive(Debug, Clone, Serialize)]
struct NormalizedBatchPatch {
    title: Option<String>,
    authors: Option<Vec<String>>,
    language: Option<String>,
    published_at: Option<String>,
    tags: Option<Vec<String>>,
    collections: Option<Vec<String>>,
}

fn now_utc_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(|error| {
        report_internal_error(
            "Unable to format timestamp",
            &error,
            "Unable to process metadata batch update.",
        )
    })
}

fn normalize_patch(input: BatchMetadataPatchInput) -> Result<NormalizedBatchPatch, String> {
    let patch = NormalizedBatchPatch {
        title: input
            .title
            .map(|value| metadata::normalize_title(&value))
            .transpose()?,
        authors: input
            .authors
            .map(|values| metadata::normalize_authors(&values))
            .transpose()?,
        language: metadata::normalize_language(input.language)?,
        published_at: metadata::normalize_published_at(input.published_at)?,
        tags: input
            .tags
            .map(|values| metadata_collections::normalize_labels(&values, "Tag"))
            .transpose()?,
        collections: input
            .collections
            .map(|values| metadata_collections::normalize_labels(&values, "Collection"))
            .transpose()?,
    };

    if patch.title.is_none()
        && patch.authors.is_none()
        && patch.language.is_none()
        && patch.published_at.is_none()
        && patch.tags.is_none()
        && patch.collections.is_none()
    {
        return Err("Batch patch must include at least one field.".to_string());
    }

    Ok(patch)
}

fn normalize_item_ids(item_ids: Vec<i64>) -> Result<Vec<i64>, String> {
    let mut ids = item_ids
        .into_iter()
        .filter(|item_id| *item_id > 0)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();

    if ids.is_empty() {
        return Err("Batch update requires at least one valid item id.".to_string());
    }

    Ok(ids)
}

fn deterministic_run_id(
    mode: &str,
    item_ids: &[i64],
    patch: &NormalizedBatchPatch,
) -> Result<String, String> {
    let payload = serde_json::json!({
        "mode": mode,
        "item_ids": item_ids,
        "patch": patch
    });
    let serialized = serde_json::to_string(&payload).map_err(|error| {
        report_internal_error(
            "Unable to serialize deterministic run payload",
            &error,
            "Unable to process metadata batch update.",
        )
    })?;

    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let digest = hasher.finalize();
    let digest_hex = format!("{digest:x}");
    Ok(format!("batch-{}", &digest_hex[..16]))
}

fn apply_patch_preview(
    current: &metadata::LibraryItemMetadata,
    patch: &NormalizedBatchPatch,
) -> (metadata::LibraryItemMetadata, bool) {
    let mut after = current.clone();

    if let Some(value) = &patch.title {
        after.title = value.clone();
    }
    if let Some(value) = &patch.authors {
        after.authors = value.clone();
    }
    if let Some(value) = &patch.language {
        after.language = Some(value.clone());
    }
    if let Some(value) = &patch.published_at {
        after.published_at = Some(value.clone());
    }
    if let Some(value) = &patch.tags {
        after.tags = value.clone();
    }
    if let Some(value) = &patch.collections {
        after.collections = value.clone();
    }

    let changed = after.title != current.title
        || after.authors != current.authors
        || after.language != current.language
        || after.published_at != current.published_at
        || after.tags != current.tags
        || after.collections != current.collections;

    (after, changed)
}

async fn persist_batch_run_with_executor<'a, E>(
    run_id: &str,
    mode: &str,
    status: &str,
    item_ids: &[i64],
    patch: &NormalizedBatchPatch,
    executor: E,
) -> Result<i64, String>
where
    E: sqlx::Executor<'a, Database = Sqlite>,
{
    let created_at = now_utc_rfc3339()?;
    let target_scope = serde_json::to_string(item_ids).map_err(|error| {
        report_internal_error(
            "Unable to serialize batch target scope",
            &error,
            "Unable to save batch metadata diagnostics.",
        )
    })?;
    let patch_payload = serde_json::to_string(patch).map_err(|error| {
        report_internal_error(
            "Unable to serialize batch patch payload",
            &error,
            "Unable to save batch metadata diagnostics.",
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO metadata_batch_runs (run_key, mode, status, target_scope, patch_payload, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(run_id)
    .bind(mode)
    .bind(status)
    .bind(target_scope)
    .bind(patch_payload)
    .bind(created_at)
    .execute(executor)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to persist metadata batch run",
            &error,
            "Unable to save batch metadata diagnostics.",
        )
    })
    .map(|result| result.last_insert_rowid())
}

async fn persist_batch_outcome_with_executor<'a, E>(
    run_row_id: i64,
    outcome: &BatchMetadataOutcome,
    executor: E,
) -> Result<(), String>
where
    E: sqlx::Executor<'a, Database = Sqlite>,
{
    let before_snapshot = outcome
        .before
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            report_internal_error(
                "Unable to serialize batch before snapshot",
                &error,
                "Unable to save batch metadata diagnostics.",
            )
        })?;
    let after_snapshot = outcome
        .after
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            report_internal_error(
                "Unable to serialize batch after snapshot",
                &error,
                "Unable to save batch metadata diagnostics.",
            )
        })?;

    sqlx::query(
        r#"
        INSERT INTO metadata_batch_results (
          run_id, library_item_id, status, reason, retry_eligible, before_snapshot, after_snapshot
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(run_row_id)
    .bind(outcome.item_id)
    .bind(&outcome.status)
    .bind(&outcome.reason)
    .bind(if outcome.retry_eligible { 1 } else { 0 })
    .bind(before_snapshot)
    .bind(after_snapshot)
    .execute(executor)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to persist metadata batch result",
            &error,
            "Unable to save batch metadata diagnostics.",
        )
    })?;

    Ok(())
}

async fn run_batch_preview(
    input: BatchMetadataUpdateInput,
    pool: &SqlitePool,
) -> Result<BatchMetadataRunResult, String> {
    let item_ids = normalize_item_ids(input.item_ids)?;
    let patch = normalize_patch(input.patch)?;
    let mode = "preview";
    let run_id = deterministic_run_id(mode, &item_ids, &patch)?;

    let mut outcomes = Vec::new();
    for item_id in &item_ids {
        let current = match metadata::get_library_item_metadata_with_pool(
            metadata::GetLibraryItemMetadataInput { item_id: *item_id },
            pool,
        )
        .await
        {
            Ok(item) => item,
            Err(error) => {
                outcomes.push(BatchMetadataOutcome {
                    item_id: *item_id,
                    status: "failed".to_string(),
                    reason: Some(error),
                    retry_eligible: false,
                    before: None,
                    after: None,
                });
                continue;
            }
        };

        let (preview_after, changed) = apply_patch_preview(&current, &patch);
        if !changed {
            outcomes.push(BatchMetadataOutcome {
                item_id: *item_id,
                status: "skipped".to_string(),
                reason: Some("No effective change for this item.".to_string()),
                retry_eligible: false,
                before: Some(current),
                after: Some(preview_after),
            });
            continue;
        }

        outcomes.push(BatchMetadataOutcome {
            item_id: *item_id,
            status: "updated".to_string(),
            reason: None,
            retry_eligible: false,
            before: Some(current),
            after: Some(preview_after),
        });
    }

    let updated_count = outcomes
        .iter()
        .filter(|outcome| outcome.status == "updated")
        .count();
    let skipped_count = outcomes
        .iter()
        .filter(|outcome| outcome.status == "skipped")
        .count();
    let failed_count = outcomes
        .iter()
        .filter(|outcome| outcome.status == "failed")
        .count();
    let status = if failed_count == 0 {
        if updated_count == 0 {
            "no_changes".to_string()
        } else {
            "success".to_string()
        }
    } else if updated_count > 0 || skipped_count > 0 {
        "partial_success".to_string()
    } else {
        "failed".to_string()
    };

    let run_row_id =
        persist_batch_run_with_executor(&run_id, mode, &status, &item_ids, &patch, pool).await?;
    for outcome in &outcomes {
        persist_batch_outcome_with_executor(run_row_id, outcome, pool).await?;
    }

    Ok(BatchMetadataRunResult {
        run_id,
        mode: mode.to_string(),
        status,
        total_targets: item_ids.len(),
        updated_count,
        skipped_count,
        failed_count,
        outcomes,
    })
}

async fn run_batch_execute(
    input: BatchMetadataUpdateInput,
    pool: &SqlitePool,
) -> Result<BatchMetadataRunResult, String> {
    let item_ids = normalize_item_ids(input.item_ids)?;
    let patch = normalize_patch(input.patch)?;
    let mode = "execute";
    let run_id = deterministic_run_id(mode, &item_ids, &patch)?;

    let mut tx = pool.begin().await.map_err(|error| {
        report_internal_error(
            "Unable to start batch execute transaction",
            &error,
            "Unable to process metadata batch update.",
        )
    })?;

    let mut outcomes = Vec::new();
    for item_id in &item_ids {
        let current = match metadata::get_library_item_metadata_with_tx(
            metadata::GetLibraryItemMetadataInput { item_id: *item_id },
            &mut tx,
        )
        .await
        {
            Ok(item) => item,
            Err(error) => {
                outcomes.push(BatchMetadataOutcome {
                    item_id: *item_id,
                    status: "failed".to_string(),
                    reason: Some(error),
                    retry_eligible: false,
                    before: None,
                    after: None,
                });
                continue;
            }
        };

        let (preview_after, changed) = apply_patch_preview(&current, &patch);
        if !changed {
            outcomes.push(BatchMetadataOutcome {
                item_id: *item_id,
                status: "skipped".to_string(),
                reason: Some("No effective change for this item.".to_string()),
                retry_eligible: false,
                before: Some(current),
                after: Some(preview_after),
            });
            continue;
        }

        let update_input = metadata::UpdateLibraryItemMetadataInput {
            item_id: *item_id,
            title: preview_after.title.clone(),
            authors: preview_after.authors.clone(),
            language: preview_after.language.clone(),
            published_at: preview_after.published_at.clone(),
            tags: if patch.tags.is_some() {
                Some(preview_after.tags.clone())
            } else {
                None
            },
            collections: if patch.collections.is_some() {
                Some(preview_after.collections.clone())
            } else {
                None
            },
        };

        match metadata::update_library_item_metadata_with_tx(update_input, &mut tx).await {
            Ok(updated) => outcomes.push(BatchMetadataOutcome {
                item_id: *item_id,
                status: "updated".to_string(),
                reason: None,
                retry_eligible: false,
                before: Some(current),
                after: Some(updated),
            }),
            Err(error) => outcomes.push(BatchMetadataOutcome {
                item_id: *item_id,
                status: "failed".to_string(),
                reason: Some(error),
                retry_eligible: true,
                before: Some(current),
                after: None,
            }),
        }
    }

    let updated_count = outcomes
        .iter()
        .filter(|outcome| outcome.status == "updated")
        .count();
    let skipped_count = outcomes
        .iter()
        .filter(|outcome| outcome.status == "skipped")
        .count();
    let failed_count = outcomes
        .iter()
        .filter(|outcome| outcome.status == "failed")
        .count();
    let status = if failed_count == 0 {
        if updated_count == 0 {
            "no_changes".to_string()
        } else {
            "success".to_string()
        }
    } else if updated_count > 0 || skipped_count > 0 {
        "partial_success".to_string()
    } else {
        "failed".to_string()
    };

    let run_row_id =
        persist_batch_run_with_executor(&run_id, mode, &status, &item_ids, &patch, &mut *tx)
            .await?;
    for outcome in &outcomes {
        persist_batch_outcome_with_executor(run_row_id, outcome, &mut *tx).await?;
    }

    tx.commit().await.map_err(|error| {
        report_internal_error(
            "Unable to commit batch execute transaction",
            &error,
            "Unable to process metadata batch update.",
        )
    })?;

    Ok(BatchMetadataRunResult {
        run_id,
        mode: mode.to_string(),
        status,
        total_targets: item_ids.len(),
        updated_count,
        skipped_count,
        failed_count,
        outcomes,
    })
}

pub async fn preview_batch_metadata_update_with_pool(
    input: BatchMetadataUpdateInput,
    pool: &SqlitePool,
) -> Result<BatchMetadataRunResult, String> {
    run_batch_preview(input, pool).await
}

pub async fn execute_batch_metadata_update_with_pool(
    input: BatchMetadataUpdateInput,
    pool: &SqlitePool,
) -> Result<BatchMetadataRunResult, String> {
    run_batch_execute(input, pool).await
}
