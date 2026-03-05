use crate::library;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool};
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, INDEXED, STORED, TEXT};
use tantivy::{Index, ReloadPolicy, TantivyDocument, Term};
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
            "Unable to process index work unit.",
        )
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProcessIndexWorkQueueInput {
    pub batch_size: Option<u32>,
    pub include_failed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProcessIndexWorkQueueResult {
    pub processed_count: u32,
    pub success_count: u32,
    pub failed_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RetryFailedIndexWorkUnitsInput {
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RetryFailedIndexWorkUnitsResult {
    pub marked_retry_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexQueueStatus {
    pub queued_count: i64,
    pub running_count: i64,
    pub success_count: i64,
    pub failed_count: i64,
    pub retry_count: i64,
    pub recovered_count: i64,
    pub index_root: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EnsureIndexHealthResult {
    pub repair_performed: bool,
    pub rebuild_queued_count: u32,
    pub index_root: String,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexedSearchHit {
    pub library_item_id: i64,
    pub title: String,
    pub score: f32,
}

#[derive(Debug, Clone, FromRow)]
struct WorkUnitRow {
    id: i64,
    library_item_id: i64,
    status: String,
}

#[derive(Debug, Clone, FromRow)]
struct IndexableItemRow {
    id: i64,
    title: String,
    authors: String,
    language: Option<String>,
    published_at: Option<String>,
    source_path: String,
}

#[derive(Debug, Clone, Copy)]
struct IndexFields {
    library_item_id: Field,
    title: Field,
    authors: Field,
    language: Field,
    published_at: Field,
    source_path: Field,
    content: Field,
}

fn build_schema() -> (Schema, IndexFields) {
    let mut builder = Schema::builder();
    let library_item_id = builder.add_u64_field("library_item_id", INDEXED | STORED | FAST);
    let title = builder.add_text_field("title", TEXT | STORED);
    let authors = builder.add_text_field("authors", TEXT | STORED);
    let language = builder.add_text_field("language", TEXT | STORED);
    let published_at = builder.add_text_field("published_at", TEXT | STORED);
    let source_path = builder.add_text_field("source_path", TEXT | STORED);
    let content = builder.add_text_field("content", TEXT | STORED);
    let schema = builder.build();

    (
        schema,
        IndexFields {
            library_item_id,
            title,
            authors,
            language,
            published_at,
            source_path,
            content,
        },
    )
}

fn fields_from_schema(schema: &Schema) -> Result<IndexFields, String> {
    let library_item_id = schema
        .get_field("library_item_id")
        .map_err(|_| "Search index schema is missing library_item_id field.".to_string())?;
    let title = schema
        .get_field("title")
        .map_err(|_| "Search index schema is missing title field.".to_string())?;
    let authors = schema
        .get_field("authors")
        .map_err(|_| "Search index schema is missing authors field.".to_string())?;
    let language = schema
        .get_field("language")
        .map_err(|_| "Search index schema is missing language field.".to_string())?;
    let published_at = schema
        .get_field("published_at")
        .map_err(|_| "Search index schema is missing published_at field.".to_string())?;
    let source_path = schema
        .get_field("source_path")
        .map_err(|_| "Search index schema is missing source_path field.".to_string())?;
    let content = schema
        .get_field("content")
        .map_err(|_| "Search index schema is missing content field.".to_string())?;

    Ok(IndexFields {
        library_item_id,
        title,
        authors,
        language,
        published_at,
        source_path,
        content,
    })
}

fn index_root_from_library_path(library_path: &str) -> PathBuf {
    PathBuf::from(library_path)
        .join(".caudex")
        .join("search-index-v1")
}

async fn fetch_library_index_root(pool: &SqlitePool) -> Result<Option<PathBuf>, String> {
    let library = library::fetch_library(pool).await.map_err(|error| {
        report_internal_error(
            "Unable to load library for search index",
            &error,
            "Unable to access library configuration.",
        )
    })?;

    Ok(library.map(|entry| index_root_from_library_path(&entry.path)))
}

pub async fn index_root_path_with_pool(pool: &SqlitePool) -> Result<PathBuf, String> {
    fetch_library_index_root(pool)
        .await?
        .ok_or_else(|| "No library is configured for indexing yet.".to_string())
}

fn open_or_create_index(index_root: &Path, schema: &Schema) -> Result<Index, String> {
    fs::create_dir_all(index_root).map_err(|error| {
        report_internal_error(
            "Unable to create search index root",
            &error,
            "Unable to prepare search index storage.",
        )
    })?;

    if index_root.join("meta.json").exists() {
        return Index::open_in_dir(index_root).map_err(|error| {
            report_internal_error(
                "Unable to open search index",
                &error,
                "Search index is unreadable. Run index repair.",
            )
        });
    }

    Index::create_in_dir(index_root, schema.clone()).map_err(|error| {
        report_internal_error(
            "Unable to create search index",
            &error,
            "Unable to initialize search index.",
        )
    })
}

async fn count_by_status(pool: &SqlitePool, status: &str) -> Result<i64, String> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM index_work_units WHERE status = ?")
        .bind(status)
        .fetch_one(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to count index work unit status",
                &error,
                "Unable to load index queue status.",
            )
        })
}

pub async fn get_index_queue_status_with_pool(
    pool: &SqlitePool,
) -> Result<IndexQueueStatus, String> {
    let index_root = fetch_library_index_root(pool)
        .await?
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(IndexQueueStatus {
        queued_count: count_by_status(pool, "queued").await?,
        running_count: count_by_status(pool, "running").await?,
        success_count: count_by_status(pool, "success").await?,
        failed_count: count_by_status(pool, "failed").await?,
        retry_count: count_by_status(pool, "retry").await?,
        recovered_count: count_by_status(pool, "recovered").await?,
        index_root,
    })
}

fn normalize_batch_size(batch_size: Option<u32>) -> i64 {
    i64::from(batch_size.unwrap_or(50).clamp(1, 200))
}

fn parse_authors(authors_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(authors_json).unwrap_or_default()
}

async fn claim_work_units(
    pool: &SqlitePool,
    batch_size: i64,
    include_failed: bool,
) -> Result<Vec<WorkUnitRow>, String> {
    let mut tx = pool.begin().await.map_err(|error| {
        report_internal_error(
            "Unable to start index work unit claim transaction",
            &error,
            "Unable to load index queue.",
        )
    })?;

    let query = if include_failed {
        "SELECT id, library_item_id, status FROM index_work_units WHERE status IN ('queued', 'retry', 'failed') ORDER BY id ASC LIMIT ?"
    } else {
        "SELECT id, library_item_id, status FROM index_work_units WHERE status IN ('queued', 'retry') ORDER BY id ASC LIMIT ?"
    };

    let candidates = sqlx::query_as::<_, WorkUnitRow>(query)
        .bind(batch_size)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to load index work units",
                &error,
                "Unable to load index queue.",
            )
        })?;

    if candidates.is_empty() {
        tx.commit().await.map_err(|error| {
            report_internal_error(
                "Unable to commit empty index claim transaction",
                &error,
                "Unable to load index queue.",
            )
        })?;
        return Ok(Vec::new());
    }

    let claimed_at = now_utc_rfc3339()?;
    let mut claimed = Vec::with_capacity(candidates.len());
    for unit in candidates {
        let result = sqlx::query(
            "UPDATE index_work_units SET status = 'running', attempt_count = attempt_count + 1, updated_at = ?, last_error = NULL WHERE id = ? AND status = ?",
        )
        .bind(&claimed_at)
        .bind(unit.id)
        .bind(&unit.status)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to claim index work unit",
                &error,
                "Unable to update index queue state.",
            )
        })?;

        if result.rows_affected() == 1 {
            claimed.push(unit);
        }
    }

    tx.commit().await.map_err(|error| {
        report_internal_error(
            "Unable to commit index work unit claim transaction",
            &error,
            "Unable to load index queue.",
        )
    })?;

    Ok(claimed)
}

async fn fetch_item_for_index(
    pool: &SqlitePool,
    item_id: i64,
) -> Result<Option<IndexableItemRow>, String> {
    sqlx::query_as::<_, IndexableItemRow>(
        r#"
        SELECT id, title, authors, language, published_at, source_path
        FROM library_items
        WHERE id = ?
        "#,
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load library item for indexing",
            &error,
            "Unable to read library item for indexing.",
        )
    })
}

async fn mark_success(
    pool: &SqlitePool,
    work_unit_id: i64,
    status: &str,
    updated_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE index_work_units SET status = ?, updated_at = ?, completed_at = ?, last_error = NULL WHERE id = ?",
    )
    .bind(status)
    .bind(updated_at)
    .bind(updated_at)
    .bind(work_unit_id)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to mark index work unit as successful",
            &error,
            "Unable to update index queue state.",
        )
    })?;

    Ok(())
}

async fn mark_failed(
    pool: &SqlitePool,
    work_unit_id: i64,
    updated_at: &str,
    error_message: &str,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE index_work_units SET status = 'failed', updated_at = ?, completed_at = NULL, last_error = ? WHERE id = ?",
    )
    .bind(updated_at)
    .bind(error_message)
    .bind(work_unit_id)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to mark index work unit as failed",
            &error,
            "Unable to update index queue state.",
        )
    })?;

    Ok(())
}

fn index_single_item(
    writer: &mut tantivy::IndexWriter,
    fields: IndexFields,
    item: Option<IndexableItemRow>,
    item_id: i64,
) -> Result<(), String> {
    writer.delete_term(Term::from_field_u64(fields.library_item_id, item_id as u64));

    if let Some(item) = item {
        let authors = parse_authors(&item.authors);
        let author_text = authors.join(" ");
        let mut document = TantivyDocument::default();
        document.add_u64(fields.library_item_id, item.id as u64);
        document.add_text(fields.title, &item.title);
        document.add_text(fields.authors, &author_text);
        if let Some(language) = item.language.as_deref() {
            document.add_text(fields.language, language);
        }
        if let Some(published_at) = item.published_at.as_deref() {
            document.add_text(fields.published_at, published_at);
        }
        document.add_text(fields.source_path, &item.source_path);
        let content = format!(
            "{} {} {} {} {}",
            item.title,
            author_text,
            item.language.unwrap_or_default(),
            item.published_at.unwrap_or_default(),
            item.source_path
        );
        document.add_text(fields.content, &content);
        writer.add_document(document).map_err(|error| {
            report_internal_error(
                "Unable to add Tantivy document",
                &error,
                "Unable to index library item.",
            )
        })?;
    }

    writer.commit().map_err(|error| {
        report_internal_error(
            "Unable to commit Tantivy changes",
            &error,
            "Unable to persist index update.",
        )
    })?;

    Ok(())
}

pub async fn process_index_work_queue_with_pool(
    input: ProcessIndexWorkQueueInput,
    pool: &SqlitePool,
) -> Result<ProcessIndexWorkQueueResult, String> {
    let work_units = claim_work_units(
        pool,
        normalize_batch_size(input.batch_size),
        input.include_failed.unwrap_or(false),
    )
    .await?;

    if work_units.is_empty() {
        return Ok(ProcessIndexWorkQueueResult {
            processed_count: 0,
            success_count: 0,
            failed_count: 0,
        });
    }

    let index_root = index_root_path_with_pool(pool).await?;
    let (schema, fields) = build_schema();
    let index = open_or_create_index(&index_root, &schema);

    let mut success_count = 0_u32;
    let mut failed_count = 0_u32;

    let index = match index {
        Ok(index) => index,
        Err(index_error) => {
            for unit in &work_units {
                let updated_at = now_utc_rfc3339()?;
                mark_failed(pool, unit.id, &updated_at, &index_error).await?;
                failed_count += 1;
            }

            return Ok(ProcessIndexWorkQueueResult {
                processed_count: work_units.len() as u32,
                success_count,
                failed_count,
            });
        }
    };

    let mut writer = index.writer(20_000_000).map_err(|error| {
        report_internal_error(
            "Unable to create Tantivy index writer",
            &error,
            "Unable to start search indexing.",
        )
    })?;

    for unit in &work_units {
        let updated_at = now_utc_rfc3339()?;

        let indexing_result = async {
            let item = fetch_item_for_index(pool, unit.library_item_id).await?;
            index_single_item(&mut writer, fields, item, unit.library_item_id)
        }
        .await;

        match indexing_result {
            Ok(()) => {
                let next_status = if unit.status == "queued" {
                    "success"
                } else {
                    "recovered"
                };
                mark_success(pool, unit.id, next_status, &updated_at).await?;
                success_count += 1;
            }
            Err(error_message) => {
                mark_failed(pool, unit.id, &updated_at, &error_message).await?;
                failed_count += 1;
            }
        }
    }

    Ok(ProcessIndexWorkQueueResult {
        processed_count: work_units.len() as u32,
        success_count,
        failed_count,
    })
}

pub async fn retry_failed_index_work_units_with_pool(
    limit: Option<u32>,
    pool: &SqlitePool,
) -> Result<RetryFailedIndexWorkUnitsResult, String> {
    let ids: Vec<i64> = if let Some(limit) = limit {
        sqlx::query_scalar(
            "SELECT id FROM index_work_units WHERE status = 'failed' ORDER BY id ASC LIMIT ?",
        )
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to load failed index work units",
                &error,
                "Unable to retry failed index work units.",
            )
        })?
    } else {
        sqlx::query_scalar(
            "SELECT id FROM index_work_units WHERE status = 'failed' ORDER BY id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to load failed index work units",
                &error,
                "Unable to retry failed index work units.",
            )
        })?
    };

    let updated_at = now_utc_rfc3339()?;
    for id in &ids {
        sqlx::query(
            "UPDATE index_work_units SET status = 'retry', updated_at = ?, last_error = NULL WHERE id = ?",
        )
        .bind(&updated_at)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to mark failed work unit for retry",
                &error,
                "Unable to retry failed index work units.",
            )
        })?;
    }

    Ok(RetryFailedIndexWorkUnitsResult {
        marked_retry_count: ids.len() as u32,
    })
}

async fn queue_rebuild_for_all_items(pool: &SqlitePool) -> Result<u32, String> {
    let now = now_utc_rfc3339()?;
    let result = sqlx::query(
        r#"
        INSERT INTO index_work_units (library_item_id, status, created_at, updated_at)
        SELECT li.id, 'queued', ?, ?
        FROM library_items li
        WHERE NOT EXISTS (
          SELECT 1
          FROM index_work_units iwu
          WHERE iwu.library_item_id = li.id
            AND iwu.status IN ('queued', 'running', 'retry')
        )
        "#,
    )
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to queue index rebuild",
            &error,
            "Unable to queue index rebuild.",
        )
    })?;

    Ok(result.rows_affected() as u32)
}

async fn recover_interrupted_running_work_units<'a, E>(executor: E) -> Result<u32, String>
where
    E: sqlx::Executor<'a, Database = Sqlite>,
{
    let now = now_utc_rfc3339()?;
    let result = sqlx::query(
        r#"
        UPDATE index_work_units
        SET status = 'retry', updated_at = ?
        WHERE status = 'running'
        "#,
    )
    .bind(&now)
    .execute(executor)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to recover interrupted running index work units",
            &error,
            "Unable to repair search index queue.",
        )
    })?;

    Ok(result.rows_affected() as u32)
}

async fn ensure_search_index_health_internal(
    pool: &SqlitePool,
    recover_running_units: bool,
) -> Result<EnsureIndexHealthResult, String> {
    let recovered_running_count = if recover_running_units {
        recover_interrupted_running_work_units(pool).await?
    } else {
        0
    };

    let mut queue_diagnostic = if recovered_running_count > 0 {
        format!("Recovered {recovered_running_count} interrupted running work unit(s).")
    } else {
        String::new()
    };

    let Some(index_root) = fetch_library_index_root(pool).await? else {
        return Ok(EnsureIndexHealthResult {
            repair_performed: false,
            rebuild_queued_count: 0,
            index_root: String::new(),
            diagnostic: if queue_diagnostic.is_empty() {
                "No library configured; index health check skipped.".to_string()
            } else {
                format!("{queue_diagnostic} No library configured; index health check skipped.")
            },
        });
    };

    let (schema, _) = build_schema();
    let mut repair_performed = false;
    let index_was_missing = !index_root.exists();

    if index_root.is_file() {
        fs::remove_file(&index_root).map_err(|error| {
            report_internal_error(
                "Unable to remove invalid index root file",
                &error,
                "Unable to repair search index.",
            )
        })?;
        repair_performed = true;
    }

    let open_result = open_or_create_index(&index_root, &schema);
    let mut diagnostic = "Search index healthy.".to_string();

    if open_result.is_err() {
        repair_performed = true;
        if index_root.exists() {
            fs::remove_dir_all(&index_root).map_err(|error| {
                report_internal_error(
                    "Unable to remove corrupted index directory",
                    &error,
                    "Unable to repair corrupted search index.",
                )
            })?;
        }
        fs::create_dir_all(&index_root).map_err(|error| {
            report_internal_error(
                "Unable to recreate search index root",
                &error,
                "Unable to repair search index.",
            )
        })?;
        Index::create_in_dir(&index_root, schema.clone()).map_err(|error| {
            report_internal_error(
                "Unable to recreate search index",
                &error,
                "Unable to repair search index.",
            )
        })?;
        diagnostic = "Search index repaired after corruption detection.".to_string();
    } else if index_was_missing {
        repair_performed = true;
        diagnostic = "Search index initialized because it was missing.".to_string();
    } else if !index_root.join("meta.json").exists() {
        repair_performed = true;
        diagnostic = "Search index initialized because it was missing.".to_string();
    }

    let rebuild_queued_count = if repair_performed {
        queue_rebuild_for_all_items(pool).await?
    } else {
        0
    };

    if !queue_diagnostic.is_empty() {
        queue_diagnostic.push(' ');
        diagnostic = format!("{queue_diagnostic}{diagnostic}");
    }

    Ok(EnsureIndexHealthResult {
        repair_performed,
        rebuild_queued_count,
        index_root: index_root.to_string_lossy().to_string(),
        diagnostic,
    })
}

pub async fn ensure_search_index_health_on_startup_with_pool(
    pool: &SqlitePool,
) -> Result<EnsureIndexHealthResult, String> {
    ensure_search_index_health_internal(pool, true).await
}

pub async fn ensure_search_index_health_with_pool(
    pool: &SqlitePool,
) -> Result<EnsureIndexHealthResult, String> {
    ensure_search_index_health_internal(pool, false).await
}

pub async fn search_index_documents_with_pool(
    query: &str,
    limit: usize,
    pool: &SqlitePool,
) -> Result<Vec<IndexedSearchHit>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let index_root = index_root_path_with_pool(pool).await?;
    let (schema, default_fields) = build_schema();
    let index = open_or_create_index(&index_root, &schema)?;
    let fields = fields_from_schema(&index.schema()).unwrap_or(default_fields);

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .map_err(|error| {
            report_internal_error(
                "Unable to open search index reader",
                &error,
                "Unable to execute search query.",
            )
        })?;

    reader.reload().map_err(|error| {
        report_internal_error(
            "Unable to reload search index reader",
            &error,
            "Unable to execute search query.",
        )
    })?;

    let searcher = reader.searcher();
    let parser = QueryParser::for_index(
        &index,
        vec![
            fields.title,
            fields.authors,
            fields.language,
            fields.published_at,
            fields.source_path,
            fields.content,
        ],
    );

    let parsed = parser.parse_query(trimmed).map_err(|error| {
        report_internal_error(
            "Unable to parse search query",
            &error,
            "Search query syntax is invalid.",
        )
    })?;

    let top_docs = searcher
        .search(&parsed, &TopDocs::with_limit(limit.max(1)))
        .map_err(|error| {
            report_internal_error(
                "Unable to execute search query",
                &error,
                "Unable to execute search query.",
            )
        })?;

    let mut hits = Vec::with_capacity(top_docs.len());
    for (score, address) in top_docs {
        let document: TantivyDocument = searcher.doc(address).map_err(|error| {
            report_internal_error(
                "Unable to load search document",
                &error,
                "Unable to load search result.",
            )
        })?;

        let library_item_id = document
            .get_first(fields.library_item_id)
            .and_then(|value| value.as_u64())
            .map(|value| value as i64)
            .ok_or_else(|| "Search document missing library_item_id.".to_string())?;

        let title = document
            .get_first(fields.title)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();

        hits.push(IndexedSearchHit {
            library_item_id,
            title,
            score,
        });
    }

    Ok(hits)
}
