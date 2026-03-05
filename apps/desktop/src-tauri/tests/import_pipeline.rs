use appsdesktop_lib::ingest::{
    start_bulk_import_with_pool, start_import_with_pool, BulkDuplicateMode, ImportFileStatus,
    StartBulkImportInput, StartImportInput,
};
use appsdesktop_lib::library::{insert_library, run_migrations};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

async fn setup_pool(path: PathBuf) -> sqlx::SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("pool should initialize")
}

fn write_pdf(path: &std::path::Path) {
    std::fs::write(
        path,
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\nxref\n0 1\n0000000000 65535 f \ntrailer\n<<>>\nstartxref\n0\n%%EOF",
    )
        .expect("pdf fixture should be created");
}

fn write_pdf_with_tag(path: &std::path::Path, tag: &str) {
    let content = format!(
        "%PDF-1.7\n1 0 obj\n<< /Producer ({tag}) >>\nendobj\nxref\n0 1\n0000000000 65535 f \ntrailer\n<<>>\nstartxref\n0\n%%EOF"
    );
    std::fs::write(path, content.as_bytes()).expect("pdf fixture should be created");
}

fn write_epub(path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("epub fixture should be created");
    let mut archive = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    archive
        .start_file("mimetype", options)
        .expect("mimetype entry should be created");
    archive
        .write_all(b"application/epub+zip")
        .expect("mimetype value should be written");
    archive
        .add_directory("META-INF/", options)
        .expect("meta-inf folder should be created");
    archive
        .start_file("META-INF/container.xml", options)
        .expect("container entry should be created");
    archive
        .write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles/></container>"#,
        )
        .expect("container payload should be written");
    archive.finish().expect("epub archive should finalize");
}

fn write_mobi(path: &std::path::Path) {
    let mut bytes = vec![0_u8; 96];
    bytes[60..68].copy_from_slice(b"BOOKMOBI");
    std::fs::write(path, bytes).expect("mobi fixture should be created");
}

#[tokio::test]
async fn start_import_ingests_supported_formats_and_queues_index_work() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let pdf = temp.path().join("book.pdf");
    let epub = temp.path().join("book.epub");
    let mobi = temp.path().join("book.mobi");
    write_pdf(&pdf);
    write_epub(&epub);
    write_mobi(&mobi);

    let result = start_import_with_pool(
        StartImportInput {
            paths: vec![
                pdf.to_string_lossy().to_string(),
                epub.to_string_lossy().to_string(),
                mobi.to_string_lossy().to_string(),
            ],
        },
        &pool,
    )
    .await
    .expect("import should succeed");

    assert_eq!(result.success_count, 3);
    assert_eq!(result.failed_count, 0);
    assert_eq!(result.skipped_count, 0);
    assert!(result
        .items
        .iter()
        .all(|item| item.status == ImportFileStatus::Success));

    let item_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM library_items")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");
    assert_eq!(item_count, 3);

    let index_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM index_work_units")
        .fetch_one(&pool)
        .await
        .expect("index count query should succeed");
    assert_eq!(index_count, 3);
}

#[tokio::test]
async fn start_import_flags_unsupported_or_corrupt_without_blocking_successes() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let valid_pdf = temp.path().join("ok.pdf");
    let unsupported = temp.path().join("notes.txt");
    let corrupt_mobi = temp.path().join("bad.mobi");

    write_pdf(&valid_pdf);
    std::fs::write(&unsupported, b"plain text").expect("unsupported fixture should be created");
    std::fs::write(&corrupt_mobi, b"not-a-valid-mobi").expect("corrupt fixture should be created");

    let result = start_import_with_pool(
        StartImportInput {
            paths: vec![
                valid_pdf.to_string_lossy().to_string(),
                unsupported.to_string_lossy().to_string(),
                corrupt_mobi.to_string_lossy().to_string(),
            ],
        },
        &pool,
    )
    .await
    .expect("import should complete with partial failures");

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failed_count, 2);
    assert!(result
        .items
        .iter()
        .any(|item| item.status == ImportFileStatus::Success));
    assert!(result
        .items
        .iter()
        .filter(|item| item.status == ImportFileStatus::Failed)
        .all(|item| item.error_message.is_some()));
}

#[tokio::test]
async fn start_import_rejects_corrupt_epub_payload() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let valid_mobi = temp.path().join("ok.mobi");
    let corrupt_epub = temp.path().join("bad.epub");
    write_mobi(&valid_mobi);
    std::fs::write(&corrupt_epub, b"PK\x03\x04not-really-an-epub")
        .expect("corrupt epub fixture should be created");

    let result = start_import_with_pool(
        StartImportInput {
            paths: vec![
                valid_mobi.to_string_lossy().to_string(),
                corrupt_epub.to_string_lossy().to_string(),
            ],
        },
        &pool,
    )
    .await
    .expect("import should complete with partial failures");

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failed_count, 1);
    assert!(result.items.iter().any(|item| {
        item.source_path.ends_with("bad.epub")
            && item.status == ImportFileStatus::Failed
            && item.error_code.as_deref() == Some("corrupt_file")
    }));
}

#[tokio::test]
async fn start_import_skips_duplicate_paths_in_same_request() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let pdf = temp.path().join("dup.pdf");
    write_pdf(&pdf);
    let same_path = pdf.to_string_lossy().to_string();

    let result = start_import_with_pool(
        StartImportInput {
            paths: vec![same_path.clone(), same_path],
        },
        &pool,
    )
    .await
    .expect("import should complete");

    assert_eq!(result.success_count, 1);
    assert_eq!(result.skipped_count, 1);
    assert!(result
        .items
        .iter()
        .any(|item| item.status == ImportFileStatus::Skipped));
}

#[tokio::test]
async fn bulk_import_recurses_and_skips_same_content_duplicates() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let root = temp.path().join("bulk");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).expect("directories should be created");

    let pdf_a = root.join("a.pdf");
    let pdf_b = nested.join("b.pdf");
    let mobi = nested.join("c.mobi");
    let unsupported = nested.join("notes.txt");

    write_pdf(&pdf_a);
    std::fs::copy(&pdf_a, &pdf_b).expect("duplicate content fixture should be copied");
    write_mobi(&mobi);
    std::fs::write(&unsupported, b"plain text").expect("unsupported fixture should be created");

    let result = start_bulk_import_with_pool(
        StartBulkImportInput {
            root_path: root.to_string_lossy().to_string(),
            duplicate_mode: BulkDuplicateMode::SkipDuplicate,
            dry_run: false,
        },
        &pool,
    )
    .await
    .expect("bulk import should complete");

    assert_eq!(result.scanned_count, 4);
    assert_eq!(result.success_count, 2);
    assert_eq!(result.skipped_count, 1);
    assert_eq!(result.failed_count, 1);
    assert!(result.items.iter().any(|item| {
        item.status == ImportFileStatus::Skipped
            && item.dedupe_decision.as_deref() == Some("skip_duplicate")
    }));
}

#[tokio::test]
async fn bulk_import_force_import_keeps_same_content_different_name() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let root = temp.path().join("bulk-force");
    std::fs::create_dir_all(&root).expect("directory should be created");
    let pdf_a = root.join("a.pdf");
    let pdf_b = root.join("b.pdf");
    write_pdf(&pdf_a);
    std::fs::copy(&pdf_a, &pdf_b).expect("duplicate content fixture should be copied");

    let result = start_bulk_import_with_pool(
        StartBulkImportInput {
            root_path: root.to_string_lossy().to_string(),
            duplicate_mode: BulkDuplicateMode::ForceImport,
            dry_run: false,
        },
        &pool,
    )
    .await
    .expect("bulk import should complete");

    assert_eq!(result.success_count, 2);
    assert_eq!(result.skipped_count, 0);
    assert!(result.items.iter().any(|item| {
        item.status == ImportFileStatus::Success
            && item.dedupe_decision.as_deref() == Some("force_import")
    }));
}

#[tokio::test]
async fn bulk_import_merge_metadata_updates_existing_item() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let root = temp.path().join("bulk-merge");
    std::fs::create_dir_all(&root).expect("directory should be created");
    let short_name = root.join("a.pdf");
    let long_name = root.join("very-descriptive-title.pdf");
    write_pdf(&short_name);
    std::fs::copy(&short_name, &long_name).expect("duplicate content fixture should be copied");

    let result = start_bulk_import_with_pool(
        StartBulkImportInput {
            root_path: root.to_string_lossy().to_string(),
            duplicate_mode: BulkDuplicateMode::MergeMetadata,
            dry_run: false,
        },
        &pool,
    )
    .await
    .expect("bulk import should complete");

    assert_eq!(result.success_count, 2);
    assert_eq!(result.skipped_count, 0);
    assert!(result.items.iter().any(|item| {
        item.status == ImportFileStatus::Success
            && item.dedupe_decision.as_deref() == Some("merge_metadata")
    }));

    let item_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM library_items")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");
    assert_eq!(item_count, 1);

    let title: String = sqlx::query_scalar("SELECT title FROM library_items LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("title query should succeed");
    assert_eq!(title, "very-descriptive-title");
}

#[tokio::test]
async fn bulk_import_same_filename_with_different_content_is_not_duplicate() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let root = temp.path().join("bulk-name");
    let dir_a = root.join("a");
    let dir_b = root.join("b");
    std::fs::create_dir_all(&dir_a).expect("directory should be created");
    std::fs::create_dir_all(&dir_b).expect("directory should be created");
    let file_a = dir_a.join("same.pdf");
    let file_b = dir_b.join("same.pdf");
    write_pdf_with_tag(&file_a, "A");
    write_pdf_with_tag(&file_b, "B");

    let result = start_bulk_import_with_pool(
        StartBulkImportInput {
            root_path: root.to_string_lossy().to_string(),
            duplicate_mode: BulkDuplicateMode::SkipDuplicate,
            dry_run: false,
        },
        &pool,
    )
    .await
    .expect("bulk import should complete");

    assert_eq!(result.success_count, 2);
    assert_eq!(result.skipped_count, 0);
}

#[tokio::test]
async fn bulk_import_reports_scan_counts_for_medium_tree() {
    let temp = tempdir().expect("temp dir should be created");
    let pool = setup_pool(temp.path().join("caudex.db")).await;
    run_migrations(&pool)
        .await
        .expect("migrations should apply");

    insert_library(
        &pool,
        "Main Library",
        temp.path().to_string_lossy().as_ref(),
        "2026-03-05T10:30:00Z",
    )
    .await
    .expect("library insert should succeed");

    let root = temp.path().join("bulk-medium");
    std::fs::create_dir_all(&root).expect("directory should be created");

    for idx in 0..60 {
        let file = root.join(format!("doc-{idx}.pdf"));
        write_pdf_with_tag(&file, &format!("pdf-{idx}"));
    }
    for idx in 0..60 {
        let file = root.join(format!("book-{idx}.mobi"));
        write_mobi(&file);
    }

    let result = start_bulk_import_with_pool(
        StartBulkImportInput {
            root_path: root.to_string_lossy().to_string(),
            duplicate_mode: BulkDuplicateMode::SkipDuplicate,
            dry_run: true,
        },
        &pool,
    )
    .await
    .expect("bulk import should complete");

    assert_eq!(result.scanned_count, 120);
    assert_eq!(result.processed_count, 120);
}
