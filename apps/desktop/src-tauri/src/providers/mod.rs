use crate::metadata_enrichment::MetadataProvider;

pub mod google_books;
pub mod open_library;

pub fn default_providers() -> Result<Vec<Box<dyn MetadataProvider>>, String> {
    Ok(vec![
        Box::new(google_books::GoogleBooksProvider::new()?),
        Box::new(open_library::OpenLibraryProvider::new()?),
    ])
}
