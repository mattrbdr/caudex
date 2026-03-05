# Caudex

Caudex is a modern, open-source desktop ebook library manager designed as a high-performance alternative to Calibre.

## Vision

Calibre is feature-rich, but often heavy and dated for daily use.  
Caudex aims to keep advanced library workflows while providing:

- fast startup
- low memory footprint
- clean, modern desktop UX
- extensibility through plugins

## Product Goals

- Manage large ebook collections without performance friction
- Keep metadata quality high with automated enrichment + manual control
- Make full-text retrieval fast and reliable
- Support end-to-end workflows in one app (import, organize, convert, deliver)

## Planned Tech Stack

- `Rust` (core engine and performance-critical workflows)
- `Tauri v2` (cross-platform desktop shell)
- `Svelte 5` (UI layer)
- `Tantivy` (full-text indexing and search)
- `WebAssembly (WASM)` plugin runtime (sandboxed extension model)

## MVP Scope (Planned)

- Import for `EPUB`, `MOBI`, and `PDF`
- Automatic metadata enrichment (ISBN + Google Books)
- Fast full-text search across the library
- Library organization tools (tags, collections, metadata editing)
- Essential format conversion workflows
- Send-to-Kindle integration
- Built-in local HTTP content server
- Desktop releases for macOS and Windows

## Distribution Targets

- macOS via Homebrew
- Windows via winget

## Why Caudex

Most ebook managers force a trade-off between power and usability.  
Caudex is built to deliver both: advanced workflows for large libraries with a fast, modern desktop experience.

## Roadmap Snapshot

1. Finalize architecture and technical design
2. Build MVP core workflows (import, metadata, search, conversion, Kindle delivery)
3. Package first cross-platform desktop builds
4. Open early community feedback loop
5. Evolve toward a WASM plugin ecosystem

## License

Planned as open-source under the MIT license.
