use crate::metadata_enrichment::{MetadataCandidate, MetadataProvider, ProviderFuture};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct OpenLibraryProvider {
    client: Client,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct OpenLibraryIsbnBook {
    title: Option<String>,
    authors: Option<Vec<OpenLibraryAuthorName>>,
    publish_date: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct OpenLibraryAuthorName {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenLibrarySearchResponse {
    docs: Option<Vec<OpenLibrarySearchDoc>>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct OpenLibrarySearchDoc {
    title: Option<String>,
    author_name: Option<Vec<String>>,
    first_publish_year: Option<i32>,
    language: Option<Vec<String>>,
}

impl OpenLibraryProvider {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|error| format!("Unable to initialize Open Library client: {error}"))?;
        Ok(Self { client })
    }

    async fn lookup_isbn(&self, isbn: &str) -> Result<Option<MetadataCandidate>, String> {
        let response = self
            .client
            .get(format!("https://openlibrary.org/isbn/{isbn}.json"))
            .send()
            .await
            .map_err(|error| format!("Open Library ISBN request failed: {error}"))?;

        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(format!(
                "Open Library ISBN returned HTTP {}",
                response.status().as_u16()
            ));
        }

        let payload = response
            .json::<OpenLibraryIsbnBook>()
            .await
            .map_err(|error| format!("Open Library ISBN parse failed: {error}"))?;
        let raw_payload = serde_json::to_string(&payload).unwrap_or_default();

        let authors = payload
            .authors
            .unwrap_or_default()
            .into_iter()
            .filter_map(|author| author.name)
            .collect::<Vec<_>>();

        let published_at = payload.publish_date.as_deref().and_then(parse_publish_date);

        Ok(Some(MetadataCandidate {
            title: payload.title,
            authors,
            language: None,
            published_at,
            confidence: 0.65,
            raw_payload,
        }))
    }

    async fn lookup_title_author(
        &self,
        title: &str,
        authors: &[String],
    ) -> Result<Option<MetadataCandidate>, String> {
        let author = authors.first().cloned().unwrap_or_default();
        let mut endpoint = Url::parse("https://openlibrary.org/search.json")
            .map_err(|error| format!("Unable to build Open Library URL: {error}"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("title", title)
            .append_pair("author", &author)
            .append_pair("limit", "1");
        let response = self
            .client
            .get(endpoint)
            .send()
            .await
            .map_err(|error| format!("Open Library search request failed: {error}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Open Library search returned HTTP {}",
                response.status().as_u16()
            ));
        }

        let payload = response
            .json::<OpenLibrarySearchResponse>()
            .await
            .map_err(|error| format!("Open Library search parse failed: {error}"))?;
        let Some(first) = payload.docs.and_then(|mut docs| docs.drain(..).next()) else {
            return Ok(None);
        };
        let raw_payload = serde_json::to_string(&first).unwrap_or_default();

        let published_at = first
            .first_publish_year
            .filter(|year| *year > 0)
            .map(|year| format!("{year:04}-01-01"));

        let language = first
            .language
            .and_then(|langs| langs.into_iter().next())
            .and_then(|lang| normalize_language(&lang));

        Ok(Some(MetadataCandidate {
            title: first.title,
            authors: first.author_name.unwrap_or_default(),
            language,
            published_at,
            confidence: 0.55,
            raw_payload,
        }))
    }
}

fn parse_publish_date(value: &str) -> Option<String> {
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

fn normalize_language(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() == 2 && trimmed.chars().all(|ch| ch.is_ascii_lowercase()) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

impl MetadataProvider for OpenLibraryProvider {
    fn provider_name(&self) -> &'static str {
        "open_library"
    }

    fn lookup_by_isbn<'a>(&'a self, isbn: &'a str) -> ProviderFuture<'a> {
        Box::pin(async move { self.lookup_isbn(isbn).await })
    }

    fn lookup_by_title_author<'a>(
        &'a self,
        title: &'a str,
        authors: &'a [String],
    ) -> ProviderFuture<'a> {
        Box::pin(async move { self.lookup_title_author(title, authors).await })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_publish_date;

    #[test]
    fn year_only_publish_date_is_normalized_to_iso_day() {
        assert_eq!(parse_publish_date("2024").as_deref(), Some("2024-01-01"));
    }

    #[test]
    fn full_iso_publish_date_is_kept() {
        assert_eq!(
            parse_publish_date("2024-12-31").as_deref(),
            Some("2024-12-31")
        );
    }
}
