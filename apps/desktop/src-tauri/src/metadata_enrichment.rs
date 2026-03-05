use crate::metadata;
use crate::providers;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<MetadataCandidate>, String>> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MetadataCandidate {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub confidence: f64,
    pub raw_payload: String,
}

pub trait MetadataProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn lookup_by_isbn<'a>(&'a self, isbn: &'a str) -> ProviderFuture<'a>;
    fn lookup_by_title_author<'a>(&'a self, title: &'a str, authors: &'a [String]) -> ProviderFuture<'a>;
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EnrichLibraryItemMetadataInput {
    pub item_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListMetadataEnrichmentProposalsInput {
    pub item_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApplyMetadataEnrichmentProposalInput {
    pub proposal_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MetadataEnrichmentProposal {
    pub id: i64,
    pub provider: String,
    pub confidence: f64,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub diagnostic: Option<String>,
    pub applied_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EnrichmentRunResult {
    pub run_id: i64,
    pub status: String,
    pub diagnostic: Option<String>,
    pub proposals: Vec<MetadataEnrichmentProposal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ListMetadataEnrichmentProposalsResult {
    pub item_id: i64,
    pub proposals: Vec<MetadataEnrichmentProposal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ApplyMetadataEnrichmentProposalResult {
    pub proposal_id: i64,
    pub item: metadata::LibraryItemMetadata,
}

#[derive(Debug, FromRow)]
struct EnrichmentProposalRow {
    id: i64,
    provider: String,
    confidence: f64,
    title: Option<String>,
    authors: String,
    language: Option<String>,
    published_at: Option<String>,
    diagnostic: Option<String>,
    applied_at: Option<String>,
}

#[derive(Debug, FromRow)]
struct ItemLookupRow {
    id: i64,
    title: String,
    authors: String,
    source_path: String,
}

#[derive(Debug, FromRow)]
struct ProposalApplyRow {
    id: i64,
    library_item_id: i64,
    title: Option<String>,
    authors: String,
    language: Option<String>,
    published_at: Option<String>,
    applied_at: Option<String>,
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
            "Unable to process metadata enrichment at this time.",
        )
    })
}

fn parse_authors_json(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn extract_isbn(text: &str) -> Option<String> {
    let mut token = String::new();
    let mut candidates = Vec::new();

    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == 'X' || ch == 'x' || ch == '-' {
            token.push(ch);
        } else if !token.is_empty() {
            candidates.push(token.clone());
            token.clear();
        }
    }
    if !token.is_empty() {
        candidates.push(token);
    }

    for candidate in candidates {
        let normalized: String = candidate
            .chars()
            .filter(|ch| ch.is_ascii_digit() || *ch == 'X' || *ch == 'x')
            .collect();
        let is_len_13 = normalized.len() == 13 && normalized.chars().all(|ch| ch.is_ascii_digit());
        let is_len_10 = normalized.len() == 10
            && normalized
                .chars()
                .enumerate()
                .all(|(idx, ch)| ch.is_ascii_digit() || (idx == 9 && (ch == 'X' || ch == 'x')));
        if is_len_13 || is_len_10 {
            return Some(normalized.to_uppercase());
        }
    }

    None
}

fn map_enrichment_proposal_row(row: EnrichmentProposalRow) -> MetadataEnrichmentProposal {
    MetadataEnrichmentProposal {
        id: row.id,
        provider: row.provider,
        confidence: row.confidence,
        title: row.title,
        authors: parse_authors_json(&row.authors),
        language: row.language,
        published_at: row.published_at,
        diagnostic: row.diagnostic,
        applied_at: row.applied_at,
    }
}

async fn load_item_for_enrichment(item_id: i64, pool: &SqlitePool) -> Result<ItemLookupRow, String> {
    sqlx::query_as::<_, ItemLookupRow>(
        r#"
        SELECT id, title, authors, source_path
        FROM library_items
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load item for metadata enrichment",
            &error,
            "Unable to load metadata item for enrichment.",
        )
    })?
    .ok_or_else(|| "Library item not found.".to_string())
}

async fn create_enrichment_run(item_id: i64, pool: &SqlitePool) -> Result<i64, String> {
    let started_at = now_utc_rfc3339()?;
    sqlx::query(
        r#"
        INSERT INTO metadata_enrichment_runs (library_item_id, status, started_at)
        VALUES (?, 'running', ?)
        "#,
    )
    .bind(item_id)
    .bind(started_at)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to create metadata enrichment run",
            &error,
            "Unable to start metadata enrichment.",
        )
    })
    .map(|result| result.last_insert_rowid())
}

async fn persist_proposal(
    run_id: i64,
    item_id: i64,
    provider: &str,
    candidate: &MetadataCandidate,
    diagnostic: Option<&str>,
    pool: &SqlitePool,
) -> Result<i64, String> {
    let created_at = now_utc_rfc3339()?;
    let authors = serde_json::to_string(&candidate.authors).map_err(|error| {
        report_internal_error(
            "Unable to serialize proposal authors",
            &error,
            "Unable to save metadata enrichment proposal.",
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO metadata_enrichment_proposals (
          run_id, library_item_id, provider, confidence, title, authors, language, published_at, raw_payload, diagnostic, created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(run_id)
    .bind(item_id)
    .bind(provider)
    .bind(candidate.confidence)
    .bind(&candidate.title)
    .bind(authors)
    .bind(&candidate.language)
    .bind(&candidate.published_at)
    .bind(&candidate.raw_payload)
    .bind(diagnostic)
    .bind(created_at)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to persist metadata enrichment proposal",
            &error,
            "Unable to save metadata enrichment proposal.",
        )
    })
    .map(|result| result.last_insert_rowid())
}

async fn complete_enrichment_run(
    run_id: i64,
    status: &str,
    diagnostic: Option<&str>,
    pool: &SqlitePool,
) -> Result<(), String> {
    let completed_at = now_utc_rfc3339()?;
    sqlx::query(
        r#"
        UPDATE metadata_enrichment_runs
        SET status = ?, diagnostic = ?, completed_at = ?
        WHERE id = ?
        "#,
    )
    .bind(status)
    .bind(diagnostic)
    .bind(completed_at)
    .bind(run_id)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to finalize metadata enrichment run",
            &error,
            "Unable to complete metadata enrichment.",
        )
    })?;

    Ok(())
}

async fn list_proposals_for_item(item_id: i64, pool: &SqlitePool) -> Result<Vec<MetadataEnrichmentProposal>, String> {
    let rows = sqlx::query_as::<_, EnrichmentProposalRow>(
        r#"
        SELECT id, provider, confidence, title, authors, language, published_at, diagnostic, applied_at
        FROM metadata_enrichment_proposals
        WHERE library_item_id = ?
        ORDER BY id DESC
        "#,
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to list metadata enrichment proposals",
            &error,
            "Unable to load metadata enrichment proposals.",
        )
    })?;

    Ok(rows.into_iter().map(map_enrichment_proposal_row).collect())
}

async fn call_provider_with_retry_isbn(
    provider: &dyn MetadataProvider,
    isbn: &str,
    max_attempts: u8,
) -> (Option<MetadataCandidate>, Option<String>) {
    let mut last_error = None;
    for attempt in 0..max_attempts {
        match provider.lookup_by_isbn(isbn).await {
            Ok(candidate) => return (candidate, last_error),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < max_attempts {
                    tokio::time::sleep(backoff_duration_with_jitter(attempt)).await;
                }
            }
        }
    }
    (None, last_error)
}

async fn call_provider_with_retry_title_author(
    provider: &dyn MetadataProvider,
    title: &str,
    authors: &[String],
    max_attempts: u8,
) -> (Option<MetadataCandidate>, Option<String>) {
    let mut last_error = None;
    for attempt in 0..max_attempts {
        match provider.lookup_by_title_author(title, authors).await {
            Ok(candidate) => return (candidate, last_error),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < max_attempts {
                    tokio::time::sleep(backoff_duration_with_jitter(attempt)).await;
                }
            }
        }
    }
    (None, last_error)
}

fn backoff_duration_with_jitter(attempt: u8) -> Duration {
    let base = 100_u64 * u64::from(attempt + 1);
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::from(elapsed.subsec_millis() % 50))
        .unwrap_or(0);
    Duration::from_millis(base + jitter)
}

async fn lookup_provider_with_isbn_then_title_author(
    provider: &dyn MetadataProvider,
    isbn_hint: Option<&str>,
    title: &str,
    authors: &[String],
) -> (Option<MetadataCandidate>, Vec<String>) {
    let mut diagnostics = Vec::new();

    if let Some(isbn) = isbn_hint {
        let (candidate, isbn_error) = call_provider_with_retry_isbn(provider, isbn, 3).await;
        if let Some(error) = isbn_error {
            diagnostics.push(format!("isbn lookup failed: {error}"));
        }
        if candidate.is_some() {
            return (candidate, diagnostics);
        }
    }

    let (candidate, title_author_error) =
        call_provider_with_retry_title_author(provider, title, authors, 3).await;
    if let Some(error) = title_author_error {
        diagnostics.push(format!("title/author lookup failed: {error}"));
    }
    (candidate, diagnostics)
}

pub async fn enrich_library_item_metadata_with_providers(
    input: EnrichLibraryItemMetadataInput,
    providers: &[Box<dyn MetadataProvider>],
    pool: &SqlitePool,
) -> Result<EnrichmentRunResult, String> {
    let item = load_item_for_enrichment(input.item_id, pool).await?;
    let run_id = create_enrichment_run(item.id, pool).await?;

    let authors = parse_authors_json(&item.authors);
    let isbn_hint = extract_isbn(&format!("{} {}", item.title, item.source_path));
    let mut diagnostics = Vec::new();
    let mut proposals = Vec::new();

    for (index, provider) in providers.iter().enumerate() {
        let provider_ref = provider.as_ref();
        let (candidate, provider_diagnostics) = lookup_provider_with_isbn_then_title_author(
            provider_ref,
            isbn_hint.as_deref(),
            &item.title,
            &authors,
        )
        .await;

        if !provider_diagnostics.is_empty() {
            diagnostics.push(format!(
                "{}: {}",
                provider_ref.provider_name(),
                provider_diagnostics.join(" | ")
            ));
        }

        if let Some(candidate) = candidate {
            let degraded_diagnostic = if index > 0 || !diagnostics.is_empty() {
                Some("Primary provider degraded; fallback provider proposal used.")
            } else {
                None
            };
            let proposal_id = persist_proposal(
                run_id,
                item.id,
                provider_ref.provider_name(),
                &candidate,
                degraded_diagnostic,
                pool,
            )
            .await?;
            proposals.push(MetadataEnrichmentProposal {
                id: proposal_id,
                provider: provider_ref.provider_name().to_string(),
                confidence: candidate.confidence,
                title: candidate.title,
                authors: candidate.authors,
                language: candidate.language,
                published_at: candidate.published_at,
                diagnostic: degraded_diagnostic.map(ToString::to_string),
                applied_at: None,
            });
            let status = if diagnostics.is_empty() { "success" } else { "degraded" };
            let diagnostic = if diagnostics.is_empty() {
                None
            } else {
                Some(diagnostics.join(" | "))
            };
            complete_enrichment_run(run_id, status, diagnostic.as_deref(), pool).await?;
            return Ok(EnrichmentRunResult {
                run_id,
                status: status.to_string(),
                diagnostic,
                proposals,
            });
        }
    }

    let diagnostic = if diagnostics.is_empty() {
        Some("No enrichment proposal available from configured providers.".to_string())
    } else {
        Some(diagnostics.join(" | "))
    };
    complete_enrichment_run(run_id, "failed", diagnostic.as_deref(), pool).await?;
    Ok(EnrichmentRunResult {
        run_id,
        status: "failed".to_string(),
        diagnostic,
        proposals,
    })
}

pub async fn enrich_library_item_metadata_with_pool(
    input: EnrichLibraryItemMetadataInput,
    pool: &SqlitePool,
) -> Result<EnrichmentRunResult, String> {
    let providers = providers::default_providers()?;
    enrich_library_item_metadata_with_providers(input, providers.as_slice(), pool).await
}

pub async fn list_metadata_enrichment_proposals_with_pool(
    input: ListMetadataEnrichmentProposalsInput,
    pool: &SqlitePool,
) -> Result<ListMetadataEnrichmentProposalsResult, String> {
    load_item_for_enrichment(input.item_id, pool).await?;
    let proposals = list_proposals_for_item(input.item_id, pool).await?;
    Ok(ListMetadataEnrichmentProposalsResult {
        item_id: input.item_id,
        proposals,
    })
}

pub async fn apply_metadata_enrichment_proposal_with_pool(
    input: ApplyMetadataEnrichmentProposalInput,
    pool: &SqlitePool,
) -> Result<ApplyMetadataEnrichmentProposalResult, String> {
    let proposal = sqlx::query_as::<_, ProposalApplyRow>(
        r#"
        SELECT id, library_item_id, title, authors, language, published_at, applied_at
        FROM metadata_enrichment_proposals
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(input.proposal_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to load metadata enrichment proposal",
            &error,
            "Unable to apply metadata enrichment proposal.",
        )
    })?
    .ok_or_else(|| "Metadata enrichment proposal not found.".to_string())?;

    if proposal.applied_at.is_some() {
        return Err("Metadata enrichment proposal already applied.".to_string());
    }

    let current = metadata::get_library_item_metadata_with_pool(
        metadata::GetLibraryItemMetadataInput {
            item_id: proposal.library_item_id,
        },
        pool,
    )
    .await?;

    let proposal_authors = parse_authors_json(&proposal.authors);
    let updated_item = metadata::update_library_item_metadata_with_pool(
        metadata::UpdateLibraryItemMetadataInput {
            item_id: proposal.library_item_id,
            title: proposal.title.unwrap_or(current.title),
            authors: if proposal_authors.is_empty() {
                current.authors
            } else {
                proposal_authors
            },
            language: proposal.language.or(current.language),
            published_at: proposal.published_at.or(current.published_at),
        },
        pool,
    )
    .await?;

    let applied_at = now_utc_rfc3339()?;
    sqlx::query(
        r#"
        UPDATE metadata_enrichment_proposals
        SET applied_at = ?
        WHERE id = ?
        "#,
    )
    .bind(applied_at)
    .bind(proposal.id)
    .execute(pool)
    .await
    .map_err(|error| {
        report_internal_error(
            "Unable to mark metadata enrichment proposal as applied",
            &error,
            "Unable to apply metadata enrichment proposal.",
        )
    })?;

    Ok(ApplyMetadataEnrichmentProposalResult {
        proposal_id: proposal.id,
        item: updated_item,
    })
}
