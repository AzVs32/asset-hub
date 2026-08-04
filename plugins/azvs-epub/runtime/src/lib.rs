use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
const READ_CHUNK_BYTES: u64 = 1024 * 1024;
const MAX_EPUB_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MARKUP_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TITLE_DOCUMENT_BYTES: u64 = 256 * 1024;
const MAX_STYLESHEET_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 1024 * 1024;
const MAX_CHAPTER_ASSETS_BYTES: u64 = 2 * 1024 * 1024;
const MAX_STYLESHEET_ASSET_BYTES: u64 = 512 * 1024;
const MAX_CHAPTER_STYLES_BYTES: usize = 1024 * 1024;
const MAX_CACHED_BOOK_BYTES: usize = 48 * 1024 * 1024;
const MAX_BOOK_CACHE_BYTES: usize = 96 * 1024 * 1024;
const MAX_COVER_CACHE_BYTES: usize = 48 * 1024 * 1024;
const BOOK_CACHE_CAPACITY: usize = 3;
const COVER_CACHE_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
struct ManifestItem {
    href: String,
    media_type: String,
    properties: String,
}

#[derive(Debug, Clone)]
struct Package {
    title: String,
    author: Option<String>,
    manifest: HashMap<String, ManifestItem>,
    spine: Vec<String>,
    cover_item_id: Option<String>,
    guide_cover_href: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ChapterSummary {
    index: usize,
    title: String,
    #[serde(skip)]
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChapterContent {
    index: usize,
    title: String,
    html: String,
    styles: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BookIndex {
    title: String,
    author: Option<String>,
    cover: Option<String>,
    chapters: Vec<ChapterSummary>,
    initial_chapter: Option<ChapterContent>,
}

#[derive(Clone)]
struct CachedBook {
    key: String,
    bytes: Arc<Vec<u8>>,
    opf_path: String,
    package: Package,
    index: BookIndex,
}

#[derive(Clone)]
struct CachedCover {
    key: String,
    cover: Option<String>,
}

mod archive;
mod cache;
mod handler;
mod metadata;
mod render;
mod sanitize;

pub use handler::{render_epub, render_epub_thumbnail};

pub(crate) use archive::*;
pub(crate) use cache::*;
pub(crate) use metadata::*;
pub(crate) use render::*;
pub(crate) use sanitize::*;

#[cfg(test)]
mod tests;
