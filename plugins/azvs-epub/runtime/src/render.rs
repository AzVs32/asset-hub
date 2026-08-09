use super::*;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use extism_pdk::{Error, FnResult};
use lol_html::{RewriteStrSettings, element, rewrite_str};
use std::collections::HashMap;
use std::io::Cursor;
use zip::ZipArchive;

pub(super) fn render_chapter(book: &CachedBook, index: usize) -> FnResult<ChapterContent> {
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

pub(super) fn rewrite_chapter_html(
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
        RewriteStrSettings::new()
            .append_element_content_handler(element!("*", |element| {
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
            }))
            .with_strict(true),
    )
    .map_err(|error| Error::msg(format!("unable to parse chapter HTML: {error}")))?;
    Ok(rewritten)
}

pub(super) fn rewrite_link(
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

pub(super) fn rewrite_asset_reference(
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

pub(super) fn rewrite_srcset(
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

pub(super) fn asset_data_url(
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

pub(super) fn manifest_mime(package: &Package, opf_path: &str, path: &str) -> Option<String> {
    package.manifest.values().find_map(|item| {
        (resolve_path(opf_path, &item.href).as_deref() == Some(path))
            .then(|| item.media_type.clone())
    })
}

pub(super) fn is_embeddable_mime(mime: &str) -> bool {
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

pub(super) fn is_safe_data_url(value: &str) -> bool {
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

pub(super) fn is_external_link(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ["http://", "https://", "mailto:", "tel:"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

pub(super) fn has_uri_scheme(value: &str) -> bool {
    value.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

pub(super) fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}
