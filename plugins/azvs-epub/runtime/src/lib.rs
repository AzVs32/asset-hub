use asset_plugin_api::{
    JsonView, MediaView, PluginActionOutput, PluginActionRequest, PluginContentEncoding,
    PluginFrameView, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
#[cfg(target_arch = "wasm32")]
use extism_pdk::host_fn;
use extism_pdk::{Error, FnResult, plugin_fn};
use roxmltree::{Document, Node};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

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

#[derive(Debug, Serialize)]
struct Chapter {
    title: String,
    html: String,
}

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
    let output = if request
        .input
        .get("operation")
        .and_then(|value| value.as_str())
        == Some("load")
    {
        let epub = epub_content_bytes(&request)?;
        let book = render_epub_bytes(&epub)?;
        PluginActionOutput::new(PluginView::Json(JsonView {
            data: serde_json::to_value(book)?,
        }))
    } else {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
            "resource_id": &request.resource.id,
            "resource_name": &request.resource.name,
            "action": &request.action,
        }))?);
        PluginActionOutput::new(PluginView::PluginFrame(PluginFrameView {
            title: Some(request.resource.name.clone()),
            url: format!("index.html#payload={payload}"),
        }))
    };

    Ok(serde_json::to_string(&output)?)
}

fn render_epub_cover_payload(input: String) -> FnResult<String> {
    let request: PluginActionRequest = serde_json::from_str(&input)?;
    let epub = epub_content_bytes(&request)?;
    let view = match render_epub_cover_bytes(&epub)? {
        Some(cover) => PluginView::Media(cover_media_view(&request.resource.name, &cover)?),
        None => PluginView::Json(JsonView {
            data: serde_json::Value::Null,
        }),
    };
    Ok(serde_json::to_string(&PluginActionOutput::new(view))?)
}

fn epub_content_bytes(input: &PluginActionRequest) -> FnResult<Vec<u8>> {
    if let Some(content) = &input.content {
        if content.encoding != PluginContentEncoding::Base64 {
            return Err(Error::msg("unsupported content encoding").into());
        }
        return Ok(STANDARD.decode(&content.data)?);
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
    let size = unsafe { asset_hub_content_size(handle.clone()) }?;
    let mut bytes = Vec::new();
    let mut offset = 0;
    while offset < size {
        let chunk = unsafe { asset_hub_content_read(handle.clone(), offset, size - offset) }?;
        if chunk.is_empty() {
            return Err(Error::msg("content read ended before the declared size").into());
        }
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| Error::msg("content read offset overflow"))?;
        bytes.extend_from_slice(&chunk);
    }
    unsafe { asset_hub_content_close(handle) }?;
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_reference(_reference: &str) -> FnResult<Vec<u8>> {
    Err(Error::msg("content references are only available in the wasm host").into())
}

#[derive(Debug, Serialize)]
struct RenderedBook {
    title: String,
    author: Option<String>,
    cover: Option<String>,
    chapters: Vec<Chapter>,
}

fn render_epub_bytes(epub: &[u8]) -> FnResult<RenderedBook> {
    let mut archive = ZipArchive::new(Cursor::new(epub))?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text(&mut archive, &opf_path)?;
    let package = parse_package(&opf, &opf_path)?;
    let cover = find_cover_data_url(&mut archive, &package, &opf_path);
    let chapters = read_chapters(&mut archive, &package, &opf_path);
    Ok(RenderedBook {
        title: package.title,
        author: package.author,
        cover,
        chapters,
    })
}

fn render_epub_cover_bytes(epub: &[u8]) -> FnResult<Option<String>> {
    let mut archive = ZipArchive::new(Cursor::new(epub))?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text(&mut archive, &opf_path)?;
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

struct Package {
    title: String,
    author: Option<String>,
    manifest: HashMap<String, ManifestItem>,
    spine: Vec<String>,
    cover_item_id: Option<String>,
    guide_cover_href: Option<String>,
}

fn find_opf_path(archive: &mut ZipArchive<Cursor<&[u8]>>) -> FnResult<String> {
    let container = read_zip_text(archive, "META-INF/container.xml")?;
    let doc = Document::parse(&container)?;
    Ok(doc
        .descendants()
        .find(|node| node.has_tag_name("rootfile"))
        .and_then(|node| node.attribute("full-path"))
        .map(str::to_string)
        .ok_or_else(|| Error::msg("EPUB container.xml does not contain a rootfile"))?)
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
    for node in doc.descendants().filter(|node| node.has_tag_name("item")) {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let Some(href) = node.attribute("href") else {
            continue;
        };
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
        .filter(|node| node.has_tag_name("itemref"))
        .filter_map(|node| node.attribute("idref").map(str::to_string))
        .collect::<Vec<_>>();
    let cover_item_id = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("meta")
                && node
                    .attribute("name")
                    .is_some_and(|name| name.eq_ignore_ascii_case("cover"))
        })
        .and_then(|node| node.attribute("content"))
        .map(str::to_string);
    let guide_cover_href = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("reference")
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
    if let Some(item) = package.manifest.values().find(|item| {
        item.properties
            .split_whitespace()
            .any(|value| value == "cover-image")
    }) && let Some(data_url) = image_item_data_url(archive, opf_path, item)
    {
        return Some(data_url);
    }

    if let Some(item) = package
        .cover_item_id
        .as_ref()
        .and_then(|id| package.manifest.get(id))
        && let Some(data_url) = image_item_data_url(archive, opf_path, item)
    {
        return Some(data_url);
    }

    if let Some(href) = &package.guide_cover_href
        && let Some(data_url) = cover_from_guide_href(archive, opf_path, href, package)
    {
        return Some(data_url);
    }

    let item = package.manifest.values().find(|item| {
        item.href.to_ascii_lowercase().contains("cover") && item.media_type.starts_with("image/")
    })?;
    image_item_data_url(archive, opf_path, item)
}

fn image_item_data_url(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    opf_path: &str,
    item: &ManifestItem,
) -> Option<String> {
    if !item.media_type.starts_with("image/") {
        return None;
    }

    let path = resolve_path(opf_path, &item.href);
    let bytes = read_zip_bytes(archive, &path).ok()?;
    Some(format!(
        "data:{};base64,{}",
        item.media_type,
        STANDARD.encode(bytes)
    ))
}

fn cover_from_guide_href(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    opf_path: &str,
    href: &str,
    package: &Package,
) -> Option<String> {
    let href = strip_fragment(href);
    if let Some(item) = package
        .manifest
        .values()
        .find(|item| strip_fragment(&item.href) == href)
    {
        if let Some(data_url) = image_item_data_url(archive, opf_path, item) {
            return Some(data_url);
        }

        let cover_page_path = resolve_path(opf_path, &item.href);
        let cover_page = read_zip_text(archive, &cover_page_path).ok()?;
        let image_href = first_image_src(&cover_page)?;
        let image_path = resolve_path(&cover_page_path, &image_href);
        if let Some(image_item) = package
            .manifest
            .values()
            .find(|item| resolve_path(opf_path, &item.href) == image_path)
        {
            return image_item_data_url(archive, opf_path, image_item);
        }

        let bytes = read_zip_bytes(archive, &image_path).ok()?;
        return Some(format!(
            "data:{};base64,{}",
            mime_from_path(&image_path),
            STANDARD.encode(bytes)
        ));
    }

    None
}

fn first_image_src(raw: &str) -> Option<String> {
    let doc = Document::parse(raw).ok()?;
    doc.descendants()
        .find(|node| local_name(node.tag_name().name()) == "img")
        .and_then(|node| node.attribute("src").or_else(|| node.attribute("href")))
        .map(str::to_string)
}

fn mime_from_path(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    }
}

fn read_chapters(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    package: &Package,
    opf_path: &str,
) -> Vec<Chapter> {
    let toc_titles = read_toc_titles(archive, package, opf_path);
    package
        .spine
        .iter()
        .filter_map(|id| package.manifest.get(id))
        .filter(|item| {
            item.media_type == "application/xhtml+xml"
                || item.media_type == "text/html"
                || item.href.ends_with(".xhtml")
                || item.href.ends_with(".html")
        })
        .enumerate()
        .filter_map(|(index, item)| {
            let path = resolve_path(opf_path, &item.href);
            let raw = read_zip_text(archive, &path).ok()?;
            let title = toc_titles
                .get(&strip_fragment(&path))
                .cloned()
                .or_else(|| chapter_title(&raw))
                .unwrap_or_else(|| {
                    file_stem(&item.href)
                        .map(clean_filename)
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| format!("Chapter {}", index + 1))
                });
            let html = chapter_body_html(&raw);
            Some(Chapter { title, html })
        })
        .collect()
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
    {
        let path = resolve_path(opf_path, &ncx.href);
        if let Ok(raw) = read_zip_text(archive, &path) {
            titles.extend(parse_ncx_titles(&raw, &path));
        }
    }

    if let Some(nav) = package.manifest.values().find(|item| {
        item.properties
            .split_whitespace()
            .any(|value| value == "nav")
            || item.href.ends_with("nav.xhtml")
            || item.href.ends_with("nav.html")
    }) {
        let path = resolve_path(opf_path, &nav.href);
        if let Ok(raw) = read_zip_text(archive, &path) {
            titles.extend(parse_nav_titles(&raw, &path));
        }
    }

    titles
}

fn parse_ncx_titles(raw: &str, ncx_path: &str) -> HashMap<String, String> {
    let Ok(doc) = Document::parse(raw) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();

    for nav_point in doc
        .descendants()
        .filter(|node| local_name(node.tag_name().name()) == "navPoint")
    {
        let title = nav_point
            .descendants()
            .find(|node| local_name(node.tag_name().name()) == "text")
            .and_then(|node| node.text())
            .map(clean_text)
            .filter(|value| !value.is_empty());
        let src = nav_point
            .descendants()
            .find(|node| local_name(node.tag_name().name()) == "content")
            .and_then(|node| node.attribute("src"));

        if let (Some(title), Some(src)) = (title, src) {
            let path = strip_fragment(&resolve_path(ncx_path, src));
            titles.insert(path, title);
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
        let Some(href) = link.attribute("href") else {
            continue;
        };
        let title = node_text(link)
            .map(|value| clean_text(&value))
            .filter(|value| !value.is_empty());
        if let Some(title) = title {
            let path = strip_fragment(&resolve_path(nav_path, href));
            titles.insert(path, title);
        }
    }

    titles
}

fn read_zip_text(archive: &mut ZipArchive<Cursor<&[u8]>>, path: &str) -> FnResult<String> {
    let bytes = read_zip_bytes(archive, path)?;
    Ok(String::from_utf8(bytes).map_err(|error| Error::msg(error.to_string()))?)
}

fn read_zip_bytes(archive: &mut ZipArchive<Cursor<&[u8]>>, path: &str) -> FnResult<Vec<u8>> {
    let mut file = archive.by_name(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn resolve_path(base_file: &str, href: &str) -> String {
    let base = base_file.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let joined = if base.is_empty() {
        href.to_string()
    } else {
        format!("{base}/{href}")
    };
    normalize_zip_path(&joined)
}

fn normalize_zip_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}

fn chapter_title(raw: &str) -> Option<String> {
    let doc = Document::parse(raw).ok()?;
    for tag in ["h1", "h2", "title"] {
        if let Some(text) = doc
            .descendants()
            .find(|node| local_name(node.tag_name().name()) == tag)
            .and_then(node_text)
            .map(|value| clean_text(&value))
            .filter(|value| !value.is_empty())
        {
            return Some(text);
        }
    }
    None
}

fn chapter_body_html(raw: &str) -> String {
    let Some((_, after_body)) = split_once_case_insensitive(raw, "<body") else {
        return sanitize_fragment(raw);
    };
    let Some((_, body_content)) = after_body.split_once('>') else {
        return sanitize_fragment(raw);
    };
    let end = find_case_insensitive(body_content, "</body>").unwrap_or(body_content.len());
    sanitize_fragment(&body_content[..end])
}

fn sanitize_fragment(value: &str) -> String {
    let without_scripts = strip_tag_blocks(value, "script");
    let without_styles = strip_tag_blocks(&without_scripts, "style");
    without_styles
        .replace("<?xml", "&lt;?xml")
        .replace("<html", "<div")
        .replace("</html>", "</div>")
        .replace("<body", "<section")
        .replace("</body>", "</section>")
}

fn strip_tag_blocks(value: &str, tag: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    let open = format!("<{tag}");
    let close = format!("</{tag}>");

    while let Some(start) = find_case_insensitive(rest, &open) {
        output.push_str(&rest[..start]);
        let after_start = &rest[start..];
        if let Some(end) = find_case_insensitive(after_start, &close) {
            rest = &after_start[end + close.len()..];
        } else {
            rest = "";
            break;
        }
    }
    output.push_str(rest);
    output
}

fn split_once_case_insensitive<'a>(value: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let index = find_case_insensitive(value, needle)?;
    Some((&value[..index], &value[index + needle.len()..]))
}

fn find_case_insensitive(value: &str, needle: &str) -> Option<usize> {
    value
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
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
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn strip_fragment(path: &str) -> String {
    path.split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(path)
        .to_string()
}

fn clean_filename(value: &str) -> String {
    let value = value
        .trim_end_matches(".xhtml")
        .trim_end_matches(".html")
        .replace(['_', '-'], " ");
    clean_text(&value)
}

fn local_name(value: &str) -> &str {
    value
        .rsplit_once(':')
        .map(|(_, name)| name)
        .unwrap_or(value)
}

fn file_stem(path: &str) -> Option<&str> {
    let file = path.rsplit('/').next()?;
    file.rsplit_once('.').map(|(stem, _)| stem).or(Some(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn renders_cover_and_chapter_navigation() {
        let epub = minimal_epub();
        let rendered = render_epub_bytes(&epub).unwrap();

        assert_eq!(rendered.title, "Sample Book");
        assert_eq!(rendered.author.as_deref(), Some("A. Writer"));
        assert!(
            rendered
                .cover
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert_eq!(rendered.chapters[0].title, "Chapter One");
        assert!(rendered.chapters[0].html.contains("Hello EPUB"));
    }

    #[test]
    fn renders_epub2_cover_meta_and_ncx_chapter_titles() {
        let epub = epub2_with_ncx();
        let rendered = render_epub_bytes(&epub).unwrap();

        assert_eq!(rendered.title, "EPUB2 Book");
        assert!(
            rendered
                .cover
                .unwrap()
                .starts_with("data:image/jpeg;base64,")
        );
        assert_eq!(rendered.chapters[0].title, "NCX Chapter One");
        assert_eq!(rendered.chapters[1].title, "NCX Chapter Two");
        assert!(rendered.chapters[0].html.contains("Body without heading"));
    }

    #[test]
    fn renders_cover_as_media_view() {
        let cover = render_epub_cover_bytes(&minimal_epub()).unwrap().unwrap();
        let media = cover_media_view("Sample Book", &cover).unwrap();

        assert_eq!(media.mime_type, "image/png");
        assert_eq!(media.title.as_deref(), Some("Sample Book"));
        assert_eq!(media.encoding, PluginContentEncoding::Base64);
        assert_eq!(media.data, STANDARD.encode("fakepng"));
    }

    #[test]
    fn render_epub_returns_plugin_frame() {
        let output = render_epub_payload(
            json!({
                "action": "azvs.epub.render",
                "access": "read_only",
                "input": {},
                "resource": {
                    "id": "01900000-0000-7000-8000-000000000000",
                    "name": "book.epub",
                    "kind": "azvs:epub",
                    "status": "active",
                    "metadata": {
                        "schema_version": 1,
                        "summary": {
                            "description": null,
                            "tags": []
                        }
                    },
                    "content": {
                        "key": "books/book.epub",
                        "size": 1,
                        "mime_type": "application/epub+zip",
                        "original_filename": "book.epub",
                        "checksum": []
                    },
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                },
                "content": {
                    "encoding": "base64",
                    "data": STANDARD.encode(minimal_epub())
                }
            })
            .to_string(),
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "plugin_frame");
        assert_eq!(output["title"], "book.epub");
        assert!(
            output["url"]
                .as_str()
                .unwrap()
                .starts_with("index.html#payload=")
        );
    }

    #[test]
    fn load_operation_returns_structured_book_data() {
        let output = render_epub_payload(
            json!({
                "action": "azvs.epub.render",
                "access": "read_only",
                "input": {"operation": "load"},
                "resource": {
                    "id": "01900000-0000-7000-8000-000000000000",
                    "name": "book.epub",
                    "kind": "azvs:epub",
                    "status": "active",
                    "metadata": {
                        "schema_version": 1,
                        "summary": {"description": null, "tags": []}
                    },
                    "content": {
                        "key": "books/book.epub",
                        "size": 1,
                        "mime_type": "application/epub+zip",
                        "original_filename": "book.epub",
                        "checksum": []
                    },
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                },
                "content": {
                    "encoding": "base64",
                    "data": STANDARD.encode(minimal_epub())
                }
            })
            .to_string(),
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "json");
        assert_eq!(output["data"]["title"], "Sample Book");
        assert_eq!(output["data"]["author"], "A. Writer");
        assert_eq!(output["data"]["chapters"][0]["title"], "Chapter One");
        assert!(
            output["data"]["chapters"][0]["html"]
                .as_str()
                .unwrap()
                .contains("Hello EPUB")
        );
    }

    fn minimal_epub() -> Vec<u8> {
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
            zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/content.opf", deflated).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Sample Book</dc:title>
    <dc:creator>A. Writer</dc:creator>
  </metadata>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/cover.png", deflated).unwrap();
            zip.write_all(b"fakepng").unwrap();
            zip.start_file("OEBPS/chapter1.xhtml", deflated).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter One</title></head><body><h1>Chapter One</h1><p>Hello EPUB</p></body></html>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn epub2_with_ncx() -> Vec<u8> {
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
            zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();
            zip.start_file("OPS/package.opf", deflated).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>EPUB2 Book</dc:title>
  </metadata>
  <metadata>
    <meta name="cover" content="cover-image"/>
  </metadata>
  <manifest>
    <item id="cover-image" href="images/front.jpg" media-type="image/jpeg"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="c1" href="text/one.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/two.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="c1"/>
    <itemref idref="c2"/>
  </spine>
</package>"#,
            )
            .unwrap();
            zip.start_file("OPS/toc.ncx", deflated).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/">
  <navMap>
    <navPoint id="n1"><navLabel><text>NCX Chapter One</text></navLabel><content src="text/one.xhtml"/></navPoint>
    <navPoint id="n2"><navLabel><text>NCX Chapter Two</text></navLabel><content src="text/two.xhtml#frag"/></navPoint>
  </navMap>
</ncx>"#,
            )
            .unwrap();
            zip.start_file("OPS/images/front.jpg", deflated).unwrap();
            zip.write_all(b"fakejpg").unwrap();
            zip.start_file("OPS/text/one.xhtml", deflated).unwrap();
            zip.write_all(br#"<html><body><p>Body without heading.</p></body></html>"#)
                .unwrap();
            zip.start_file("OPS/text/two.xhtml", deflated).unwrap();
            zip.write_all(br#"<html><body><p>Second body.</p></body></html>"#)
                .unwrap();
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }
}
