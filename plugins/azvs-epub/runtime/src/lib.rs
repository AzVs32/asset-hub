use ammonia::Builder;
use asset_plugin_api::{
    JsonView, MediaView, PluginActionOutput, PluginActionRequest, PluginContentEncoding,
    PluginFrameView, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
#[cfg(target_arch = "wasm32")]
use extism_pdk::host_fn;
use extism_pdk::{Error, FnResult, plugin_fn};
use lol_html::{RewriteStrSettings, element, rewrite_str};
use roxmltree::{Document, Node};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex, OnceLock};
use zip::ZipArchive;

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

#[cfg(target_arch = "wasm32")]
#[host_fn]
extern "ExtismHost" {
    fn asset_hub_content_open(reference: String) -> String;
    fn asset_hub_content_size(handle: String) -> u64;
    fn asset_hub_content_read(handle: String, offset: u64, length: u64) -> Vec<u8>;
    fn asset_hub_content_close(handle: String);
}

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

static BOOK_CACHE: OnceLock<Mutex<VecDeque<Arc<CachedBook>>>> = OnceLock::new();
static COVER_CACHE: OnceLock<Mutex<VecDeque<CachedCover>>> = OnceLock::new();

#[plugin_fn]
pub fn render_epub(input: String) -> FnResult<String> {
    render_epub_payload(input)
}

#[plugin_fn]
pub fn render_epub_cover(input: String) -> FnResult<String> {
    render_epub_cover_payload(input)
}

fn render_epub_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    let operation = request
        .input
        .get("operation")
        .and_then(|value| value.as_str());
    let output = match operation {
        Some("load") => {
            let book = load_cached_book(&request)?;
            let mut index = book.index.clone();
            if !index.chapters.is_empty() {
                index.initial_chapter = render_chapter(&book, 0).ok();
            }
            PluginActionOutput::new(PluginView::Json(JsonView {
                data: serde_json::to_value(index)?,
            }))
        }
        Some("chapter") => {
            let index = request
                .input
                .get("index")
                .and_then(|value| value.as_u64())
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| Error::msg("missing or invalid chapter index"))?;
            let book = load_cached_book(&request)?;
            let chapter = render_chapter(&book, index)?;
            PluginActionOutput::new(PluginView::Json(JsonView {
                data: serde_json::to_value(chapter)?,
            }))
        }
        Some(_) => return Err(Error::msg("unsupported EPUB operation").into()),
        None => {
            let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
                "resource_id": &request.resource.id,
                "resource_name": &request.resource.name,
                "action": &request.action,
            }))?);
            PluginActionOutput::new(PluginView::PluginFrame(PluginFrameView {
                title: Some(request.resource.name.clone()),
                url: format!("index.html#payload={payload}"),
            }))
        }
    };

    Ok(serde_json::to_string(&output)?)
}

fn render_epub_cover_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    let key = resource_cache_key(&request);
    let cover = if let Some(cover) = cached_cover(&key) {
        cover
    } else if let Some(book) = cached_book(&key) {
        book.index.cover.clone()
    } else {
        let epub = epub_content_bytes(&request)?;
        let cover = render_epub_cover_bytes(&epub)?;
        store_cover(key, cover.clone());
        cover
    };
    let view = match cover {
        Some(cover) => PluginView::Media(cover_media_view(&request.resource.name, &cover)?),
        None => PluginView::Json(JsonView {
            data: serde_json::Value::Null,
        }),
    };
    Ok(serde_json::to_string(&PluginActionOutput::new(view))?)
}

fn resource_cache_key(request: &PluginActionRequest) -> String {
    let mut key = format!(
        "{}:{}:{}",
        request.resource.id,
        request.resource.updated_at,
        request
            .resource
            .content
            .as_ref()
            .map_or(0, |value| value.size)
    );
    if let Some(checksum) = request
        .resource
        .content
        .as_ref()
        .and_then(|content| content.checksum.first())
    {
        key.push(':');
        key.push_str(&checksum.kind);
        key.push(':');
        key.push_str(&checksum.value);
    }
    key
}

fn cached_book(key: &str) -> Option<Arc<CachedBook>> {
    let cache = BOOK_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let mut cache = cache.lock().ok()?;
    let position = cache.iter().position(|entry| entry.key == key)?;
    let book = cache.remove(position)?;
    cache.push_front(book.clone());
    Some(book)
}

fn load_cached_book(request: &PluginActionRequest) -> FnResult<Arc<CachedBook>> {
    let key = resource_cache_key(request);
    if let Some(book) = cached_book(&key) {
        return Ok(book);
    }

    let bytes = Arc::new(epub_content_bytes(request)?);
    let book = Arc::new(parse_book(key, bytes.clone())?);
    if bytes.len() <= MAX_CACHED_BOOK_BYTES {
        let cache = BOOK_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
        if let Ok(mut cache) = cache.lock() {
            cache.retain(|entry| entry.key != book.key);
            cache.push_front(book.clone());
            while cache.len() > BOOK_CACHE_CAPACITY
                || cache.iter().map(|entry| entry.bytes.len()).sum::<usize>() > MAX_BOOK_CACHE_BYTES
            {
                cache.pop_back();
            }
        }
    }
    store_cover(book.key.clone(), book.index.cover.clone());
    Ok(book)
}

fn cached_cover(key: &str) -> Option<Option<String>> {
    let cache = COVER_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let mut cache = cache.lock().ok()?;
    let position = cache.iter().position(|entry| entry.key == key)?;
    let entry = cache.remove(position)?;
    let cover = entry.cover.clone();
    cache.push_front(entry);
    Some(cover)
}

fn store_cover(key: String, cover: Option<String>) {
    let cache = COVER_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|entry| entry.key != key);
        cache.push_front(CachedCover { key, cover });
        while cache.len() > COVER_CACHE_CAPACITY
            || cache
                .iter()
                .map(|entry| entry.cover.as_ref().map_or(0, String::len))
                .sum::<usize>()
                > MAX_COVER_CACHE_BYTES
        {
            cache.pop_back();
        }
    }
}

fn epub_content_bytes(input: &PluginActionRequest) -> FnResult<Vec<u8>> {
    if let Some(content) = &input.content {
        if content.encoding != PluginContentEncoding::Base64 {
            return Err(Error::msg("unsupported content encoding").into());
        }
        let bytes = STANDARD.decode(&content.data)?;
        if bytes.len() as u64 > MAX_EPUB_BYTES {
            return Err(Error::msg("EPUB exceeds the 128 MiB plugin limit").into());
        }
        return Ok(bytes);
    }

    let content_ref = input
        .content_ref
        .as_ref()
        .ok_or_else(|| Error::msg("missing EPUB content payload"))?;
    if content_ref.encoding != PluginContentEncoding::Handle {
        return Err(Error::msg("unsupported content reference encoding").into());
    }
    read_content_reference(&content_ref.reference)
}

#[cfg(target_arch = "wasm32")]
fn read_content_reference(reference: &str) -> FnResult<Vec<u8>> {
    let handle = unsafe { asset_hub_content_open(reference.to_string()) }?;
    let result = (|| {
        let size = unsafe { asset_hub_content_size(handle.clone()) }?;
        if size > MAX_EPUB_BYTES {
            return Err(Error::msg("EPUB exceeds the 128 MiB plugin limit").into());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        let mut offset = 0;
        while offset < size {
            let length = (size - offset).min(READ_CHUNK_BYTES);
            let chunk = unsafe { asset_hub_content_read(handle.clone(), offset, length) }?;
            if chunk.is_empty() || chunk.len() as u64 > length {
                return Err(Error::msg("invalid content chunk returned by host").into());
            }
            offset += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })();
    unsafe { asset_hub_content_close(handle) }?;
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_reference(_reference: &str) -> FnResult<Vec<u8>> {
    Err(Error::msg("content references are only available in the wasm host").into())
}

fn parse_book(key: String, bytes: Arc<Vec<u8>>) -> FnResult<CachedBook> {
    let mut archive = open_archive(&bytes)?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text_limited(&mut archive, &opf_path, MAX_MARKUP_BYTES)?;
    let package = parse_package(&opf, &opf_path)?;
    let cover = find_cover_data_url(&mut archive, &package, &opf_path);
    let chapters = read_chapter_summaries(&mut archive, &package, &opf_path);
    let index = BookIndex {
        title: package.title.clone(),
        author: package.author.clone(),
        cover,
        chapters,
        initial_chapter: None,
    };
    Ok(CachedBook {
        key,
        bytes,
        opf_path,
        package,
        index,
    })
}

fn open_archive(bytes: &[u8]) -> FnResult<ZipArchive<Cursor<&[u8]>>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(Error::msg("EPUB contains too many ZIP entries").into());
    }
    let mut total = 0_u64;
    for index in 0..archive.len() {
        total = total
            .checked_add(archive.by_index(index)?.size())
            .ok_or_else(|| Error::msg("EPUB uncompressed size overflow"))?;
    }
    if total > MAX_UNCOMPRESSED_BYTES {
        return Err(Error::msg("EPUB uncompressed content exceeds 512 MiB").into());
    }
    Ok(archive)
}

fn render_epub_cover_bytes(epub: &[u8]) -> FnResult<Option<String>> {
    let mut archive = open_archive(epub)?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text_limited(&mut archive, &opf_path, MAX_MARKUP_BYTES)?;
    let package = parse_package(&opf, &opf_path)?;
    Ok(find_cover_data_url(&mut archive, &package, &opf_path))
}

fn cover_media_view(title: &str, data_url: &str) -> FnResult<MediaView> {
    let (mime_type, data) = data_url
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
        .ok_or_else(|| Error::msg("invalid EPUB cover data URL"))?;
    Ok(MediaView {
        mime_type: mime_type.to_string(),
        title: Some(title.to_string()),
        encoding: PluginContentEncoding::Base64,
        data: data.to_string(),
    })
}

fn find_opf_path(archive: &mut ZipArchive<Cursor<&[u8]>>) -> FnResult<String> {
    let container = read_zip_text_limited(archive, "META-INF/container.xml", MAX_MARKUP_BYTES)?;
    let doc = Document::parse(&container)?;
    let path = doc
        .descendants()
        .find(|node| local_name(node.tag_name().name()) == "rootfile")
        .and_then(|node| node.attribute("full-path"))
        .ok_or_else(|| Error::msg("EPUB container.xml does not contain a rootfile"))?;
    safe_zip_path(path).ok_or_else(|| Error::msg("EPUB rootfile has an unsafe path").into())
}

fn parse_package(opf: &str, opf_path: &str) -> FnResult<Package> {
    let doc = Document::parse(opf)?;
    let title = doc
        .descendants()
        .find(|node| local_name(node.tag_name().name()) == "title")
        .and_then(|node| node.text())
        .map(clean_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| file_stem(opf_path).unwrap_or("Untitled").to_string());
    let author = doc
        .descendants()
        .find(|node| local_name(node.tag_name().name()) == "creator")
        .and_then(|node| node.text())
        .map(clean_text)
        .filter(|value| !value.is_empty());

    let mut manifest = HashMap::new();
    for node in doc
        .descendants()
        .filter(|node| local_name(node.tag_name().name()) == "item")
    {
        let (Some(id), Some(href)) = (node.attribute("id"), node.attribute("href")) else {
            continue;
        };
        if resolve_path(opf_path, href).is_none() {
            continue;
        }
        manifest.insert(
            id.to_string(),
            ManifestItem {
                href: href.to_string(),
                media_type: node.attribute("media-type").unwrap_or("").to_string(),
                properties: node.attribute("properties").unwrap_or("").to_string(),
            },
        );
    }
    let spine = doc
        .descendants()
        .filter(|node| local_name(node.tag_name().name()) == "itemref")
        .filter_map(|node| node.attribute("idref").map(str::to_string))
        .collect();
    let cover_item_id = doc
        .descendants()
        .find(|node| {
            local_name(node.tag_name().name()) == "meta"
                && node
                    .attribute("name")
                    .is_some_and(|name| name.eq_ignore_ascii_case("cover"))
        })
        .and_then(|node| node.attribute("content"))
        .map(str::to_string);
    let guide_cover_href = doc
        .descendants()
        .find(|node| {
            local_name(node.tag_name().name()) == "reference"
                && node
                    .attribute("type")
                    .is_some_and(|value| value.eq_ignore_ascii_case("cover"))
        })
        .and_then(|node| node.attribute("href"))
        .map(str::to_string);
    Ok(Package {
        title,
        author,
        manifest,
        spine,
        cover_item_id,
        guide_cover_href,
    })
}

fn find_cover_data_url(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
) -> Option<String> {
    let candidates = package
        .manifest
        .values()
        .filter(|item| {
            item.properties
                .split_whitespace()
                .any(|value| value == "cover-image")
        })
        .chain(
            package
                .cover_item_id
                .as_ref()
                .and_then(|id| package.manifest.get(id)),
        )
        .chain(package.manifest.values().filter(|item| {
            item.href.to_ascii_lowercase().contains("cover")
                && item.media_type.starts_with("image/")
        }));
    for item in candidates {
        if let Some(data_url) = image_item_data_url(archive, opf_path, item) {
            return Some(data_url);
        }
    }
    if let Some(href) = &package.guide_cover_href {
        return cover_from_guide_href(archive, opf_path, href, package);
    }
    None
}

fn image_item_data_url(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    opf_path: &str,
    item: &ManifestItem,
) -> Option<String> {
    if !item.media_type.starts_with("image/") {
        return None;
    }
    let path = resolve_path(opf_path, &item.href)?;
    let bytes = read_zip_bytes_limited(archive, &path, MAX_COVER_BYTES).ok()?;
    Some(data_url(&item.media_type, &bytes))
}

fn cover_from_guide_href(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    opf_path: &str,
    href: &str,
    package: &Package,
) -> Option<String> {
    let href_path = strip_fragment(href);
    let item = package
        .manifest
        .values()
        .find(|item| strip_fragment(&item.href) == href_path)?;
    if let Some(data_url) = image_item_data_url(archive, opf_path, item) {
        return Some(data_url);
    }
    let page_path = resolve_path(opf_path, &item.href)?;
    let page = read_zip_text_limited(archive, &page_path, MAX_MARKUP_BYTES).ok()?;
    let image_href = first_image_src(&page)?;
    let image_path = resolve_path(&page_path, &image_href)?;
    asset_data_url(archive, package, opf_path, &image_path, MAX_COVER_BYTES).ok()
}

fn first_image_src(raw: &str) -> Option<String> {
    let doc = Document::parse(raw).ok()?;
    doc.descendants()
        .find(|node| {
            matches!(
                local_name(node.tag_name().name()),
                "img" | "image" | "svg:image"
            )
        })
        .and_then(|node| {
            node.attribute("src")
                .or_else(|| node.attribute("href"))
                .or_else(|| node.attribute(("http://www.w3.org/1999/xlink", "href")))
        })
        .map(str::to_string)
}

fn read_chapter_summaries(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
) -> Vec<ChapterSummary> {
    let titles = read_toc_titles(archive, package, opf_path);
    let mut chapters = Vec::new();
    for item in package
        .spine
        .iter()
        .filter_map(|id| package.manifest.get(id))
        .filter(|item| is_markup(item))
    {
        let Some(path) = resolve_path(opf_path, &item.href) else {
            continue;
        };
        let title = titles
            .get(&path)
            .cloned()
            .or_else(|| {
                read_zip_text_limited(archive, &path, MAX_TITLE_DOCUMENT_BYTES)
                    .ok()
                    .and_then(|raw| chapter_document_title(&raw))
            })
            .or_else(|| file_stem(&item.href).map(clean_filename))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Untitled chapter".to_string());
        chapters.push(ChapterSummary {
            index: chapters.len(),
            title,
            path,
        });
    }
    chapters
}

fn chapter_document_title(raw: &str) -> Option<String> {
    let doc = Document::parse(raw).ok()?;
    for tag in ["h1", "h2", "h3"] {
        if let Some(title) = doc
            .descendants()
            .find(|node| local_name(node.tag_name().name()) == tag)
            .and_then(node_text)
            .map(|value| clean_text(&value))
            .filter(|value| is_usable_chapter_title(value))
        {
            return Some(title);
        }
    }
    if let Some(title) = doc
        .descendants()
        .find(|node| local_name(node.tag_name().name()) == "title")
        .and_then(node_text)
        .map(|value| clean_text(&value))
        .filter(|value| is_usable_chapter_title(value))
    {
        return Some(title);
    }
    let body = doc
        .descendants()
        .find(|node| local_name(node.tag_name().name()) == "body")?;
    body.children()
        .filter(Node::is_element)
        .take(3)
        .find_map(|block| {
            block
                .descendants()
                .find(|node| matches!(local_name(node.tag_name().name()), "b" | "strong"))
                .and_then(node_text)
                .map(|value| clean_text(&value))
                .filter(|value| is_usable_chapter_title(value))
        })
}

fn is_usable_chapter_title(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    !value.is_empty()
        && value.chars().count() <= 160
        && !matches!(
            lower.as_str(),
            "unknown" | "untitled" | "untitled chapter" | "chapter" | "document"
        )
}

fn is_markup(item: &ManifestItem) -> bool {
    item.media_type == "application/xhtml+xml"
        || item.media_type == "text/html"
        || strip_fragment(&item.href).ends_with(".xhtml")
        || strip_fragment(&item.href).ends_with(".html")
}

fn read_toc_titles(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    if let Some(ncx) = package
        .manifest
        .values()
        .find(|item| item.media_type == "application/x-dtbncx+xml" || item.href.ends_with(".ncx"))
        && let Some(path) = resolve_path(opf_path, &ncx.href)
        && let Ok(raw) = read_zip_text_limited(archive, &path, MAX_MARKUP_BYTES)
    {
        titles.extend(parse_ncx_titles(&raw, &path));
    }
    if let Some(nav) = package.manifest.values().find(|item| {
        item.properties
            .split_whitespace()
            .any(|value| value == "nav")
            || item.href.ends_with("nav.xhtml")
            || item.href.ends_with("nav.html")
    }) && let Some(path) = resolve_path(opf_path, &nav.href)
        && let Ok(raw) = read_zip_text_limited(archive, &path, MAX_MARKUP_BYTES)
    {
        titles.extend(parse_nav_titles(&raw, &path));
    }
    titles
}

fn parse_ncx_titles(raw: &str, ncx_path: &str) -> HashMap<String, String> {
    let Ok(doc) = Document::parse(raw) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();
    for point in doc
        .descendants()
        .filter(|node| local_name(node.tag_name().name()) == "navPoint")
    {
        let title = point
            .descendants()
            .find(|node| local_name(node.tag_name().name()) == "text")
            .and_then(node_text)
            .map(|value| clean_text(&value));
        let src = point
            .descendants()
            .find(|node| local_name(node.tag_name().name()) == "content")
            .and_then(|node| node.attribute("src"));
        if let (Some(title), Some(src)) = (title, src)
            && let Some(path) = resolve_path(ncx_path, src)
        {
            titles.entry(path).or_insert(title);
        }
    }
    titles
}

fn parse_nav_titles(raw: &str, nav_path: &str) -> HashMap<String, String> {
    let Ok(doc) = Document::parse(raw) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();
    for link in doc
        .descendants()
        .filter(|node| local_name(node.tag_name().name()) == "a")
    {
        if let (Some(href), Some(title)) = (link.attribute("href"), node_text(link))
            && let Some(path) = resolve_path(nav_path, href)
        {
            titles.entry(path).or_insert_with(|| clean_text(&title));
        }
    }
    titles
}

fn render_chapter(book: &CachedBook, index: usize) -> FnResult<ChapterContent> {
    let chapter = book
        .index
        .chapters
        .get(index)
        .ok_or_else(|| Error::msg("chapter index is out of range"))?;
    let mut archive = open_archive(&book.bytes)?;
    let raw = read_zip_text_limited(&mut archive, &chapter.path, MAX_MARKUP_BYTES)?;
    let chapter_paths = book
        .index
        .chapters
        .iter()
        .map(|chapter| (chapter.path.clone(), chapter.index))
        .collect::<HashMap<_, _>>();
    let styles = extract_styles(
        &mut archive,
        &book.package,
        &book.opf_path,
        &chapter.path,
        &raw,
    );
    let rewritten = rewrite_chapter_html(
        &mut archive,
        &book.package,
        &book.opf_path,
        &chapter.path,
        &chapter_paths,
        &raw,
    )
    .unwrap_or(raw);
    Ok(ChapterContent {
        index,
        title: chapter.title.clone(),
        html: sanitize_html(&rewritten),
        styles,
    })
}

fn rewrite_chapter_html(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
    chapter_path: &str,
    chapter_paths: &HashMap<String, usize>,
    raw: &str,
) -> FnResult<String> {
    let mut embedded_bytes = 0_u64;
    let rewritten = rewrite_str(
        raw,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*", |element| {
                let tag = element.tag_name().to_ascii_lowercase();
                if tag == "head" {
                    element.remove();
                    return Ok(());
                }
                if tag == "html" || tag == "body" {
                    element.remove_and_keep_content();
                    return Ok(());
                }
                element.remove_attribute("style");
                for attr in ["src", "poster"] {
                    if let Some(value) = element.get_attribute(attr) {
                        if let Some(rewritten) = rewrite_asset_reference(
                            archive,
                            package,
                            opf_path,
                            chapter_path,
                            &value,
                            &mut embedded_bytes,
                        ) {
                            element.set_attribute(attr, &rewritten)?;
                        } else {
                            element.remove_attribute(attr);
                        }
                    }
                }
                for attr in ["href", "xlink:href"] {
                    let Some(value) = element.get_attribute(attr) else {
                        continue;
                    };
                    let rewritten = if tag == "a" {
                        rewrite_link(chapter_path, chapter_paths, &value)
                    } else if tag == "image" || tag == "use" {
                        rewrite_asset_reference(
                            archive,
                            package,
                            opf_path,
                            chapter_path,
                            &value,
                            &mut embedded_bytes,
                        )
                    } else {
                        None
                    };
                    if let Some(rewritten) = rewritten {
                        element.set_attribute(attr, &rewritten)?;
                    } else {
                        element.remove_attribute(attr);
                    }
                }
                if let Some(srcset) = element.get_attribute("srcset") {
                    let rewritten = rewrite_srcset(
                        archive,
                        package,
                        opf_path,
                        chapter_path,
                        &srcset,
                        &mut embedded_bytes,
                    );
                    if rewritten.is_empty() {
                        element.remove_attribute("srcset");
                    } else {
                        element.set_attribute("srcset", &rewritten)?;
                    }
                }
                Ok(())
            })],
            strict: true,
            ..RewriteStrSettings::new()
        },
    )
    .map_err(|error| Error::msg(format!("unable to parse chapter HTML: {error}")))?;
    Ok(rewritten)
}

fn rewrite_link(
    chapter_path: &str,
    chapter_paths: &HashMap<String, usize>,
    value: &str,
) -> Option<String> {
    let value = value.trim();
    if value.starts_with('#') {
        return Some(value.to_string());
    }
    if is_external_link(value) {
        return Some(value.to_string());
    }
    let path = resolve_path(chapter_path, value)?;
    let fragment = value.split_once('#').map(|(_, fragment)| fragment);
    let target = chapter_paths.get(&path)?;
    Some(match fragment {
        Some(fragment) if !fragment.is_empty() => {
            format!("epub://chapter/{target}#{fragment}")
        }
        _ => format!("epub://chapter/{target}"),
    })
}

fn rewrite_asset_reference(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
    base_path: &str,
    value: &str,
    embedded_bytes: &mut u64,
) -> Option<String> {
    let value = value.trim();
    if value.starts_with('#') {
        return Some(value.to_string());
    }
    if is_safe_data_url(value) {
        return Some(value.to_string());
    }
    if has_uri_scheme(value) || value.starts_with("//") {
        return None;
    }
    let path = resolve_path(base_path, value)?;
    let bytes = read_zip_bytes_limited(archive, &path, MAX_ASSET_BYTES).ok()?;
    *embedded_bytes = embedded_bytes.checked_add(bytes.len() as u64)?;
    if *embedded_bytes > MAX_CHAPTER_ASSETS_BYTES {
        return None;
    }
    let mime = manifest_mime(package, opf_path, &path).unwrap_or_else(|| mime_from_path(&path));
    if !is_embeddable_mime(&mime) {
        return None;
    }
    Some(data_url(&mime, &bytes))
}

fn rewrite_srcset(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
    base_path: &str,
    value: &str,
    embedded_bytes: &mut u64,
) -> String {
    value
        .split(',')
        .filter_map(|candidate| {
            let candidate = candidate.trim();
            let mut parts = candidate.split_whitespace();
            let url = parts.next()?;
            let rewritten = rewrite_asset_reference(
                archive,
                package,
                opf_path,
                base_path,
                url,
                embedded_bytes,
            )?;
            let descriptor = parts.collect::<Vec<_>>().join(" ");
            Some(if descriptor.is_empty() {
                rewritten
            } else {
                format!("{rewritten} {descriptor}")
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn sanitize_html(value: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags(&[
            "audio",
            "video",
            "source",
            "track",
            "picture",
            "svg",
            "g",
            "path",
            "rect",
            "circle",
            "ellipse",
            "line",
            "polyline",
            "polygon",
            "text",
            "tspan",
            "defs",
            "lineargradient",
            "radialgradient",
            "stop",
            "symbol",
            "use",
            "image",
        ])
        .add_generic_attributes(&[
            "id",
            "class",
            "dir",
            "role",
            "aria-label",
            "aria-describedby",
            "epub:type",
        ])
        .add_tag_attributes(
            "img",
            &["src", "srcset", "alt", "width", "height", "loading"],
        )
        .add_tag_attributes("a", &["href", "title"])
        .add_tag_attributes(
            "video",
            &["src", "poster", "controls", "preload", "width", "height"],
        )
        .add_tag_attributes("audio", &["src", "controls", "preload"])
        .add_tag_attributes("source", &["src", "srcset", "type", "media"])
        .add_tag_attributes("track", &["src", "kind", "label", "srclang", "default"])
        .add_tag_attributes(
            "svg",
            &["viewbox", "width", "height", "preserveaspectratio", "xmlns"],
        )
        .add_tag_attributes(
            "path",
            &["d", "fill", "stroke", "stroke-width", "transform"],
        )
        .add_tag_attributes("g", &["fill", "stroke", "stroke-width", "transform"])
        .add_tag_attributes("use", &["href", "xlink:href", "x", "y", "width", "height"])
        .add_tag_attributes(
            "image",
            &["href", "xlink:href", "x", "y", "width", "height"],
        )
        .add_tag_attributes(
            "rect",
            &["x", "y", "width", "height", "rx", "ry", "fill", "stroke"],
        )
        .add_tag_attributes("circle", &["cx", "cy", "r", "fill", "stroke"])
        .add_tag_attributes("ellipse", &["cx", "cy", "rx", "ry", "fill", "stroke"])
        .add_tag_attributes("line", &["x1", "y1", "x2", "y2", "stroke"])
        .add_tag_attributes("polyline", &["points", "fill", "stroke"])
        .add_tag_attributes("polygon", &["points", "fill", "stroke"])
        .add_tag_attributes("text", &["x", "y", "dx", "dy", "fill", "transform"])
        .add_tag_attributes("tspan", &["x", "y", "dx", "dy", "fill"])
        .add_tag_attributes("stop", &["offset", "stop-color", "stop-opacity"])
        .url_schemes(
            ["data", "epub", "http", "https", "mailto", "tel"]
                .into_iter()
                .collect(),
        )
        .link_rel(Some("noopener noreferrer"));
    builder.clean(value).to_string()
}

fn extract_styles(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
    chapter_path: &str,
    raw: &str,
) -> Vec<String> {
    let Ok(doc) = Document::parse(raw) else {
        return Vec::new();
    };
    let mut styles = Vec::new();
    for node in doc.descendants() {
        match local_name(node.tag_name().name()) {
            "style" => {
                if let Some(css) = node.text() {
                    styles.push(rewrite_css(archive, package, opf_path, chapter_path, css));
                }
            }
            "link"
                if node.attribute("rel").is_some_and(|rel| {
                    rel.split_whitespace()
                        .any(|value| value.eq_ignore_ascii_case("stylesheet"))
                }) =>
            {
                let Some(href) = node.attribute("href") else {
                    continue;
                };
                let Some(path) = resolve_path(chapter_path, href) else {
                    continue;
                };
                if let Ok(css) = read_zip_text_limited(archive, &path, MAX_STYLESHEET_BYTES) {
                    styles.push(rewrite_css(archive, package, opf_path, &path, &css));
                }
            }
            _ => {}
        }
    }
    let mut total = 0_usize;
    styles
        .into_iter()
        .filter(|style| !style.trim().is_empty())
        .filter(|style| {
            total = total.saturating_add(style.len());
            total <= MAX_CHAPTER_STYLES_BYTES
        })
        .collect()
}

fn rewrite_css(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
    css_path: &str,
    css: &str,
) -> String {
    let css = strip_css_imports(css);
    let mut output = String::with_capacity(css.len());
    let mut rest = css.as_str();
    while let Some(index) = find_case_insensitive(rest, "url(") {
        output.push_str(&rest[..index]);
        let after = &rest[index + 4..];
        let Some(end) = after.find(')') else {
            break;
        };
        let value = after[..end].trim().trim_matches(['\'', '"']);
        let rewritten = if is_safe_data_url(value) {
            Some(value.to_string())
        } else if has_uri_scheme(value) || value.starts_with("//") || value.starts_with('#') {
            None
        } else {
            resolve_path(css_path, value).and_then(|path| {
                asset_data_url(
                    archive,
                    package,
                    opf_path,
                    &path,
                    MAX_STYLESHEET_ASSET_BYTES,
                )
                .ok()
            })
        };
        if let Some(rewritten) = rewritten {
            output.push_str("url(\"");
            output.push_str(&rewritten);
            output.push_str("\")");
        } else {
            output.push_str("url(\"\")");
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    replace_case_insensitive(&output, "expression(", "blocked(")
}

fn strip_css_imports(css: &str) -> String {
    let mut output = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(index) = find_case_insensitive(rest, "@import") {
        output.push_str(&rest[..index]);
        let after = &rest[index..];
        if let Some(end) = after.find(';') {
            rest = &after[end + 1..];
        } else {
            return output;
        }
    }
    output.push_str(rest);
    output
}

fn asset_data_url(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
    path: &str,
    limit: u64,
) -> FnResult<String> {
    let mime = manifest_mime(package, opf_path, path).unwrap_or_else(|| mime_from_path(path));
    if !is_embeddable_mime(&mime) {
        return Err(Error::msg("unsupported EPUB asset type").into());
    }
    let bytes = read_zip_bytes_limited(archive, path, limit)?;
    Ok(data_url(&mime, &bytes))
}

fn manifest_mime(package: &Package, opf_path: &str, path: &str) -> Option<String> {
    package.manifest.values().find_map(|item| {
        (resolve_path(opf_path, &item.href).as_deref() == Some(path))
            .then(|| item.media_type.clone())
    })
}

fn is_embeddable_mime(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
        || matches!(
            mime,
            "font/ttf"
                | "font/otf"
                | "font/woff"
                | "font/woff2"
                | "application/font-sfnt"
                | "application/vnd.ms-opentype"
        )
}

fn is_safe_data_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "data:image/",
        "data:audio/",
        "data:video/",
        "data:font/",
        "data:application/font-sfnt",
        "data:application/vnd.ms-opentype",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn is_external_link(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ["http://", "https://", "mailto:", "tel:"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn has_uri_scheme(value: &str) -> bool {
    value.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

fn read_zip_text_limited(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: u64,
) -> FnResult<String> {
    let bytes = read_zip_bytes_limited(archive, path, limit)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_zip_bytes_limited(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: u64,
) -> FnResult<Vec<u8>> {
    let safe_path = safe_zip_path(path).ok_or_else(|| Error::msg("unsafe EPUB ZIP path"))?;
    let mut file = archive.by_name(&safe_path)?;
    if file.is_dir() || file.size() > limit {
        return Err(Error::msg(format!("EPUB entry exceeds its {limit} byte limit")).into());
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn resolve_path(base_file: &str, href: &str) -> Option<String> {
    let href = href
        .split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    let href = percent_decode(href)?;
    let base = base_file.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let joined = if href.starts_with('/') {
        href.trim_start_matches('/').to_string()
    } else if base.is_empty() {
        href
    } else {
        format!("{base}/{href}")
    };
    safe_zip_path(&joined)
}

fn safe_zip_path(path: &str) -> Option<String> {
    if path.contains('\\') || path.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn strip_fragment(path: &str) -> String {
    path.split_once('#')
        .map_or(path, |(path, _)| path)
        .to_string()
}

fn mime_from_path(path: &str) -> String {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn node_text(node: Node<'_, '_>) -> Option<String> {
    let text = node
        .descendants()
        .filter(Node::is_text)
        .filter_map(|node| node.text())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.trim().is_empty()).then_some(text)
}

fn clean_filename(value: &str) -> String {
    let value = strip_fragment(value);
    clean_text(
        &value
            .trim_end_matches(".xhtml")
            .trim_end_matches(".html")
            .replace(['_', '-'], " "),
    )
}

fn local_name(value: &str) -> &str {
    value.rsplit_once(':').map_or(value, |(_, name)| name)
}

fn file_stem(path: &str) -> Option<&str> {
    let file = path.rsplit('/').next()?;
    Some(file.rsplit_once('.').map_or(file, |(stem, _)| stem))
}

fn find_case_insensitive(value: &str, needle: &str) -> Option<usize> {
    value
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

fn replace_case_insensitive(value: &str, needle: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = find_case_insensitive(rest, needle) {
        output.push_str(&rest[..index]);
        output.push_str(replacement);
        rest = &rest[index + needle.len()..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn renders_resources_styles_navigation_and_sanitizes_markup() {
        let epub = sample_epub();
        let bytes = Arc::new(epub);
        let mut book = parse_book("test".to_string(), bytes).unwrap();
        book.index.initial_chapter = render_chapter(&book, 0).ok();
        let chapter = book.index.initial_chapter.as_ref().unwrap();

        assert_eq!(book.index.title, "Sample Book");
        assert_eq!(book.index.author.as_deref(), Some("A. Writer"));
        assert_eq!(book.index.chapters[0].title, "First Chapter");
        assert_eq!(book.index.chapters[1].title, "The Second Chapter");
        assert!(chapter.html.contains("data:image/png;base64,"));
        assert!(chapter.html.contains("epub://chapter/1#note"));
        assert!(!chapter.html.contains("script"));
        assert!(!chapter.html.contains("onclick"));
        assert!(chapter.styles[0].contains("data:font/woff2;base64,"));
        assert!(!chapter.styles[0].contains("@import"));
        assert!(!chapter.styles[0].contains("https://tracker.invalid"));
    }

    #[test]
    fn sanitizer_rejects_active_content_and_unsafe_urls() {
        let html = sanitize_html(
            r#"<p onclick="alert(1)">ok</p><script>alert(1)</script><img src="javascript:alert(1)"><iframe srcdoc="bad"></iframe>"#,
        );
        assert!(html.starts_with("<p>ok</p>"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("script"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("iframe"));
    }

    #[test]
    fn cover_is_a_media_view() {
        let cover = render_epub_cover_bytes(&sample_epub()).unwrap().unwrap();
        let media = cover_media_view("Sample Book", &cover).unwrap();
        assert_eq!(media.mime_type, "image/png");
        assert_eq!(media.data, STANDARD.encode("cover"));
    }

    #[test]
    fn reads_epub2_ncx_titles_and_cover_metadata() {
        let book = parse_book("epub2".to_string(), Arc::new(epub2())).unwrap();
        assert_eq!(book.index.title, "EPUB2 Book");
        assert_eq!(book.index.chapters[0].title, "NCX Chapter");
        assert!(
            book.index
                .cover
                .as_deref()
                .is_some_and(|cover| cover.starts_with("data:image/jpeg;base64,"))
        );
    }

    #[test]
    fn initial_action_returns_plugin_frame() {
        let output = render_epub_payload(request_json(json!({}))).unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output["view"], "plugin_frame");
        assert!(
            output["url"]
                .as_str()
                .unwrap()
                .starts_with("index.html#payload=")
        );
    }

    #[test]
    fn load_and_chapter_operations_return_structured_data() {
        let load = render_epub_payload(request_json(json!({"operation": "load"}))).unwrap();
        let load: Value = serde_json::from_str(&load).unwrap();
        assert_eq!(load["data"]["title"], "Sample Book");
        assert_eq!(load["data"]["chapters"].as_array().unwrap().len(), 2);
        assert!(load["data"]["initial_chapter"]["html"].is_string());

        let chapter =
            render_epub_payload(request_json(json!({"operation": "chapter", "index": 1}))).unwrap();
        let chapter: Value = serde_json::from_str(&chapter).unwrap();
        assert_eq!(chapter["data"]["index"], 1);
        assert!(
            chapter["data"]["html"]
                .as_str()
                .unwrap()
                .contains("Second body")
        );
    }

    #[test]
    fn path_resolution_does_not_escape_the_archive_root() {
        assert_eq!(
            resolve_path("OPS/text/one.xhtml", "../images/a.png").as_deref(),
            Some("OPS/images/a.png")
        );
        assert_eq!(resolve_path("one.xhtml", "../../secret"), None);
        assert_eq!(
            safe_zip_path("/OPS/../book.opf").as_deref(),
            Some("book.opf")
        );
        assert_eq!(safe_zip_path("../../book.opf"), None);
    }

    #[test]
    fn chapter_title_ignores_placeholders_and_uses_leading_bold_text() {
        let raw = r#"<html><head><title>Unknown</title></head><body><p><span><b>真实章节标题</b></span></p><p>正文</p></body></html>"#;
        assert_eq!(chapter_document_title(raw).as_deref(), Some("真实章节标题"));
        assert_eq!(
            chapter_document_title(
                "<html><head><title>Untitled</title></head><body><p>正文</p></body></html>"
            ),
            None
        );
    }

    fn request_json(input: Value) -> String {
        json!({
            "action": "azvs.epub.render",
            "access": "read_only",
            "input": input,
            "resource": {
                "id": "01900000-0000-7000-8000-000000000000",
                "name": "book.epub",
                "kind": "azvs:epub",
                "status": "active",
                "metadata": {"schema_version": 1, "summary": {"description": null, "tags": []}},
                "content": {"key": "books/book.epub", "size": 1, "mime_type": "application/epub+zip", "original_filename": "book.epub", "checksum": []},
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            },
            "content": {"encoding": "base64", "data": STANDARD.encode(sample_epub())}
        }).to_string()
    }

    fn sample_epub() -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let stored =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("mimetype", stored).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();
            zip.start_file("META-INF/container.xml", deflated).unwrap();
            zip.write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();
            zip.start_file("OEBPS/content.opf", deflated).unwrap();
            zip.write_all(br#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Sample Book</dc:title><dc:creator>A. Writer</dc:creator></metadata><manifest><item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="styles/book.css" media-type="text/css"/><item id="font" href="fonts/text.woff2" media-type="font/woff2"/><item id="pic" href="images/pic.png" media-type="image/png"/><item id="c1" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#).unwrap();
            zip.start_file("OEBPS/nav.xhtml", deflated).unwrap();
            zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav><ol><li><a href="text/one.xhtml">First Chapter</a><ol><li><a href="text/one.xhtml#section">First Section</a></li></ol></li></ol></nav></body></html>"#).unwrap();
            zip.start_file("OEBPS/text/one.xhtml", deflated).unwrap();
            zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../styles/book.css"/></head><body><h1>First</h1><img src="../images/pic.png" onclick="bad()"/><p><a href="two.xhtml#note">Next note</a></p><script>alert(1)</script></body></html>"#).unwrap();
            zip.start_file("OEBPS/text/two.xhtml", deflated).unwrap();
            zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>The Second Chapter</h1><p id="note">Second body</p></body></html>"#).unwrap();
            zip.start_file("OEBPS/styles/book.css", deflated).unwrap();
            zip.write_all(br#"@import url('https://tracker.invalid/x.css'); @font-face { font-family: Book; src: url('../fonts/text.woff2'); } p { font-family: Book; background: url('https://tracker.invalid/a.png'); }"#).unwrap();
            for (path, data) in [
                ("OEBPS/images/cover.png", b"cover".as_slice()),
                ("OEBPS/images/pic.png", b"picture".as_slice()),
                ("OEBPS/fonts/text.woff2", b"font".as_slice()),
            ] {
                zip.start_file(path, deflated).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn epub2() -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("META-INF/container.xml", options).unwrap();
            zip.write_all(br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#).unwrap();
            zip.start_file("OPS/book.opf", options).unwrap();
            zip.write_all(br#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>EPUB2 Book</dc:title><meta name="cover" content="cover-image"/></metadata><manifest><item id="cover-image" href="cover.jpg" media-type="image/jpeg"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="chapter"/></spine></package>"#).unwrap();
            zip.start_file("OPS/toc.ncx", options).unwrap();
            zip.write_all(br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint><navLabel><text>NCX Chapter</text></navLabel><content src="chapter.xhtml"/></navPoint></navMap></ncx>"#).unwrap();
            zip.start_file("OPS/chapter.xhtml", options).unwrap();
            zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>EPUB 2 body</p></body></html>"#).unwrap();
            zip.start_file("OPS/cover.jpg", options).unwrap();
            zip.write_all(b"jpeg").unwrap();
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }
}
