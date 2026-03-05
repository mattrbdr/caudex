use appsdesktop_lib::library::{insert_library, run_migrations};
use appsdesktop_lib::search_index::{
    process_index_work_queue_with_pool, search_index_documents_with_pool,
    ProcessIndexWorkQueueInput,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

struct BenchmarkConfig {
    corpus_size: usize,
    query_count: usize,
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("Invalid numeric value for {flag}: {value}"))
}

fn parse_args() -> Result<BenchmarkConfig, String> {
    let mut corpus_size = 2_000_usize;
    let mut query_count = 250_usize;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--corpus" => {
                let value = args
                    .next()
                    .ok_or_else(|| "Missing value for --corpus".to_string())?;
                corpus_size = parse_usize(&value, "--corpus")?;
            }
            "--queries" => {
                let value = args
                    .next()
                    .ok_or_else(|| "Missing value for --queries".to_string())?;
                query_count = parse_usize(&value, "--queries")?;
            }
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }

    if corpus_size == 0 {
        return Err("Corpus size must be greater than 0.".to_string());
    }
    if query_count == 0 {
        return Err("Query count must be greater than 0.".to_string());
    }

    Ok(BenchmarkConfig {
        corpus_size,
        query_count,
    })
}

async fn setup_pool(path: PathBuf) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .map_err(|error| format!("Unable to initialize benchmark pool: {error}"))
}

async fn seed_corpus(pool: &SqlitePool, corpus_size: usize) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("Unable to begin corpus seed transaction: {error}"))?;

    for index in 0..corpus_size {
        let category = if index % 7 == 0 { "history" } else { "fiction" };
        let title = format!("book {index} caudex {category} metadata");
        let source_path = format!("/tmp/caudex-bench-{index}.epub");

        let inserted = sqlx::query(
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
            VALUES (1, ?, 'epub', ?, ?, 'en', '2024-01-01', '2026-03-05T10:35:00Z')
            "#,
        )
        .bind(&source_path)
        .bind(&title)
        .bind(r#"["Benchmark Author"]"#)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("Unable to seed benchmark library item: {error}"))?;

        let item_id = inserted.last_insert_rowid();
        sqlx::query(
            r#"
            INSERT INTO index_work_units (library_item_id, status, created_at, updated_at)
            VALUES (?, 'queued', '2026-03-05T10:35:00Z', '2026-03-05T10:35:00Z')
            "#,
        )
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("Unable to seed benchmark index work unit: {error}"))?;
    }

    tx.commit()
        .await
        .map_err(|error| format!("Unable to commit corpus seed transaction: {error}"))?;

    Ok(())
}

fn percentile95(values: &[f64]) -> Result<f64, String> {
    if values.is_empty() {
        return Err("Cannot compute p95 from an empty dataset.".to_string());
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() as f64) * 0.95).ceil() as usize;
    let bounded = index.saturating_sub(1).min(sorted.len() - 1);
    Ok((sorted[bounded] * 1_000.0).round() / 1_000.0)
}

async fn process_full_queue(pool: &SqlitePool) -> Result<u32, String> {
    let mut processed_total = 0_u32;
    loop {
        let result = process_index_work_queue_with_pool(
            ProcessIndexWorkQueueInput {
                batch_size: Some(200),
                include_failed: Some(false),
            },
            pool,
        )
        .await?;

        processed_total += result.processed_count;
        if result.failed_count > 0 {
            return Err(format!(
                "Index benchmark setup failed with {} failed unit(s).",
                result.failed_count
            ));
        }
        if result.processed_count == 0 {
            break;
        }
    }

    Ok(processed_total)
}

#[tokio::main]
async fn main() {
    let output = match run_benchmark().await {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    println!(
        "{}",
        serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string())
    );
}

async fn run_benchmark() -> Result<serde_json::Value, String> {
    let config = parse_args()?;
    let root = env::temp_dir().join(format!(
        "caudex-search-bench-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("Unable to read current time: {error}"))?
            .as_nanos()
    ));
    fs::create_dir_all(&root)
        .map_err(|error| format!("Unable to create benchmark temp dir: {error}"))?;
    let pool = setup_pool(root.join("caudex-benchmark.db")).await?;
    run_migrations(&pool)
        .await
        .map_err(|error| format!("Unable to run benchmark migrations: {error}"))?;

    insert_library(
        &pool,
        "Benchmark Library",
        root.to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .map_err(|error| format!("Unable to insert benchmark library: {error}"))?;

    seed_corpus(&pool, config.corpus_size).await?;
    let indexed_count = process_full_queue(&pool).await?;

    let mut durations_ms = Vec::with_capacity(config.query_count);
    for index in 0..config.query_count {
        let query = if index % 5 == 0 {
            "history metadata".to_string()
        } else {
            format!("book {index}")
        };

        let started = Instant::now();
        let _ = search_index_documents_with_pool(&query, 20, &pool).await?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        durations_ms.push(elapsed_ms);
    }

    let search_p95_ms = percentile95(&durations_ms)?;
    Ok(serde_json::json!({
        "search_p95_ms": search_p95_ms,
        "corpus_size": config.corpus_size,
        "query_count": config.query_count,
        "indexed_count": indexed_count
    }))
}
