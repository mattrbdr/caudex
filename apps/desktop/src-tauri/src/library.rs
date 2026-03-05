use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct Library {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub created_at: String,
}

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

pub async fn fetch_library(pool: &SqlitePool) -> Result<Option<Library>, sqlx::Error> {
    sqlx::query_as::<_, Library>(
        r#"
        SELECT id, name, path, created_at
        FROM libraries
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

pub async fn insert_library(
    pool: &SqlitePool,
    name: &str,
    path: &str,
    created_at: &str,
) -> Result<Library, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO libraries (name, path, created_at)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(name)
    .bind(path)
    .bind(created_at)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Library>(
        r#"
        SELECT id, name, path, created_at
        FROM libraries
        WHERE id = ?
        "#,
    )
    .bind(result.last_insert_rowid())
    .fetch_one(pool)
    .await
}
