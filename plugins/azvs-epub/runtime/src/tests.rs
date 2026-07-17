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
        zip.write_all(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>EPUB 2 body</p></body></html>"#,
        )
        .unwrap();
        zip.start_file("OPS/cover.jpg", options).unwrap();
        zip.write_all(b"jpeg").unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}
