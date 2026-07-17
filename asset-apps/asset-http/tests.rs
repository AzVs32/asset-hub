use crate::router;
use crate::settings::{CorsPolicy, RouterOptions, SessionOptions};
use asset_apps::AssetRuntime;
use asset_core::domain::{AccessContext, UserId};
use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, KindRegistryConfig, PluginHostConfig,
    ResourceKindConfig,
};
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::{Extension, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_util::stream;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;
use zip::write::SimpleFileOptions;

const BODY_LIMIT: usize = 1024 * 1024;

struct TestApp {
    router: Router,
    root: PathBuf,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn resource_kinds_are_listed_and_unsupported_kind_is_rejected() {
    let app = test_app("resource-kinds").await;
    let (status, kinds) = empty_json_request(&app, Method::GET, "/resource-kinds").await;

    assert_eq!(status, StatusCode::OK);
    let unknown = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:unknown")
        .unwrap();
    assert_eq!(unknown["parent"], "core:file");
    for (kind, source) in [
        ("core:file", "plugin:core.file"),
        ("core:image", "plugin:core.image"),
        ("core:document", "plugin:core.document"),
        ("core:video", "plugin:core.video"),
    ] {
        assert!(
            kinds["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == kind && item["source"] == source),
            "missing {kind}"
        );
    }
    let image_kind = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:image")
        .unwrap();
    assert_eq!(image_kind["detect"]["mime_types"], json!(["image/*"]));
    assert!(
        image_kind["detect"]["extensions"]
            .as_array()
            .unwrap()
            .contains(&json!(".png"))
    );
    let file_kind = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:file")
        .unwrap();
    assert!(file_kind.get("detect").is_none());

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "unsupported",
            "kind": "plugin:not-installed"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("unsupported resource kind")
    );
}

#[tokio::test]
async fn configured_resource_kind_is_listed_and_content_support_is_enforced() {
    let app = test_app_with_kind_definitions(
        "configured-resource-kinds",
        vec![ResourceKindConfig {
            kind: "doc:note".to_string(),
            label: Some("Note".to_string()),
            supports_content: false,
            actions: Vec::new(),
            ..ResourceKindConfig::default()
        }],
    )
    .await;
    let (status, kinds) = empty_json_request(&app, Method::GET, "/resource-kinds").await;

    assert_eq!(status, StatusCode::OK);
    let note_kind = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "doc:note")
        .unwrap();
    assert_eq!(note_kind["label"], "Note");
    assert_eq!(note_kind["source"], "config");
    assert_eq!(note_kind["supports_content"], false);

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "metadata note",
            "kind": "doc:note"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "doc:note");
    assert!(!has_action(&resource, "download_content"));
    assert!(!has_action(&resource, "read"));
    assert!(!has_action(&resource, "view_inline"));

    let (status, error) = stream_upload(
        &app,
        "/resources/content/stream?name=note.txt&kind=doc%3Anote&storage_key=notes%2Fnote.txt",
        "text/plain",
        b"note",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("does not support content upload")
    );
}

#[tokio::test]
async fn core_document_resource_exposes_download_only() {
    let app = test_app("core-document-read").await;

    let (status, kinds) = empty_json_request(&app, Method::GET, "/resource-kinds").await;
    assert_eq!(status, StatusCode::OK);
    let document_kind = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:document")
        .unwrap();
    assert_eq!(document_kind["source"], "plugin:core.document");
    let actions = document_kind["actions"].as_array().unwrap();
    let download = actions
        .iter()
        .find(|action| action["id"] == "download_content")
        .unwrap();
    assert_eq!(download["label"], "Download");
    assert_eq!(download["access"], "read_only");
    assert_eq!(download["executor"]["type"], "builtin");
    assert_eq!(download["executor"]["handler"], "builtin.content.download");
    assert_eq!(download["requires"]["content_delivery"], "reference");
    assert_eq!(download["output"]["view"], json!(["binary_url"]));
    assert_eq!(
        download["ui"]["locations"],
        json!(["resource_detail", "context_menu"])
    );

    let view_inline = actions
        .iter()
        .find(|action| action["id"] == "view_inline")
        .unwrap();
    assert_eq!(view_inline["executor"]["handler"], "builtin.media.view");
    assert!(view_inline["requires"].get("resource").is_none());
    assert!(view_inline["requires"].get("metadata").is_none());
    assert_eq!(view_inline["requires"]["content_delivery"], "reference");
    assert_eq!(view_inline["output"]["view"], json!(["media"]));
    assert_eq!(
        view_inline["applies_to"],
        json!({
            "kinds": ["core:document"],
            "mime_types": ["application/pdf"],
            "extensions": [".pdf"]
        })
    );

    let preview = actions
        .iter()
        .find(|action| action["id"] == "preview")
        .unwrap();
    assert_eq!(preview["executor"]["handler"], "builtin.media.preview");
    assert_eq!(preview["ui"]["group"], "preview");

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=book.txt&kind=core%3Adocument&storage_key=books%2Fbook.txt",
        "text/plain",
        b"Hello book",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let id = resource["id"].as_str().unwrap();
    assert!(has_action(&resource, "download_content"));
    assert!(!has_action(&resource, "read"));
    assert!(!has_action(&resource, "view_inline"));
    let (status, error) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}/read")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("action `read`"));
}

#[tokio::test]
async fn action_endpoint_requires_plugin_handler() {
    let app = test_app("action-endpoint-handler").await;
    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=book.txt&kind=core%3Adocument&storage_key=books%2Faction-book.txt",
        "text/plain",
        b"Hello action",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    let id = resource["id"].as_str().unwrap();
    let (status, error) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{id}/actions/read"),
        json!({"input": {"mode": "test"}}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("action `read`"));
}

#[tokio::test]
async fn action_endpoint_has_a_dedicated_request_body_limit() {
    let app = test_app("action-body-limit").await;
    let oversized = format!(
        "{{\"input\":{{\"value\":\"{}\"}}}}",
        "x".repeat(crate::handlers::MAX_ACTION_REQUEST_BYTES)
    );
    let response = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/resources/01900000-0000-7000-8000-000000000000/actions/example")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(oversized))
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn core_document_epub_resource_does_not_get_core_text_extraction() {
    let app = test_app("core-document-epub-read").await;
    let epub = minimal_epub();

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=book.epub&storage_key=books/book.epub&kind=core%3Adocument")
            .header(header::CONTENT_TYPE, "application/epub+zip")
            .body(Body::from(epub))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let resource = response_json(response).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["mime_type"], "application/epub+zip");

    let id = resource["id"].as_str().unwrap();
    let (status, readable) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}/read")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        readable["error"]
            .as_str()
            .unwrap()
            .contains("action `read`")
    );
}

#[tokio::test]
async fn core_document_pdf_resource_supports_builtin_preview() {
    let app = test_app("core-document-pdf-read").await;
    let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF\n";

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=book.pdf&storage_key=books/book.pdf&kind=core%3Adocument")
            .header(header::CONTENT_TYPE, "application/pdf")
            .body(Body::from(pdf.as_slice()))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let resource = response_json(response).await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(has_action(&resource, "download_content"));
    assert!(!has_action(&resource, "read"));
    assert!(has_action(&resource, "view_inline"));
    assert!(has_action(&resource, "preview"));

    let id = resource["id"].as_str().unwrap();
    let preview = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/preview"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(
        preview.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    let preview_content = to_bytes(preview.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(preview_content.as_ref(), pdf);
    let response = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/content"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "inline"
    );
    assert_eq!(
        response.headers().get(header::ACCEPT_RANGES).unwrap(),
        "bytes"
    );
    let content = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(content.as_ref(), pdf);

    let (status, error) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}/read")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("action `read`"));
}

#[tokio::test]
async fn builtin_pdf_preview_action_returns_url_media_view() {
    let app = test_app("core-document-pdf-url-preview").await;
    let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF\n";

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=book.pdf&storage_key=books/url-book.pdf&kind=core%3Adocument")
            .header(header::CONTENT_TYPE, "application/pdf")
            .body(Body::from(pdf.as_slice()))
            .unwrap(),
    )
    .await;
    let resource = response_json(response).await;
    let id = resource["id"].as_str().unwrap();

    let (status, output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{id}/actions/preview"),
        json!({"input": {}}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["view"]["view"], "media");
    assert_eq!(output["view"]["mime_type"], "application/pdf");
    assert_eq!(output["view"]["encoding"], "url");
    assert_eq!(output["view"]["data"], format!("/resources/{id}/content"));
}

#[tokio::test]
async fn image_resource_exposes_builtin_preview_and_thumbnail() {
    let app = test_app("image-preview-thumbnail").await;
    let png_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+yF9sAAAAASUVORK5CYII=";
    let png_bytes = BASE64_STANDARD.decode(png_base64).unwrap();

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=pixel.png&kind=core%3Aimage&storage_key=images%2Fpixel.png",
        "image/png",
        &png_bytes,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(has_action(&resource, "preview"));
    assert!(has_action(&resource, "thumbnail"));
    assert!(has_action(&resource, "view_inline"));
    let id = resource["id"].as_str().unwrap();
    let (content_status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;
    assert_eq!(content_status, StatusCode::OK);
    assert_eq!(content.as_ref(), png_bytes);
    assert_eq!(&content[..8], b"\x89PNG\r\n\x1a\n");

    let preview = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/preview"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(
        preview.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );

    let thumbnail = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/thumbnail"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(thumbnail.status(), StatusCode::OK);
    assert_eq!(
        thumbnail.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
}

#[tokio::test]
async fn builtin_large_image_preview_uses_url() {
    let app = test_app("large-image-url-preview").await;
    let large_image = vec![0u8; 4 * 1024 * 1024 + 1];

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=large.png&storage_key=images/large.png&kind=core%3Aimage")
            .header(header::CONTENT_TYPE, "image/png")
            .body(Body::from(large_image))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let resource = response_json(response).await;
    let id = resource["id"].as_str().unwrap();

    let (status, preview) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{id}/actions/preview"),
        json!({"input": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["view"]["view"], "media");
    assert_eq!(preview["view"]["encoding"], "url");
    assert_eq!(preview["view"]["data"], format!("/resources/{id}/content"));
}

#[tokio::test]
async fn builtin_image_thumbnail_action_stays_inline() {
    let app = test_app("image-thumbnail-inline").await;
    let png_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+yF9sAAAAASUVORK5CYII=";
    let png_bytes = BASE64_STANDARD.decode(png_base64).unwrap();

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=pixel.png&kind=core%3Aimage&storage_key=images%2Fthumbnail-pixel.png",
        "image/png",
        &png_bytes,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = resource["id"].as_str().unwrap();

    let (status, thumbnail) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{id}/actions/thumbnail"),
        json!({"input": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thumbnail}");
    assert_eq!(thumbnail["view"]["view"], "media");
    assert_eq!(thumbnail["view"]["encoding"], "base64");
}

#[tokio::test]
async fn non_reader_resource_rejects_online_reading() {
    let app = test_app("non-reader").await;
    let id = create_text_resource(&app, "read/not-book.txt").await;
    let (status, error) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}/read")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("action `read`"));
}

#[tokio::test]
async fn create_resource_accepts_structured_metadata_and_rejects_metadata_string() {
    let app = test_app("create-resource").await;

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "resources_not_blob",
            "kind": "core:unknown",
            "metadata": {
                "summary": {
                    "description": "metadata-only resource",
                    "tags": ["demo", "document"]
                }
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["name"], "resources_not_blob");
    assert_eq!(resource["kind"], "core:unknown");
    assert!(resource["metadata"].get("schema_version").is_none());
    assert!(resource["metadata"]["kind_metadata"].is_null());
    assert_eq!(
        resource["metadata"]["summary"]["description"],
        "metadata-only resource"
    );
    assert_eq!(
        resource["metadata"]["summary"]["tags"],
        json!(["demo", "document"])
    );

    let id = resource["id"].as_str().unwrap();
    let (status, found) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["id"], id);

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "invalid_metadata",
            "metadata": "{\"A\":\"a\",\"B\":\"b\"}"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("expected struct ResourceMetadataRequest")
    );

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "removed_kind_metadata",
            "metadata": {
                "summary": {"description": null, "tags": []},
                "kind": {}
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("unknown field `kind`")
    );
}

#[tokio::test]
async fn stream_upload_roundtrips_small_blob_and_creates_directories() {
    let app = test_app("small-upload").await;
    let data = b"hello, asset-hub!";

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=hello.txt&kind=core%3Aunknown&directory=examples&storage_key=examples%2Fhello.txt&original_filename=hello.txt&sha256=ee6d5b2c127b5113e886343345d8f11810024201f0c46f54b76d8cc2908c538c",
        "text/plain",
        data,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["key"], "examples/hello.txt");
    assert_eq!(resource["content"]["size"], data.len() as u64);
    assert_eq!(resource["content"]["mime_type"], "text/plain");
    assert_eq!(resource["content"]["checksum"][0]["kind"], "sha256");
    assert_eq!(
        resource["content"]["checksum"][0]["value"],
        "ee6d5b2c127b5113e886343345d8f11810024201f0c46f54b76d8cc2908c538c"
    );

    let id = resource["id"].as_str().unwrap();
    let (status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content.as_ref(), data);

    let (status, directory_resource) = stream_upload(
        &app,
        "/resources/content/stream?name=nested.txt&kind=core%3Afile&directory=examples%2Fnested&original_filename=nested.txt",
        "text/plain",
        b"nested",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(directory_resource["directory"], "examples/nested");
    assert_eq!(
        directory_resource["content"]["key"],
        "examples/nested/nested.txt"
    );

    let (status, root_listing) = empty_json_request(&app, Method::GET, "/directories").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(root_listing["path"], "");
    assert_eq!(root_listing["folders"][0]["path"], "examples");
    assert_eq!(root_listing["resources"]["total"], 0);

    let (status, examples_listing) =
        empty_json_request(&app, Method::GET, "/directories?path=examples").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(examples_listing["folders"][0]["path"], "examples/nested");
    assert_eq!(examples_listing["resources"]["total"], 1);
    assert_eq!(
        examples_listing["resources"]["items"][0]["name"],
        "hello.txt"
    );
}

#[tokio::test]
async fn resource_content_supports_single_byte_ranges_for_video_seek() {
    let app = test_app("content-range").await;
    let data = b"0123456789";
    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=clip.mp4&original_filename=clip.mp4",
        "video/mp4",
        data,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resource}");

    let id = resource["id"].as_str().unwrap();
    let ranged = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/content"))
            .header(header::RANGE, "bytes=2-5")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(ranged.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        ranged.headers().get(header::ACCEPT_RANGES).unwrap(),
        "bytes"
    );
    assert_eq!(
        ranged.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 2-5/10"
    );
    assert_eq!(
        ranged.headers().get(header::CONTENT_TYPE).unwrap(),
        "video/mp4"
    );
    let body = to_bytes(ranged.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(body.as_ref(), b"2345");

    let open_ended = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/content"))
            .header(header::RANGE, "bytes=6-")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(open_ended.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        open_ended.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 6-9/10"
    );
    let body = to_bytes(open_ended.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(body.as_ref(), b"6789");

    let suffix = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/content"))
            .header(header::RANGE, "bytes=-4")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        suffix.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 6-9/10"
    );
    let body = to_bytes(suffix.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(body.as_ref(), b"6789");

    let unsatisfiable = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/content"))
            .header(header::RANGE, "bytes=10-20")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(unsatisfiable.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        unsatisfiable.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes */10"
    );
}

#[tokio::test]
async fn scan_storage_imports_existing_files_idempotently() {
    let app = test_app("scan-storage").await;
    let file_path = app.root.join("blob").join("docs").join("readme.md");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, b"# Existing file\n").unwrap();
    #[cfg(unix)]
    let outside = {
        let outside = unique_temp_root("scan-outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"must not be scanned").unwrap();
        std::os::unix::fs::symlink(&outside, app.root.join("blob").join("outside-link")).unwrap();
        outside
    };

    let (status, scan) = json_request(
        &app,
        Method::POST,
        "/scan",
        json!({ "directory": "docs", "sha256": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scan["scanned_directory"], "docs");
    assert_eq!(scan["scanned"], 1);
    assert_eq!(scan["imported"], 1);
    assert_eq!(scan["skipped"], 0);
    assert_eq!(scan["resources"][0]["name"], "readme.md");
    assert_eq!(scan["resources"][0]["directory"], "docs");
    assert_eq!(scan["resources"][0]["content"]["key"], "docs/readme.md");
    assert_eq!(scan["resources"][0]["content"]["size"], 16);
    assert_eq!(
        scan["resources"][0]["content"]["checksum"][0]["kind"],
        "sha256"
    );
    #[cfg(unix)]
    std::fs::remove_dir_all(outside).unwrap();

    let (status, listing) = empty_json_request(&app, Method::GET, "/directories?path=docs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["resources"]["total"], 1);
    assert_eq!(listing["resources"]["items"][0]["name"], "readme.md");

    let (status, scan) = json_request(&app, Method::POST, "/scan", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scan["scanned"], 1);
    assert_eq!(scan["imported"], 0);
    assert_eq!(scan["skipped"], 1);

    std::fs::remove_file(&file_path).unwrap();
    let (status, audit) = json_request(&app, Method::POST, "/scan", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit["errors"][0]["key"], "docs/readme.md");
    assert!(
        audit["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("missing blob")
    );
}

#[tokio::test]
async fn audit_storage_reports_content_inconsistencies() {
    let app = test_app("audit-storage").await;
    let missing_path = app.root.join("blob").join("docs").join("missing.txt");
    let mismatch_path = app.root.join("blob").join("docs").join("mismatch.txt");
    let orphan_path = app.root.join("blob").join("docs").join("orphan.txt");
    stream_upload(
        &app,
        "/resources/content/stream?name=missing.txt&storage_key=docs%2Fmissing.txt",
        "text/plain",
        b"missing",
    )
    .await;
    stream_upload(
        &app,
        "/resources/content/stream?name=mismatch.txt&storage_key=docs%2Fmismatch.txt",
        "text/plain",
        b"original",
    )
    .await;
    std::fs::remove_file(missing_path).unwrap();
    std::fs::write(mismatch_path, b"changed").unwrap();
    std::fs::write(orphan_path, b"orphan").unwrap();

    let (status, audit) = json_request(
        &app,
        Method::POST,
        "/audit",
        json!({ "directory": "docs", "sha256": true }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit["audited_directory"], "docs");
    assert_eq!(audit["checked_resources"], 2);
    assert_eq!(audit["missing"], 1);
    assert_eq!(audit["orphaned"], 1);
    assert!(audit["mismatched"].as_u64().unwrap() >= 1);
    assert!(
        audit["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| { issue["kind"] == "missing_blob" && issue["key"] == "docs/missing.txt" })
    );
    assert!(audit["issues"].as_array().unwrap().iter().any(|issue| {
        issue["kind"] == "checksum_mismatch" && issue["key"] == "docs/mismatch.txt"
    }));
    assert!(
        audit["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| { issue["kind"] == "orphan_blob" && issue["key"] == "docs/orphan.txt" })
    );
}

#[tokio::test]
async fn upload_rejects_checksum_mismatch_and_existing_storage_key() {
    let app = test_app("upload-security").await;

    let (status, error) = stream_upload(
        &app,
        "/resources/content/stream?name=bad.txt&storage_key=secure%2Fbad.txt&sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "text/plain",
        b"hello, asset-hub!",
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["error"].as_str().unwrap().contains("sha256"));

    let id = create_text_resource(&app, "secure/existing.txt").await;
    assert!(!id.is_empty());

    let (status, error) = stream_upload(
        &app,
        "/resources/content/stream?name=duplicate.txt&storage_key=secure%2Fexisting.txt",
        "text/plain",
        b"hello, asset-hub!",
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["error"].as_str().unwrap().contains("already exists"));
}

#[tokio::test]
async fn stream_upload_roundtrips_large_blob_without_buffered_request_dto() {
    let app = test_app("stream-upload").await;
    let data = b"large file bytes";

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=large-file&directory=streams&original_filename=large.bin&kind=core%3Aunknown&sha256=6d9019b5e7c1d286c231d7f998166f6c036e3f01b972fa46e958e1a5c3750241")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(data.as_slice()))
            .unwrap(),
    )
    .await;

    let status = response.status();
    let resource = response_json(response).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["key"], "streams/large.bin");
    assert_eq!(resource["content"]["size"], data.len() as u64);
    assert_eq!(resource["content"]["mime_type"], "application/octet-stream");

    let id = resource["id"].as_str().unwrap();
    let (status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content.as_ref(), data);
}

#[tokio::test]
async fn stream_upload_is_not_limited_by_the_regular_request_timeout() {
    let app = test_app_with_router_options(
        "stream-upload-timeout",
        RouterOptions {
            cors: CorsPolicy::Origins(vec![header::HeaderValue::from_static(
                "http://127.0.0.1:5173",
            )]),
            request_timeout: Duration::from_millis(1),
            ..RouterOptions::default()
        },
    )
    .await;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(25));
        let _ = sender.send(Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"slow upload",
        )));
    });
    let body = Body::from_stream(stream::once(async move {
        receiver.await.expect("slow upload sender should complete")
    }));
    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=slow.txt&directory=uploads&original_filename=slow.txt")
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::ORIGIN, "http://127.0.0.1:5173")
            .body(body)
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&header::HeaderValue::from_static("http://127.0.0.1:5173"))
    );
}

#[tokio::test]
async fn upload_detects_most_specific_plugin_kind() {
    let app = test_app_with_plugin_manifests(
        "markdown-plugin-detect",
        vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../plugins/azvs-markdown/manifest.json"),
        ],
    )
    .await;
    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=README.md&original_filename=README.md")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("# README"))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let resource = response_json(response).await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "azvs:markdown");
    let actions = resource["actions"]["available_actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|action| action["id"] == "azvs.markdown.render")
    );
    assert!(
        actions
            .iter()
            .any(|action| action["id"] == "download_content")
    );

    let resource_id = resource["id"].as_str().unwrap();
    let (status, rendered) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/azvs.markdown.render"),
        json!({ "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rendered}");
    assert_eq!(rendered["action"], "azvs.markdown.render");
    assert_eq!(rendered["view"]["view"], "plugin_frame");
    assert!(
        rendered["view"]["url"]
            .as_str()
            .unwrap()
            .starts_with("/plugins/azvs.markdown/index.html#payload=")
    );
}

#[tokio::test]
async fn plugin_reference_content_respects_the_host_content_budget() {
    let plugin = PluginHostConfig {
        max_content_bytes: 4,
        ..PluginHostConfig::default()
    };
    let app = test_app_with_plugin_host_config(
        "markdown-plugin-content-budget",
        vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../plugins/azvs-markdown/manifest.json"),
        ],
        plugin,
    )
    .await;
    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=README.md&original_filename=README.md",
        "text/markdown",
        b"# README",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resource}");

    let id = resource["id"].as_str().unwrap();
    let (status, error) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{id}/actions/azvs.markdown.render"),
        json!({"input": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error["code"], "plugin.content_limit_exceeded");
    assert_eq!(error["retryable"], false);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("plugin limit is 4")
    );
}

#[tokio::test]
async fn soft_delete_hides_content_and_purge_removes_resource() {
    let app = test_app("delete-resource").await;
    let id = create_text_resource(&app, "delete/me.txt").await;

    let (status, deleted) =
        empty_json_request(&app, Method::DELETE, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(deleted["deleted_at"].is_string());

    let (status, _) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let response = request(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri(format!("/resources/{id}/purge"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (status, _) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn disabled_purge_endpoint_returns_forbidden() {
    let app = test_app_with_router_options(
        "purge-disabled",
        RouterOptions {
            enable_purge: false,
            ..RouterOptions::default()
        },
    )
    .await;
    let id = create_text_resource(&app, "delete/no-purge.txt").await;

    let (status, error) =
        empty_json_request(&app, Method::DELETE, &format!("/resources/{id}/purge")).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("ASSET_HTTP_ENABLE_PURGE")
    );
}

#[tokio::test]
async fn cors_policy_adds_allowed_origin_header() {
    let app = test_app_with_router_options(
        "cors-origin",
        RouterOptions {
            cors: CorsPolicy::Origins(vec![header::HeaderValue::from_static(
                "http://127.0.0.1:5173",
            )]),
            request_timeout: Duration::from_secs(5),
            ..RouterOptions::default()
        },
    )
    .await;
    let response = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/health")
            .header(header::ORIGIN, "http://127.0.0.1:5173")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&header::HeaderValue::from_static("http://127.0.0.1:5173"))
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        Some(&header::HeaderValue::from_static("true"))
    );
}

#[tokio::test]
async fn list_resources_filters_by_kind_tag_and_query() {
    let app = test_app("list-resources").await;

    let (_, first) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "alpha document",
            "kind": "core:unknown",
            "metadata": {
                "summary": {
                    "tags": ["alpha", "docs"]
                }
            }
        }),
    )
    .await;
    let (_, second) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "beta image",
            "kind": "core:unknown",
            "metadata": {
                "summary": {
                    "tags": ["beta", "media"]
                }
            }
        }),
    )
    .await;
    let (_, third) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "alpha image",
            "kind": "core:unknown",
            "metadata": {
                "summary": {
                    "tags": ["alpha", "media"]
                }
            }
        }),
    )
    .await;

    assert!(first["id"].is_string());
    assert!(second["id"].is_string());
    assert!(third["id"].is_string());

    let (status, page) = empty_json_request(
        &app,
        Method::GET,
        "/resources?kind=core%3Aunknown&tag=alpha&q=image&page=1&limit=10",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1);
    assert_eq!(page["page"], 1);
    assert_eq!(page["limit"], 10);
    assert_eq!(page["items"][0]["name"], "alpha image");
    assert_eq!(page["items"][0]["status"], "active");
}

#[tokio::test]
async fn kind_filter_can_include_all_descendants() {
    let app = test_app_with_kind_definitions(
        "kind-descendant-filter",
        vec![
            ResourceKindConfig {
                kind: "core:code".to_string(),
                parent: Some("core:document".to_string()),
                label: Some("Code".to_string()),
                ..ResourceKindConfig::default()
            },
            ResourceKindConfig {
                kind: "code:c".to_string(),
                parent: Some("core:code".to_string()),
                label: Some("C".to_string()),
                ..ResourceKindConfig::default()
            },
            ResourceKindConfig {
                kind: "code:cpp".to_string(),
                parent: Some("core:code".to_string()),
                label: Some("C++".to_string()),
                ..ResourceKindConfig::default()
            },
        ],
    )
    .await;

    for (name, kind) in [
        ("generic code", "core:code"),
        ("main.c", "code:c"),
        ("main.cpp", "code:cpp"),
    ] {
        let (status, _) = json_request(
            &app,
            Method::POST,
            "/resources",
            json!({ "name": name, "kind": kind }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, exact) =
        empty_json_request(&app, Method::GET, "/resources?kind=core%3Acode").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(exact["total"], 1);

    let (status, hierarchy) = empty_json_request(
        &app,
        Method::GET,
        "/resources?kind=core%3Acode&include_descendants=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hierarchy["total"], 3);

    let (status, kinds) = empty_json_request(&app, Method::GET, "/resource-kinds").await;
    assert_eq!(status, StatusCode::OK);
    let c = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "code:c")
        .unwrap();
    assert_eq!(c["parent"], "core:code");
    assert_eq!(
        c["ancestors"],
        json!(["core:code", "core:document", "core:file"])
    );
    assert!(
        c["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "download_content")
    );
}

#[tokio::test]
async fn update_resource_changes_fields_and_restores_soft_deleted_resource() {
    let app = test_app("update-resource").await;
    let id = create_text_resource(&app, "update/me.txt").await;

    let (status, updated) = json_request(
        &app,
        Method::PATCH,
        &format!("/resources/{id}"),
        json!({
            "name": "updated.txt",
            "kind": "core:unknown",
            "status": "archived",
            "metadata": {
                "summary": {
                    "description": "updated resource",
                    "tags": ["updated"]
                }
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "updated.txt");
    assert_eq!(updated["kind"], "core:unknown");
    assert_eq!(updated["status"], "archived");
    assert_eq!(updated["metadata"]["summary"]["tags"], json!(["updated"]));

    let (status, _) = empty_json_request(&app, Method::DELETE, &format!("/resources/{id}")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, restored) = json_request(
        &app,
        Method::PATCH,
        &format!("/resources/{id}"),
        json!({
            "restore": true,
            "status": "active"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["status"], "active");
    assert!(restored["deleted_at"].is_null());
}

#[tokio::test]
async fn openapi_documents_metadata_examples() {
    let app = test_app("openapi").await;
    let (status, document) = empty_json_request(&app, Method::GET, "/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);

    let metadata_example = &document["components"]["schemas"]["ResourceMetadataRequest"]["example"];
    let create_example = &document["components"]["schemas"]["CreateResourceRequest"]["example"];
    assert_eq!(
        metadata_example["summary"]["description"],
        "Human readable resource description"
    );
    assert_eq!(
        create_example["metadata"]["summary"]["description"],
        "A metadata-only resource"
    );
    assert!(document["paths"].get("/resources/content").is_none());
    assert!(document["paths"].get("/resources/content/stream").is_some());
    assert!(document["paths"].get("/auth/login").is_some());
    assert!(document["paths"].get("/auth/users/{id}").is_some());
    assert_eq!(
        document["components"]["securitySchemes"]["cookie_auth"]["in"],
        "cookie"
    );
}

#[tokio::test]
async fn plugin_web_assets_are_served_from_the_verified_startup_snapshot() {
    let app_root = unique_temp_root("plugin-web");
    let web_root = app_root.join("plugins/azvs-markdown/dist");
    std::fs::create_dir_all(&web_root).unwrap();
    std::fs::write(
        web_root.join("index.html"),
        "<!doctype html><title>Markdown</title>",
    )
    .unwrap();

    let app = test_app_with_plugin_web_assets(
        app_root,
        HashMap::from([(
            "azvs.markdown".to_string(),
            HashMap::from([(
                PathBuf::from("index.html"),
                std::sync::Arc::from(b"<!doctype html><title>Markdown</title>".as_slice()),
            )]),
        )]),
    )
    .await;
    std::fs::write(
        web_root.join("index.html"),
        "<!doctype html><title>Changed after startup</title>",
    )
    .unwrap();
    let response = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/plugins/azvs.markdown/index.html")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let status = response.status();
    assert_eq!(
        response.headers()["content-security-policy"],
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' data:; font-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'"
    );
    let body = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<!doctype html><title>Markdown</title>");
}

async fn test_app(name: &str) -> TestApp {
    test_app_with_kind_definitions(name, Vec::new()).await
}

async fn test_app_with_router_options(name: &str, options: RouterOptions) -> TestApp {
    test_app_with_kind_definitions_and_router_options(name, Vec::new(), options).await
}

async fn test_app_with_plugin_manifests(name: &str, plugin_manifests: Vec<PathBuf>) -> TestApp {
    test_app_with_plugin_host_config(name, plugin_manifests, PluginHostConfig::default()).await
}

async fn test_app_with_plugin_host_config(
    name: &str,
    plugin_manifests: Vec<PathBuf>,
    plugin: PluginHostConfig,
) -> TestApp {
    test_app_with_kind_and_plugin_config(
        name,
        KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifests,
        },
        plugin,
        RouterOptions::default(),
    )
    .await
}

async fn test_app_with_kind_definitions(
    name: &str,
    kind_definitions: Vec<ResourceKindConfig>,
) -> TestApp {
    test_app_with_kind_definitions_and_router_options(
        name,
        kind_definitions,
        RouterOptions::default(),
    )
    .await
}

async fn test_app_with_kind_definitions_and_router_options(
    name: &str,
    kind_definitions: Vec<ResourceKindConfig>,
    options: RouterOptions,
) -> TestApp {
    test_app_with_config(
        name,
        KindRegistryConfig {
            definitions: kind_definitions,
            plugin_manifests: Vec::new(),
        },
        options,
    )
    .await
}

async fn test_app_with_config(
    name: &str,
    kind: KindRegistryConfig,
    options: RouterOptions,
) -> TestApp {
    test_app_with_kind_and_plugin_config(name, kind, PluginHostConfig::default(), options).await
}

async fn test_app_with_kind_and_plugin_config(
    name: &str,
    kind: KindRegistryConfig,
    plugin: PluginHostConfig,
    options: RouterOptions,
) -> TestApp {
    let root = unique_temp_root(name);
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite_path: root.join("asset-hub.sqlite"),
            max_connections: 1,
        },
        blob: BlobConfig {
            fs_root: root.join("blob"),
        },
        kind,
        plugin,
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = router::build_with_options_and_plugin_web_assets(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        options,
        HashMap::new(),
        authorization,
    )
    .layer(Extension(AccessContext::administrator(UserId::new())));

    TestApp { router, root }
}

async fn test_app_with_plugin_web_assets(
    root: PathBuf,
    plugin_web_assets: asset_infra::PluginWebAssets,
) -> TestApp {
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite_path: root.join("asset-hub.sqlite"),
            max_connections: 1,
        },
        blob: BlobConfig {
            fs_root: root.join("blob"),
        },
        kind: KindRegistryConfig::default(),
        plugin: Default::default(),
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = router::build_with_options_and_plugin_web_assets(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        RouterOptions::default(),
        plugin_web_assets,
        authorization,
    )
    .layer(Extension(AccessContext::administrator(UserId::new())));

    TestApp { router, root }
}

async fn create_text_resource(app: &TestApp, storage_key: &str) -> String {
    let uri = format!(
        "/resources/content/stream?name=delete-me.txt&storage_key={}",
        storage_key.replace('/', "%2F")
    );
    let (status, resource) = stream_upload(app, &uri, "text/plain", b"delete me").await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["kind"], "core:file");

    resource["id"].as_str().unwrap().to_string()
}

async fn stream_upload(
    app: &TestApp,
    uri: &str,
    content_type: &str,
    data: impl AsRef<[u8]>,
) -> (StatusCode, Value) {
    let response = request(
        app,
        Request::builder()
            .method(Method::PUT)
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(data.as_ref().to_vec()))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let body = response_json(response).await;
    (status, body)
}

#[tokio::test]
async fn member_uses_explicit_workspace_and_additional_grants() {
    let root = unique_temp_root("authenticated-directory-acl");
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite_path: root.join("asset-hub.sqlite"),
            max_connections: 2,
        },
        blob: BlobConfig {
            fs_root: root.join("blob"),
        },
        kind: KindRegistryConfig::default(),
        plugin: Default::default(),
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let base = router::build_with_options_and_plugin_web_assets(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        RouterOptions::default(),
        HashMap::from([(
            "azvs.markdown".to_string(),
            HashMap::from([(
                PathBuf::from("viewer.js"),
                std::sync::Arc::from(b"document.body.textContent = 'loaded'".as_slice()),
            )]),
        )]),
        authorization.clone(),
    );
    let router = router::with_authentication(
        base,
        runtime.user_service(),
        authorization,
        &runtime.config().database.sqlite_path,
        Some(("admin", "administrator-password")),
        &SessionOptions {
            cookie_secure: false,
            inactivity_timeout: Duration::from_secs(3600),
        },
    )
    .await
    .unwrap();
    let app = TestApp { router, root };

    let health = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(health.status(), StatusCode::OK);
    let health = response_json(health).await;
    assert_eq!(health["status"], "ready");
    assert_eq!(health["database"]["status"], "ready");
    assert_eq!(health["blob_storage"]["status"], "ready");

    let plugin_asset = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/plugins/azvs.markdown/viewer.js")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(plugin_asset.status(), StatusCode::OK);
    assert_eq!(
        plugin_asset.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "*"
    );

    let openapi = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/api-docs/openapi.json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(openapi.status(), StatusCode::OK);

    let swagger = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/swagger-ui/")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(swagger.status(), StatusCode::OK);

    let unauthenticated = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri("/resources?directory=teams/alice")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let oversized_login = request_with_cookie(
        &app,
        Method::POST,
        "/auth/login",
        json!({ "username": "admin", "password": "x".repeat(crate::auth::MAX_LOGIN_REQUEST_BYTES) }),
        "",
    )
    .await;
    assert_eq!(oversized_login.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let failed_login = request_with_cookie(
        &app,
        Method::POST,
        "/auth/login",
        json!({ "username": "admin", "password": "incorrect-password" }),
        "",
    )
    .await;
    assert_eq!(failed_login.status(), StatusCode::UNAUTHORIZED);

    let (admin_cookie, admin_login) =
        login_with_password(&app, "admin", "administrator-password").await;
    assert_eq!(admin_login["user"]["workspace_directory"], "");
    let response = request_with_cookie(
        &app,
        Method::POST,
        "/auth/users",
        json!({
            "username": "alice",
            "password": "alice-secure-password",
            "is_admin": false,
            "workspace_directory": "teams/alice"
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let alice = response_json(response).await;
    let alice_id = alice["user"]["id"].as_str().unwrap();
    assert_eq!(alice["user"]["workspace_directory"], "teams/alice");
    let workspace_grants = request_with_cookie(
        &app,
        Method::GET,
        &format!("/auth/directory-grants?user_id={alice_id}"),
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(workspace_grants.status(), StatusCode::OK);
    assert_eq!(
        response_json(workspace_grants).await,
        json!([{
            "directory": "teams/alice",
            "permission": "full",
            "is_workspace": true
        }])
    );
    let downgrade_workspace = request_with_cookie(
        &app,
        Method::PUT,
        "/auth/directory-grants",
        json!({
            "user_id": alice_id,
            "directory": "teams/alice",
            "permission": "read"
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(downgrade_workspace.status(), StatusCode::CONFLICT);
    let revoke_workspace = request_with_cookie(
        &app,
        Method::DELETE,
        &format!("/auth/directory-grants?user_id={alice_id}&directory=teams%2Falice"),
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(revoke_workspace.status(), StatusCode::CONFLICT);
    let root_member = request_with_cookie(
        &app,
        Method::POST,
        "/auth/users",
        json!({
            "username": "root-member",
            "password": "root-member-password",
            "is_admin": false,
            "workspace_directory": ""
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(root_member.status(), StatusCode::CREATED);
    let root_member = response_json(root_member).await;
    let root_member_id = root_member["user"]["id"].as_str().unwrap();
    let root_entries = request_with_cookie(
        &app,
        Method::GET,
        &format!("/auth/directory-grants?user_id={root_member_id}"),
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(root_entries.status(), StatusCode::OK);
    assert_eq!(
        response_json(root_entries).await,
        json!([{ "directory": "", "permission": "full", "is_workspace": true }])
    );
    let teams = request_with_cookie(
        &app,
        Method::GET,
        "/directories?path=teams",
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(teams.status(), StatusCode::OK);
    let teams = response_json(teams).await;
    assert!(
        teams["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|directory| directory["path"] == "teams/alice")
    );

    let response = request_with_cookie(
        &app,
        Method::PUT,
        "/auth/directory-grants",
        json!({
            "user_id": alice_id, "directory": "shared", "permission": "write"
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = request_with_cookie(
        &app,
        Method::PUT,
        "/auth/directory-grants",
        json!({
            "user_id": alice_id, "directory": "shared/photos", "permission": "read"
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = request_with_cookie(
        &app,
        Method::PUT,
        "/auth/directory-grants",
        json!({
            "user_id": alice_id, "directory": "public", "permission": "read"
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let all_entries = request_with_cookie(
        &app,
        Method::GET,
        &format!("/auth/directory-grants?user_id={alice_id}"),
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(all_entries.status(), StatusCode::OK);
    assert_eq!(
        response_json(all_entries).await,
        json!([
            { "directory": "public", "permission": "read", "is_workspace": false },
            { "directory": "shared", "permission": "write", "is_workspace": false },
            { "directory": "shared/photos", "permission": "read", "is_workspace": false },
            { "directory": "teams/alice", "permission": "full", "is_workspace": true }
        ])
    );

    let (alice_cookie, login) = login_with_password(&app, "alice", "alice-secure-password").await;
    assert_eq!(login["user"]["workspace_directory"], "teams/alice");
    let scan = request_with_cookie(&app, Method::POST, "/scan", json!({}), &alice_cookie).await;
    assert_eq!(scan.status(), StatusCode::FORBIDDEN);
    let folder = request_with_cookie(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_path": "teams/alice", "name": "empty-folder" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(folder.status(), StatusCode::CREATED);
    let folder = response_json(folder).await;
    assert_eq!(folder["path"], "teams/alice/empty-folder");

    let denied_folder = request_with_cookie(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_path": "teams/bob", "name": "forbidden" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(denied_folder.status(), StatusCode::FORBIDDEN);

    let allowed = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "allowed", "directory": "teams/alice"
        }),
        &alice_cookie,
    )
    .await;
    assert_eq!(allowed.status(), StatusCode::CREATED);
    let denied = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "denied", "directory": "teams/bob"
        }),
        &alice_cookie,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let shared_write = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({ "name": "shared-write", "directory": "shared/photos" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(shared_write.status(), StatusCode::CREATED);
    let public_write = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({ "name": "forbidden-public-write", "directory": "public" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(public_write.status(), StatusCode::FORBIDDEN);

    let delegated_grant = request_with_cookie(
        &app,
        Method::PUT,
        "/auth/directory-grants",
        json!({
            "user_id": alice_id,
            "directory": "teams/alice/shared",
            "permission": "read"
        }),
        &alice_cookie,
    )
    .await;
    assert_eq!(delegated_grant.status(), StatusCode::FORBIDDEN);

    let disabled = request_with_cookie(
        &app,
        Method::PATCH,
        &format!("/auth/users/{alice_id}"),
        json!({ "status": "disabled" }),
        &admin_cookie,
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::OK);
    let stale_session =
        request_with_cookie(&app, Method::GET, "/auth/me", json!({}), &alice_cookie).await;
    assert_eq!(stale_session.status(), StatusCode::UNAUTHORIZED);

    let audit_events = request_with_cookie(
        &app,
        Method::GET,
        "/auth/audit-events?limit=500",
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(audit_events.status(), StatusCode::OK);
    let audit_events = response_json(audit_events).await;
    let events = audit_events.as_array().unwrap();
    assert!(events.iter().any(|event| {
        event["event_type"] == "auth.login"
            && event["outcome"] == "failure"
            && event["target"] == "admin"
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "auth.directory_grant.update"
            && event["actor_username"] == "admin"
            && event["outcome"] == "success"
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "auth.user.status"
            && event["actor_username"] == "admin"
            && event["status_code"] == 200
    }));
}

async fn login_with_password(app: &TestApp, username: &str, password: &str) -> (String, Value) {
    let response = request_with_cookie(
        app,
        Method::POST,
        "/auth/login",
        json!({ "username": username, "password": password }),
        "",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body = response_json(response).await;
    (cookie, body)
}

async fn request_with_cookie(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Value,
    cookie: &str,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    request(app, builder.body(Body::from(body.to_string())).unwrap()).await
}

async fn json_request(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = request(
        app,
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let body = response_json(response).await;

    (status, body)
}

async fn empty_json_request(app: &TestApp, method: Method, uri: &str) -> (StatusCode, Value) {
    let response = request(
        app,
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let status = response.status();
    let body = response_json(response).await;

    (status, body)
}

async fn empty_bytes_request(
    app: &TestApp,
    method: Method,
    uri: &str,
) -> (StatusCode, axum::body::Bytes) {
    let response = request(
        app,
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let status = response.status();
    let body = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();

    (status, body)
}

async fn request(app: &TestApp, request: Request<Body>) -> axum::response::Response {
    app.router.clone().oneshot(request).await.unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();

    if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    }
}

fn has_action(resource: &Value, id: &str) -> bool {
    resource["actions"]["available_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action["id"] == id))
}

fn minimal_epub() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    writer.start_file("OPS/chapter1.xhtml", options).unwrap();
    writer
        .write_all(br#"<html><body><h1>Chapter One</h1><p>Hello &amp; EPUB.</p></body></html>"#)
        .unwrap();

    writer.finish().unwrap().into_inner()
}

fn unique_temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    std::env::temp_dir().join(format!(
        "asset-hub-http-test-{name}-{}-{nanos}",
        std::process::id()
    ))
}
