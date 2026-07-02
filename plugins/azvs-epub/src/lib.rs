use asset_plugin_api::{
    HtmlView, PluginActionOutput, PluginActionRequest, PluginContentEncoding, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
#[cfg(target_arch = "wasm32")]
use extism_pdk::host_fn;
use extism_pdk::{plugin_fn, Error, FnResult};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

#[cfg(target_arch = "wasm32")]
#[host_fn]
extern "ExtismHost" {
    fn asset_hub_content_read(url: String) -> String;
}

#[derive(Debug, Clone)]
struct ManifestItem {
    href: String,
    media_type: String,
    properties: String,
}

#[derive(Debug)]
struct Chapter {
    title: String,
    html: String,
}

#[plugin_fn]
pub fn render_epub(input: String) -> FnResult<String> {
    render_epub_payload(input)
}

fn render_epub_payload(input: String) -> FnResult<String> {
    let input: PluginActionRequest = serde_json::from_str(&input)?;
    let epub = STANDARD.decode(epub_content_base64(&input)?)?;
    let rendered = render_epub_bytes(&epub)?;

    Ok(serde_json::to_string(&PluginActionOutput::new(PluginView::Html(
        HtmlView {
            title: Some(rendered.title),
            html: rendered.html,
        },
    )))?)
}

fn epub_content_base64(input: &PluginActionRequest) -> FnResult<String> {
    if let Some(content) = &input.content {
        if content.encoding != PluginContentEncoding::Base64 {
            return Err(Error::msg("unsupported content encoding").into());
        }
        return Ok(content.data.clone());
    }

    let content_ref = input
        .content_ref
        .as_ref()
        .ok_or_else(|| Error::msg("missing EPUB content payload"))?;
    if content_ref.encoding != PluginContentEncoding::Url {
        return Err(Error::msg("unsupported content reference encoding").into());
    }

    read_content_ref_base64(&content_ref.url)
}

#[cfg(target_arch = "wasm32")]
fn read_content_ref_base64(url: &str) -> FnResult<String> {
    unsafe { asset_hub_content_read(url.to_string()) }.map_err(Into::into)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_ref_base64(_url: &str) -> FnResult<String> {
    Err(Error::msg("content references are only available in the wasm host").into())
}

struct RenderedBook {
    title: String,
    html: String,
}

fn render_epub_bytes(epub: &[u8]) -> FnResult<RenderedBook> {
    let mut archive = ZipArchive::new(Cursor::new(epub))?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text(&mut archive, &opf_path)?;
    let package = parse_package(&opf, &opf_path)?;
    let cover = find_cover_data_url(&mut archive, &package, &opf_path);
    let chapters = read_chapters(&mut archive, &package, &opf_path);
    let html = book_html(
        &package.title,
        package.author.as_deref(),
        cover.as_deref(),
        &chapters,
    );

    Ok(RenderedBook {
        title: package.title,
        html,
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
    }) {
        if let Some(data_url) = image_item_data_url(archive, opf_path, item) {
            return Some(data_url);
        }
    }

    if let Some(item) = package
        .cover_item_id
        .as_ref()
        .and_then(|id| package.manifest.get(id))
    {
        if let Some(data_url) = image_item_data_url(archive, opf_path, item) {
            return Some(data_url);
        }
    }

    if let Some(href) = &package.guide_cover_href {
        if let Some(data_url) = cover_from_guide_href(archive, opf_path, href, package) {
            return Some(data_url);
        }
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

fn book_html(
    title: &str,
    author: Option<&str>,
    cover: Option<&str>,
    chapters: &[Chapter],
) -> String {
    let nav = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            format!(
                r#"<button class="chapter-link{}" type="button" data-chapter="{index}">{}</button>"#,
                if index == 0 { " active" } else { "" },
                escape_html(&chapter.title)
            )
        })
        .collect::<String>();
    let sections = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            format!(
                r#"<section class="chapter{}" data-chapter="{index}"><h2>{}</h2>{}</section>"#,
                if index == 0 { " active" } else { "" },
                escape_html(&chapter.title),
                chapter.html
            )
        })
        .collect::<String>();
    let cover_html = cover
        .map(|cover| format!(r#"<img class="cover" src="{cover}" alt="Book cover">"#))
        .unwrap_or_else(|| r#"<div class="cover placeholder">No Cover</div>"#.to_string());
    let author_html = author
        .map(|author| format!(r#"<p class="author">{}</p>"#, escape_html(author)))
        .unwrap_or_default();

    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
:root {{
  color-scheme: light;
  --paper: #fffdf8;
  --ink: #222831;
  --muted: #6b7280;
  --line: #ded8cc;
  --accent: #2f6f6d;
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0;
  background: #ebe6dc;
  color: var(--ink);
  font-family: ui-serif, Georgia, "Times New Roman", serif;
}}
.layout {{
  display: grid;
  grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
  min-height: 100vh;
}}
.sidebar {{
  position: sticky;
  top: 0;
  height: 100vh;
  overflow: auto;
  padding: 22px;
  border-right: 1px solid var(--line);
  background: #f7f2e8;
}}
.cover {{
  display: block;
  width: 100%;
  max-height: 320px;
  object-fit: contain;
  margin-bottom: 18px;
  border-radius: 6px;
  background: #ddd4c3;
}}
.cover.placeholder {{
  display: grid;
  place-items: center;
  aspect-ratio: 2 / 3;
  color: var(--muted);
  font: 600 14px ui-sans-serif, system-ui;
}}
h1 {{
  margin: 0;
  font-size: 24px;
  line-height: 1.2;
}}
.author {{
  margin: 8px 0 20px;
  color: var(--muted);
  font: 14px/1.4 ui-sans-serif, system-ui;
}}
.chapter-list {{
  display: grid;
  gap: 6px;
}}
.chapter-link {{
  width: 100%;
  min-height: 34px;
  padding: 8px 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: #374151;
  text-align: left;
  font: 13px/1.35 ui-sans-serif, system-ui;
  cursor: pointer;
}}
.chapter-link:hover,
.chapter-link.active {{
  border-color: #b9cabf;
  background: #ffffff;
  color: var(--accent);
}}
.reader {{
  max-width: 860px;
  width: 100%;
  margin: 0 auto;
  padding: 52px 58px 80px;
  background: var(--paper);
  box-shadow: 0 0 0 1px rgba(31, 41, 55, .05);
}}
.chapter {{ display: none; }}
.chapter.active {{ display: block; }}
.chapter h2 {{
  margin: 0 0 24px;
  color: #111827;
  font-size: 28px;
  line-height: 1.2;
}}
.chapter p {{
  margin: 0 0 1.05em;
  font-size: 19px;
  line-height: 1.9;
}}
.chapter img {{
  max-width: 100%;
  height: auto;
}}
@media (max-width: 760px) {{
  .layout {{ grid-template-columns: 1fr; }}
  .sidebar {{ position: static; height: auto; border-right: 0; border-bottom: 1px solid var(--line); }}
  .reader {{ padding: 32px 22px 56px; }}
}}
</style>
</head>
<body>
<main class="layout">
  <aside class="sidebar">
    {cover_html}
    <h1>{}</h1>
    {author_html}
    <nav class="chapter-list" aria-label="Chapters">{nav}</nav>
  </aside>
  <article class="reader">{sections}</article>
</main>
<script>
const buttons = Array.from(document.querySelectorAll('.chapter-link'));
const chapters = Array.from(document.querySelectorAll('.chapter'));
buttons.forEach((button) => {{
  button.addEventListener('click', () => {{
    const id = button.dataset.chapter;
    buttons.forEach((item) => item.classList.toggle('active', item === button));
    chapters.forEach((chapter) => chapter.classList.toggle('active', chapter.dataset.chapter === id));
    window.scrollTo({{ top: 0, behavior: 'smooth' }});
  }});
}});
</script>
</body>
</html>"#,
        escape_html(title)
    )
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

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
        assert!(rendered.html.contains("data:image/png;base64,"));
        assert!(rendered.html.contains("Chapter One"));
        assert!(rendered.html.contains("chapter-link active"));
        assert!(rendered.html.contains("Hello EPUB"));
    }

    #[test]
    fn renders_epub2_cover_meta_and_ncx_chapter_titles() {
        let epub = epub2_with_ncx();
        let rendered = render_epub_bytes(&epub).unwrap();

        assert_eq!(rendered.title, "EPUB2 Book");
        assert!(rendered.html.contains("data:image/jpeg;base64,"));
        assert!(rendered.html.contains("NCX Chapter One"));
        assert!(rendered.html.contains("NCX Chapter Two"));
        assert!(rendered.html.contains("Body without heading"));
    }

    #[test]
    fn render_epub_returns_html_reader() {
        let output = render_epub_payload(
            json!({
                "action": "azvs:render_epub",
                "access": "read_only",
                "input": {},
                "resource": {
                    "id": "01900000-0000-7000-8000-000000000000",
                    "name": "book.epub",
                    "kind": "azvs:epub",
                    "status": "active",
                    "metadata": {},
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

        assert_eq!(output["view"], "html");
        assert_eq!(output["title"], "Sample Book");
        assert!(output["html"].as_str().unwrap().contains("Chapter One"));
        assert!(output["html"].as_str().unwrap().contains("Hello EPUB"));
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
