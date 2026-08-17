use super::*;
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io::Cursor;
use zip::ZipArchive;

pub(super) fn parse_package(opf: &str, opf_path: &str) -> Result<Package> {
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

pub(super) fn find_cover_data_url(
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

pub(super) fn image_item_data_url(
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

pub(super) fn cover_from_guide_href(
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

pub(super) fn first_image_src(raw: &str) -> Option<String> {
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

pub(super) fn read_chapter_summaries(
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

pub(super) fn chapter_document_title(raw: &str) -> Option<String> {
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

pub(super) fn is_usable_chapter_title(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    !value.is_empty()
        && value.chars().count() <= 160
        && !matches!(
            lower.as_str(),
            "unknown" | "untitled" | "untitled chapter" | "chapter" | "document"
        )
}

pub(super) fn is_markup(item: &ManifestItem) -> bool {
    item.media_type == "application/xhtml+xml"
        || item.media_type == "text/html"
        || strip_fragment(&item.href).ends_with(".xhtml")
        || strip_fragment(&item.href).ends_with(".html")
}

pub(super) fn read_toc_titles(
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

pub(super) fn parse_ncx_titles(raw: &str, ncx_path: &str) -> HashMap<String, String> {
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

pub(super) fn parse_nav_titles(raw: &str, nav_path: &str) -> HashMap<String, String> {
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

pub(super) fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn node_text(node: Node<'_, '_>) -> Option<String> {
    let text = node
        .descendants()
        .filter(Node::is_text)
        .filter_map(|node| node.text())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.trim().is_empty()).then_some(text)
}

pub(super) fn clean_filename(value: &str) -> String {
    let value = strip_fragment(value);
    clean_text(
        &value
            .trim_end_matches(".xhtml")
            .trim_end_matches(".html")
            .replace(['_', '-'], " "),
    )
}

pub(super) fn local_name(value: &str) -> &str {
    value.rsplit_once(':').map_or(value, |(_, name)| name)
}

pub(super) fn file_stem(path: &str) -> Option<&str> {
    let file = path.rsplit('/').next()?;
    Some(file.rsplit_once('.').map_or(file, |(stem, _)| stem))
}

pub(super) fn find_case_insensitive(value: &str, needle: &str) -> Option<usize> {
    value
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

pub(super) fn replace_case_insensitive(value: &str, needle: &str, replacement: &str) -> String {
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
