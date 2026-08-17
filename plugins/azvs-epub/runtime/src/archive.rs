use super::*;
use roxmltree::Document;
use std::io::{Cursor, Read};
use std::sync::Arc;
use zip::ZipArchive;

pub(super) fn epub_content_bytes(context: &ResourceContext) -> Result<Vec<u8>> {
    context.content().read_all(MAX_EPUB_BYTES, READ_CHUNK_BYTES)
}

pub(super) fn parse_book(key: String, bytes: Arc<Vec<u8>>) -> Result<CachedBook> {
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

pub(super) fn open_archive(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>> {
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

pub(super) fn render_epub_cover_bytes(epub: &[u8]) -> Result<Option<String>> {
    let mut archive = open_archive(epub)?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf = read_zip_text_limited(&mut archive, &opf_path, MAX_MARKUP_BYTES)?;
    let package = parse_package(&opf, &opf_path)?;
    Ok(find_cover_data_url(&mut archive, &package, &opf_path))
}

pub(super) fn cover_media_view(title: &str, data_url: &str) -> Result<Media> {
    let (mime_type, data) = data_url
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
        .ok_or_else(|| Error::msg("invalid EPUB cover data URL"))?;
    Ok(Media::base64_data(mime_type, data).title(title))
}

pub(super) fn find_opf_path(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<String> {
    let container = read_zip_text_limited(archive, "META-INF/container.xml", MAX_MARKUP_BYTES)?;
    let doc = Document::parse(&container)?;
    let path = doc
        .descendants()
        .find(|node| local_name(node.tag_name().name()) == "rootfile")
        .and_then(|node| node.attribute("full-path"))
        .ok_or_else(|| Error::msg("EPUB container.xml does not contain a rootfile"))?;
    safe_zip_path(path).ok_or_else(|| Error::msg("EPUB rootfile has an unsafe path").into())
}

pub(super) fn read_zip_text_limited(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: u64,
) -> Result<String> {
    let bytes = read_zip_bytes_limited(archive, path, limit)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn read_zip_bytes_limited(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let safe_path = safe_zip_path(path).ok_or_else(|| Error::msg("unsafe EPUB ZIP path"))?;
    let mut file = archive.by_name(&safe_path)?;
    if file.is_dir() || file.size() > limit {
        return Err(Error::msg(format!("EPUB entry exceeds its {limit} byte limit")).into());
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn resolve_path(base_file: &str, href: &str) -> Option<String> {
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

pub(super) fn safe_zip_path(path: &str) -> Option<String> {
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

pub(super) fn percent_decode(value: &str) -> Option<String> {
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

pub(super) fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(super) fn strip_fragment(path: &str) -> String {
    path.split_once('#')
        .map_or(path, |(path, _)| path)
        .to_string()
}

pub(super) fn mime_from_path(path: &str) -> String {
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
