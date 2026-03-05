use crate::metadata_enrichment::{MetadataCandidate, MetadataProvider, ProviderFuture};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct GoogleBooksProvider {
    client: Client,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleBooksResponse {
    items: Option<Vec<GoogleBooksItem>>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleBooksItem {
    volume_info: GoogleVolumeInfo,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleVolumeInfo {
    title: Option<String>,
    authors: Option<Vec<String>>,
    language: Option<String>,
    published_date: Option<String>,
}

impl GoogleBooksProvider {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|error| format!("Unable to initialize Google Books client: {error}"))?;
        Ok(Self { client })
    }

    async fn query(&self, q: String, confidence: f64) -> Result<Option<MetadataCandidate>, String> {
        let mut endpoint = Url::parse("https://www.googleapis.com/books/v1/volumes")
            .map_err(|error| format!("Unable to build Google Books URL: {error}"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("q", &q)
            .append_pair("maxResults", "1")
            .append_pair("printType", "books")
            .append_pair("projection", "lite");
        let response = self
            .client
            .get(endpoint)
            .send()
            .await
            .map_err(|error| format!("Google Books request failed: {error}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Google Books returned HTTP {}",
                response.status().as_u16()
            ));
        }

        let payload = response
            .json::<GoogleBooksResponse>()
            .await
            .map_err(|error| format!("Google Books response parse failed: {error}"))?;

        let Some(first) = payload.items.and_then(|mut items| items.drain(..).next()) else {
            return Ok(None);
        };

        let raw_payload = serde_json::to_string(&first).unwrap_or_default();
        let volume_info = first.volume_info;
        let published_at = volume_info
            .published_date
            .and_then(normalize_published_date);
        let candidate = MetadataCandidate {
            title: volume_info.title,
            authors: volume_info.authors.unwrap_or_default(),
            language: volume_info.language,
            published_at,
            confidence,
            raw_payload,
        };

        Ok(Some(candidate))
    }
}

fn normalize_published_date(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() == 10 && is_iso_date(trimmed) {
        return Some(trimmed.to_string());
    }
    if trimmed.len() == 4 && trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Some(format!("{trimmed}-01-01"));
    }
    None
}

fn is_iso_date(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts
            .iter()
            .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
}

impl MetadataProvider for GoogleBooksProvider {
    fn provider_name(&self) -> &'static str {
        "google_books"
    }

    fn lookup_by_isbn<'a>(&'a self, isbn: &'a str) -> ProviderFuture<'a> {
        Box::pin(async move { self.query(format!("isbn:{isbn}"), 0.9).await })
    }

    fn lookup_by_title_author<'a>(
        &'a self,
        title: &'a str,
        authors: &'a [String],
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let mut query = format!("intitle:{title}");
            if let Some(author) = authors.first() {
                query.push_str(" inauthor:");
                query.push_str(author);
            }
            self.query(query, 0.72).await
        })
    }
}
