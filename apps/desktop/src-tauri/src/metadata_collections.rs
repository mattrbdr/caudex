use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;

fn report_internal_error(context: &str, error: &dyn Display, user_message: &str) -> String {
    eprintln!("{context}: {error}");
    user_message.to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AssignItemTagsCollectionsInput {
    pub item_id: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AssignItemTagsCollectionsResult {
    pub item_id: i64,
    pub tags: Vec<String>,
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListItemTagsCollectionsInput {
    pub item_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ItemTagsCollections {
    pub item_id: i64,
    pub tags: Vec<String>,
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ListMetadataNamesResult {
    pub names: Vec<String>,
}

#[derive(Debug, FromRow)]
struct TaxonomyRow {
    library_item_id: i64,
    name: String,
}

pub(crate) fn normalize_labels(values: &[String], kind: &str) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.len() > 64 {
            return Err(format!("{kind} labels must be 64 characters or fewer."));
        }

        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            normalized.push(trimmed.to_string());
        }
    }

    Ok(normalized)
}

pub(crate) async fn load_tags_map(
    item_ids: &[i64],
    pool: &SqlitePool,
) -> Result<HashMap<i64, Vec<String>>, String> {
    load_taxonomy_map_with_executor(item_ids, "item_tags", "tag_id", "tags", pool).await
}

pub(crate) async fn load_collections_map(
    item_ids: &[i64],
    pool: &SqlitePool,
) -> Result<HashMap<i64, Vec<String>>, String> {
    load_taxonomy_map_with_executor(
        item_ids,
        "item_collections",
        "collection_id",
        "collections",
        pool,
    )
    .await
}

pub(crate) async fn load_tags_map_with_tx(
    item_ids: &[i64],
    tx: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<HashMap<i64, Vec<String>>, String> {
    load_taxonomy_map_with_executor(item_ids, "item_tags", "tag_id", "tags", &mut **tx).await
}

pub(crate) async fn load_collections_map_with_tx(
    item_ids: &[i64],
    tx: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<HashMap<i64, Vec<String>>, String> {
    load_taxonomy_map_with_executor(
        item_ids,
        "item_collections",
        "collection_id",
        "collections",
        &mut **tx,
    )
    .await
}

async fn load_taxonomy_map_with_executor<'a, E>(
    item_ids: &[i64],
    relation_table: &str,
    relation_foreign_key: &str,
    names_table: &str,
    executor: E,
) -> Result<HashMap<i64, Vec<String>>, String>
where
    E: sqlx::Executor<'a, Database = Sqlite>,
{
    let mut grouped = HashMap::new();
    if item_ids.is_empty() {
        return Ok(grouped);
    }

    let placeholders = std::iter::repeat("?")
        .take(item_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT rel.library_item_id AS library_item_id, names.name AS name
        FROM {relation_table} rel
        JOIN {names_table} names ON names.id = rel.{relation_foreign_key}
        WHERE rel.library_item_id IN ({placeholders})
        ORDER BY names.name COLLATE NOCASE ASC
        "#
    );
    let mut query = sqlx::query_as::<_, TaxonomyRow>(&sql);
    for item_id in item_ids {
        query = query.bind(item_id);
    }

    let rows = query.fetch_all(executor).await.map_err(|error| {
        report_internal_error(
            "Unable to load metadata taxonomy map",
            &error,
            "Unable to load metadata taxonomy.",
        )
    })?;

    for row in rows {
        grouped
            .entry(row.library_item_id)
            .or_insert_with(Vec::new)
            .push(row.name);
    }

    Ok(grouped)
}

async fn ensure_library_item_exists(item_id: i64, pool: &SqlitePool) -> Result<(), String> {
    let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM library_items WHERE id = ?")
        .bind(item_id)
        .fetch_one(pool)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to verify library item existence",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })?;

    if exists == 0 {
        return Err("Library item not found.".to_string());
    }

    Ok(())
}

async fn resolve_tag_id(
    name: &str,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<i64, String> {
    sqlx::query(
        r#"
        INSERT INTO tags (name)
        VALUES (?)
        ON CONFLICT(name) DO UPDATE SET name = excluded.name
        "#,
    )
    .bind(name)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to upsert tag",
            &error,
            "Unable to update metadata taxonomy.",
        )
    })?;

    sqlx::query_scalar::<_, i64>("SELECT id FROM tags WHERE name = ? LIMIT 1")
        .bind(name)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to resolve tag id",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })
}

async fn resolve_collection_id(
    name: &str,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<i64, String> {
    sqlx::query(
        r#"
        INSERT INTO collections (name)
        VALUES (?)
        ON CONFLICT(name) DO UPDATE SET name = excluded.name
        "#,
    )
    .bind(name)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to upsert collection",
            &error,
            "Unable to update metadata taxonomy.",
        )
    })?;

    sqlx::query_scalar::<_, i64>("SELECT id FROM collections WHERE name = ? LIMIT 1")
        .bind(name)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to resolve collection id",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })
}

pub(crate) async fn replace_item_tags_with_tx(
    item_id: i64,
    tags: &[String],
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM item_tags WHERE library_item_id = ?")
        .bind(item_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to clear item tags",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })?;

    for tag in tags {
        let tag_id = resolve_tag_id(tag, tx).await?;
        sqlx::query(
            r#"
            INSERT INTO item_tags (library_item_id, tag_id)
            VALUES (?, ?)
            ON CONFLICT(library_item_id, tag_id) DO NOTHING
            "#,
        )
        .bind(item_id)
        .bind(tag_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to assign tag to item",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })?;
    }

    Ok(())
}

pub(crate) async fn replace_item_collections_with_tx(
    item_id: i64,
    collections: &[String],
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM item_collections WHERE library_item_id = ?")
        .bind(item_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to clear item collections",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })?;

    for collection in collections {
        let collection_id = resolve_collection_id(collection, tx).await?;
        sqlx::query(
            r#"
            INSERT INTO item_collections (library_item_id, collection_id)
            VALUES (?, ?)
            ON CONFLICT(library_item_id, collection_id) DO NOTHING
            "#,
        )
        .bind(item_id)
        .bind(collection_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| {
            report_internal_error(
                "Unable to assign collection to item",
                &error,
                "Unable to update metadata taxonomy.",
            )
        })?;
    }

    Ok(())
}

pub async fn assign_item_tags_collections_with_pool(
    input: AssignItemTagsCollectionsInput,
    pool: &SqlitePool,
) -> Result<AssignItemTagsCollectionsResult, String> {
    ensure_library_item_exists(input.item_id, pool).await?;
    let tags = normalize_labels(&input.tags, "Tag")?;
    let collections = normalize_labels(&input.collections, "Collection")?;

    let mut tx = pool.begin().await.map_err(|error| {
        report_internal_error(
            "Unable to start taxonomy assignment transaction",
            &error,
            "Unable to update metadata taxonomy.",
        )
    })?;

    replace_item_tags_with_tx(input.item_id, &tags, &mut tx).await?;
    replace_item_collections_with_tx(input.item_id, &collections, &mut tx).await?;

    tx.commit().await.map_err(|error| {
        report_internal_error(
            "Unable to commit taxonomy assignment transaction",
            &error,
            "Unable to update metadata taxonomy.",
        )
    })?;

    Ok(AssignItemTagsCollectionsResult {
        item_id: input.item_id,
        tags,
        collections,
    })
}

pub async fn list_item_tags_collections_with_pool(
    input: ListItemTagsCollectionsInput,
    pool: &SqlitePool,
) -> Result<ItemTagsCollections, String> {
    ensure_library_item_exists(input.item_id, pool).await?;
    let tags_map = load_tags_map(&[input.item_id], pool).await?;
    let collections_map = load_collections_map(&[input.item_id], pool).await?;

    Ok(ItemTagsCollections {
        item_id: input.item_id,
        tags: tags_map.get(&input.item_id).cloned().unwrap_or_default(),
        collections: collections_map
            .get(&input.item_id)
            .cloned()
            .unwrap_or_default(),
    })
}

pub async fn list_metadata_tags_with_pool(
    pool: &SqlitePool,
) -> Result<ListMetadataNamesResult, String> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT name
        FROM tags
        ORDER BY name COLLATE NOCASE ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to list metadata tags",
            &error,
            "Unable to load metadata tags.",
        )
    })?;

    Ok(ListMetadataNamesResult { names: rows })
}

pub async fn list_metadata_collections_with_pool(
    pool: &SqlitePool,
) -> Result<ListMetadataNamesResult, String> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT name
        FROM collections
        ORDER BY name COLLATE NOCASE ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to list metadata collections",
            &error,
            "Unable to load metadata collections.",
        )
    })?;

    Ok(ListMetadataNamesResult { names: rows })
}
