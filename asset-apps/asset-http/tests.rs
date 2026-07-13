use crate::router;
use crate::settings::{CorsPolicy, RouterOptions, SessionOptions};
use asset_apps::AssetRuntime;
use asset_core::domain::{AccessContext, UserId};
use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, KindRegistryConfig, ResourceKindConfig,
};
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::{Extension, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
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
    assert_eq!(download["requires"]["content_delivery"], "url");
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
    assert_eq!(view_inline["requires"]["metadata"], true);
    assert_eq!(view_inline["requires"]["content_delivery"], "url");
    assert_eq!(view_inline["output"]["view"], json!(["media"]));
    assert_eq!(
        view_inline["applies_to"],
        json!({
            "kinds": ["core:image", "core:document", "core:video"],
            "mime_types": ["image/*", "application/pdf", "video/*"],
            "extensions": [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif", ".pdf", ".mp4", ".webm", ".mov", ".m4v", ".ogv"]
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
    assert_eq!(resource["metadata"]["schema_version"], 1);
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

    let (status, scan) = json_request(&app, Method::POST, "/scan", json!({ "sha256": true })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scan["path"], "");
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
async fn upload_detects_most_specific_plugin_kind() {
    let app = test_app_with_plugin_manifests(
        "markdown-plugin-detect",
        vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../plugins/azvs-markdown/azvs-markdown.json"),
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
async fn plugin_web_assets_are_served_from_declared_roots() {
    let app_root = unique_temp_root("plugin-web");
    let web_root = app_root.join("plugins/azvs-markdown/web");
    std::fs::create_dir_all(&web_root).unwrap();
    std::fs::write(
        web_root.join("index.html"),
        "<!doctype html><title>Markdown</title>",
    )
    .unwrap();

    let app = test_app_with_plugin_web_roots(
        app_root,
        HashMap::from([("azvs.markdown".to_string(), web_root)]),
    )
    .await;
    let (status, body) =
        empty_bytes_request(&app, Method::GET, "/plugins/azvs.markdown/index.html").await;

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
    test_app_with_config(
        name,
        KindRegistryConfig {
            definitions: Vec::new(),
            plugin_manifests,
        },
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
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = router::build_with_options_and_plugin_web_roots(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        options,
        HashMap::new(),
        authorization,
    )
    .layer(Extension(AccessContext::administrator(UserId::new())));

    TestApp { router, root }
}

async fn test_app_with_plugin_web_roots(
    root: PathBuf,
    plugin_web_roots: HashMap<String, PathBuf>,
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
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = router::build_with_options_and_plugin_web_roots(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        RouterOptions::default(),
        plugin_web_roots,
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
async fn member_starts_in_home_directory_and_uses_additional_grants() {
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
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let base = router::build_with_options_and_plugin_web_roots(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        RouterOptions::default(),
        HashMap::new(),
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

    let (admin_cookie, admin_login) =
        login_with_password(&app, "admin", "administrator-password").await;
    assert_eq!(admin_login["user"]["home_directory"], "");
    let response = request_with_cookie(
        &app,
        Method::POST,
        "/auth/users",
        json!({
            "username": "alice",
            "password": "alice-secure-password",
            "is_admin": false,
            "home_directory": "teams/alice"
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let alice = response_json(response).await;
    let alice_id = alice["user"]["id"].as_str().unwrap();
    assert_eq!(alice["user"]["home_directory"], "teams/alice");
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

    let (alice_cookie, login) = login_with_password(&app, "alice", "alice-secure-password").await;
    assert_eq!(login["user"]["home_directory"], "teams/alice");
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
