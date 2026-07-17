use super::*;
use ammonia::Builder;
use roxmltree::Document;
use std::io::Cursor;
use zip::ZipArchive;

pub(super) fn sanitize_html(value: &str) -> String {
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

pub(super) fn extract_styles(
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

pub(super) fn rewrite_css(
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

pub(super) fn strip_css_imports(css: &str) -> String {
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
