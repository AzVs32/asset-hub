use crate::router;
use asset_apps::AssetRuntime;
use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, KindRegistryConfig, ResourceKindConfig,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use std::io::{Cursor, Write};
use std::path::PathBuf;
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
    assert!(
        kinds["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| { kind["kind"] == "core:unknown" && kind["schema_id"].is_null() })
    );
    for (kind, source) in [
        ("core:file", "plugin:core-file"),
        ("core:image", "plugin:core-image"),
        ("core:document", "plugin:core-document"),
        ("core:video", "plugin:core-video"),
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
            schema_id: Some("doc:note@1".to_string()),
            metadata_schema: Some(json!({
                "type": "object",
                "properties": {
                    "topic": { "type": "string" }
                }
            })),
            supports_content: false,
            actions: Vec::new(),
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
    assert_eq!(note_kind["schema_id"], "doc:note@1");
    assert_eq!(note_kind["source"], "config");
    assert_eq!(note_kind["supports_content"], false);
    assert_eq!(note_kind["metadata_schema"]["type"], "object");

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
    assert_eq!(resource["actions"]["download_content"], false);
    assert_eq!(resource["actions"]["read"], false);
    assert_eq!(resource["actions"]["view_inline"], false);

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "note.txt",
            "kind": "doc:note",
            "storage_key": "notes/note.txt",
            "data_base64": "bm90ZQ=="
        }),
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
    assert_eq!(document_kind["source"], "plugin:core-document");
    assert_eq!(
        document_kind["actions"],
        json!([
            {"id": "download_content", "label": "Download", "access": "read_only"},
            {"id": "view_inline", "label": "View", "access": "read_only"},
            {"id": "preview", "label": "Preview", "access": "read_only"}
        ])
    );

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "book.txt",
            "kind": "core:document",
            "storage_key": "books/book.txt",
            "data_base64": "SGVsbG8gYm9vaw==",
            "mime_type": "text/plain"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let id = resource["id"].as_str().unwrap();
    assert_eq!(resource["actions"]["download_content"], true);
    assert_eq!(resource["actions"]["read"], false);
    assert_eq!(resource["actions"]["view_inline"], false);
    let (status, error) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}/read")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("action `read`"));
}

#[tokio::test]
async fn action_endpoint_requires_plugin_handler() {
    let app = test_app("action-endpoint-handler").await;
    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "book.txt",
            "kind": "core:document",
            "storage_key": "books/action-book.txt",
            "data_base64": "SGVsbG8gYWN0aW9u",
            "mime_type": "text/plain"
        }),
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
    assert_eq!(resource["actions"]["download_content"], true);
    assert_eq!(resource["actions"]["read"], false);
    assert_eq!(resource["actions"]["view_inline"], true);
    assert_eq!(resource["actions"]["preview"], true);

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
async fn image_resource_exposes_builtin_preview_and_thumbnail() {
    let app = test_app("image-preview-thumbnail").await;
    let png_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+yF9sAAAAASUVORK5CYII=";

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "pixel.png",
            "kind": "core:image",
            "storage_key": "images/pixel.png",
            "data_base64": png_base64,
            "mime_type": "image/png"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["actions"]["preview"], true);
    assert_eq!(resource["actions"]["thumbnail"], true);
    assert_eq!(resource["actions"]["view_inline"], true);
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
                },
                "kind": {
                    "schema_id": "test:metadata@1",
                    "data": {
                        "A": "a",
                        "B": "b"
                    }
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
    assert_eq!(resource["metadata"]["kind"]["data"]["A"], "a");

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
}

#[tokio::test]
async fn upload_resource_content_roundtrips_small_blob() {
    let app = test_app("small-upload").await;
    let data = b"hello, asset-hub!";

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "hello.txt",
            "kind": "core:unknown",
            "storage_key": "examples/hello.txt",
            "data_base64": "aGVsbG8sIGFzc2V0LWh1YiE=",
            "metadata": {
                "summary": {
                    "description": "small text file",
                    "tags": ["demo", "text"]
                },
                "kind": {
                    "schema_id": "test:metadata@1",
                    "data": {
                        "source": "test"
                    }
                }
            },
            "mime_type": "text/plain",
            "original_filename": "hello.txt",
            "sha256": "ee6d5b2c127b5113e886343345d8f11810024201f0c46f54b76d8cc2908c538c"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["key"], "examples/hello.txt");
    assert_eq!(resource["content"]["size"], data.len() as u64);
    assert_eq!(resource["content"]["mime_type"], "text/plain");
    assert_eq!(resource["metadata"]["kind"]["data"]["source"], "test");

    let id = resource["id"].as_str().unwrap();
    let (status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content.as_ref(), data);
}

#[tokio::test]
async fn upload_rejects_checksum_mismatch_and_existing_storage_key() {
    let app = test_app("upload-security").await;

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "bad.txt",
            "storage_key": "secure/bad.txt",
            "data_base64": "aGVsbG8sIGFzc2V0LWh1YiE=",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["error"].as_str().unwrap().contains("sha256"));

    let id = create_text_resource(&app, "secure/existing.txt").await;
    assert!(!id.is_empty());

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "duplicate.txt",
            "storage_key": "secure/existing.txt",
            "data_base64": "aGVsbG8sIGFzc2V0LWh1YiE="
        }),
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
            .uri("/resources/content/stream?name=large-file&storage_key=streams/large.bin&kind=core%3Aunknown&sha256=6d9019b5e7c1d286c231d7f998166f6c036e3f01b972fa46e958e1a5c3750241")
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
                },
                "kind": {
                    "schema_id": "test:metadata@1",
                    "data": {
                        "source": "patch"
                    }
                }
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "updated.txt");
    assert_eq!(updated["kind"], "core:unknown");
    assert_eq!(updated["status"], "archived");
    assert_eq!(updated["metadata"]["kind"]["data"]["source"], "patch");

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
    let upload_example =
        &document["components"]["schemas"]["UploadResourceContentRequest"]["example"];

    assert_eq!(metadata_example["kind"]["data"]["source"], "swagger");
    assert_eq!(
        create_example["metadata"]["kind"]["schema_id"],
        "test:metadata@1"
    );
    assert_eq!(upload_example["data_base64"], "aGVsbG8sIGFzc2V0LWh1YiE=");
}

async fn test_app(name: &str) -> TestApp {
    test_app_with_kind_definitions(name, Vec::new()).await
}

async fn test_app_with_kind_definitions(
    name: &str,
    kind_definitions: Vec<ResourceKindConfig>,
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
        kind: KindRegistryConfig {
            definitions: kind_definitions,
            plugin_manifest_dirs: Vec::new(),
        },
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let router = router::build(runtime.resource_service(), runtime.resource_kind_registry());

    TestApp { router, root }
}

async fn create_text_resource(app: &TestApp, storage_key: &str) -> String {
    let (status, resource) = json_request(
        app,
        Method::POST,
        "/resources/content",
        json!({
            "name": "delete-me.txt",
            "storage_key": storage_key,
            "data_base64": "ZGVsZXRlIG1l",
            "mime_type": "text/plain"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["kind"], "core:file");

    resource["id"].as_str().unwrap().to_string()
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
