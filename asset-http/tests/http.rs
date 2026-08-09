use asset_core::domain::{AccessContext, UserId, UserRole};
use asset_http::{
    CorsPolicy, HttpSessionRuntime, MAX_ACTION_REQUEST_BYTES, MAX_LOGIN_REQUEST_BYTES,
    RouterOptions, SessionOptions, build_router, with_authentication,
};
use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, LocalBlobConfig, LocalBlobSyncConfig,
    PluginHostConfig, SqliteDatabaseConfig,
};
use asset_runtime::AssetRuntime;
use asset_runtime::PluginWebAssets;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::{Extension, Router};
use bytes::Bytes;
use futures_util::stream;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;
use tower_sessions_sqlx_store::sqlx::sqlite::SqliteConnectOptions;
use tower_sessions_sqlx_store::sqlx::{SqlitePool, query_scalar};

const BODY_LIMIT: usize = 1024 * 1024;
const ROOT_DIRECTORY_ID: &str = "00000000-0000-0000-0000-000000000000";

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
    let default = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:resource")
        .unwrap();
    assert!(default["parent"].is_null());
    for (kind, source) in [
        ("core:resource", "builtin:core.resource"),
        ("core:image", "builtin:core.image"),
        ("core:text", "builtin:core.text"),
        ("core:video", "builtin:core.video"),
    ] {
        assert!(
            kinds["items"].as_array().unwrap().iter().any(|item| {
                item["kind"] == kind
                    && item["origin"]
                        == json!({"kind": "builtin", "id": source.trim_start_matches("builtin:")})
            }),
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
        .find(|kind| kind["kind"] == "core:resource")
        .unwrap();
    assert!(file_kind.get("detect").is_none());

    let (status, error) = stream_upload(
        &app,
        "/resources?name=unsupported.bin&kind=plugin%3Anot-installed",
        "application/octet-stream",
        b"unsupported",
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
async fn directory_kinds_and_directory_capabilities_are_exposed() {
    let app = test_app("directory-kinds").await;
    let (status, kinds) = empty_json_request(&app, Method::GET, "/directory-kinds").await;

    assert_eq!(status, StatusCode::OK);
    let default = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:directory")
        .unwrap();
    assert!(default["parent"].is_null());
    assert_eq!(default["label"], "Directory");
    assert_eq!(
        default["origin"],
        json!({"kind": "builtin", "id": "core.directory"})
    );
    let download = default["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["id"] == "core.directory.download")
        .unwrap();
    assert_eq!(download["access"], "read");
    assert!(download.get("executor").is_none());
    assert_eq!(download["output"]["views"], json!(["download"]));
    let thumbnail = default["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["id"] == "core.directory.thumbnail")
        .unwrap();
    assert_eq!(thumbnail["output"]["views"], json!(["media"]));
    assert_eq!(
        thumbnail["ui"]["locations"],
        json!(["directory_list_thumbnail"])
    );

    let (status, directory) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({
            "parent_id": ROOT_DIRECTORY_ID,
            "name": "typed",
            "kind": "core:directory"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{directory}");
    assert_eq!(directory["kind"], "core:directory");
    assert!(has_directory_action(&directory, "core.directory.download"));
    assert!(has_directory_action(&directory, "core.directory.thumbnail"));

    let directory_id = directory["id"].as_str().unwrap();
    let (status, thumbnail_output) = json_request(
        &app,
        Method::POST,
        &format!("/directories/{directory_id}/actions/core.directory.thumbnail"),
        json!({ "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thumbnail_output}");
    assert_eq!(thumbnail_output["view"]["view"], "media");
    assert_eq!(thumbnail_output["view"]["mime_type"], "image/svg+xml");
    assert_eq!(thumbnail_output["view"]["encoding"], "base64");

    let (status, listing) = empty_json_request(&app, Method::GET, "/directories").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["directory"]["kind"], "core:directory");
    let listed = listing["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["id"] == directory["id"])
        .unwrap();
    assert_eq!(listed["kind"], "core:directory");
    assert!(has_directory_action(listed, "core.directory.download"));
    assert!(has_directory_action(listed, "core.directory.thumbnail"));

    let (status, error) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({
            "parent_id": ROOT_DIRECTORY_ID,
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
            .contains("unsupported directory kind")
    );
}

#[tokio::test]
async fn directory_download_action_archives_nested_resources_and_empty_directories() {
    let app = test_app("directory-download").await;
    let (status, directory) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({"parent_id": ROOT_DIRECTORY_ID, "name": "bundle"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{directory}");
    let directory_id = directory["id"].as_str().unwrap();

    let (status, _) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({"parent_id": directory_id, "name": "empty"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = stream_upload(
        &app,
        "/resources?name=top.txt&directory=bundle",
        "text/plain",
        b"top-level",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = stream_upload(
        &app,
        "/resources?name=readme.txt&directory=bundle%2Fnested",
        "text/plain",
        b"nested-content",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, output) = json_request(
        &app,
        Method::POST,
        &format!("/directories/{directory_id}/actions/core.directory.download"),
        json!({"expected_revision": directory["revision"], "input": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["view"]["view"], "download");
    assert_eq!(output["view"]["mime_type"], "application/zip");
    assert_eq!(output["view"]["filename"], "bundle.zip");
    assert_eq!(
        output["view"]["url"],
        format!("/directories/{directory_id}/download")
    );

    let response = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/directories/{directory_id}/download"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/zip");
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("bundle.zip")
    );
    let body = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
    let mut names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            "bundle/",
            "bundle/empty/",
            "bundle/nested/",
            "bundle/nested/readme.txt",
            "bundle/top.txt",
        ]
    );
    let mut top = String::new();
    archive
        .by_name("bundle/top.txt")
        .unwrap()
        .read_to_string(&mut top)
        .unwrap();
    assert_eq!(top, "top-level");
    let mut nested = String::new();
    archive
        .by_name("bundle/nested/readme.txt")
        .unwrap()
        .read_to_string(&mut nested)
        .unwrap();
    assert_eq!(nested, "nested-content");
}

#[tokio::test]
async fn core_text_resource_inherits_generic_actions_and_provides_text_actions() {
    let app = test_app("core-text-actions").await;

    let (status, kinds) = empty_json_request(&app, Method::GET, "/resource-kinds").await;
    assert_eq!(status, StatusCode::OK);
    let text_kind = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:text")
        .unwrap();
    assert_eq!(
        text_kind["origin"],
        json!({"kind": "builtin", "id": "core.text"})
    );
    assert!(
        text_kind["detect"]["mime_types"]
            .as_array()
            .unwrap()
            .contains(&json!("text/*"))
    );
    assert!(
        text_kind["detect"]["extensions"]
            .as_array()
            .unwrap()
            .contains(&json!(".txt"))
    );
    let actions = text_kind["actions"].as_array().unwrap();
    let download = actions
        .iter()
        .find(|action| action["id"] == "core.resource.download")
        .unwrap();
    assert_eq!(download["label"], "Download");
    assert_eq!(download["access"], "read");
    assert!(download.get("executor").is_none());
    assert_eq!(download["requires"]["content_delivery"], "reference");
    assert_eq!(download["output"]["views"], json!(["download"]));
    assert_eq!(
        download["ui"]["locations"],
        json!(["resource_detail", "context_menu"])
    );

    let thumbnail = actions
        .iter()
        .find(|action| action["id"] == "core.resource.thumbnail")
        .unwrap();
    assert_eq!(thumbnail["output"]["views"], json!(["media"]));
    assert_eq!(
        thumbnail["ui"]["locations"],
        json!(["resource_list_thumbnail"])
    );
    let read = actions
        .iter()
        .find(|action| action["id"] == "core.text.read")
        .unwrap();
    assert_eq!(read["provides"], "text_read");
    assert_eq!(read["access"], "read");
    assert_eq!(read["requires"]["content_delivery"], "inline");
    assert_eq!(read["output"]["views"], json!(["text"]));
    let edit = actions
        .iter()
        .find(|action| action["id"] == "core.text.edit")
        .unwrap();
    assert_eq!(edit["provides"], "text_edit");
    assert_eq!(edit["access"], "write");
    assert_eq!(edit["requires"]["content_delivery"], "inline");
    assert_eq!(edit["output"]["views"], json!(["text"]));
    assert_eq!(actions.len(), 4);

    let (status, resource) = stream_upload(
        &app,
        "/resources?name=book.txt&directory=books",
        "text/plain",
        b"Hello book",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["kind"], "core:text");
    assert!(has_action(&resource, "core.resource.download"));
    assert!(has_action(&resource, "core.resource.thumbnail"));
    assert!(has_action(&resource, "core.text.read"));
    assert!(has_action(&resource, "core.text.edit"));
    assert_eq!(resource["actions"].as_array().unwrap().len(), 4);

    let resource_id = resource["id"].as_str().unwrap();
    let (status, output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.resource.download"),
        json!({ "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["view"]["view"], "download");
    assert_eq!(
        output["view"]["url"],
        format!("/resources/{resource_id}/download")
    );
    assert_eq!(output["view"]["filename"], "book.txt");

    let (status, thumbnail_output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.resource.thumbnail"),
        json!({ "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thumbnail_output}");
    assert_eq!(thumbnail_output["view"]["view"], "media");
    assert_eq!(thumbnail_output["view"]["mime_type"], "image/svg+xml");
    assert_eq!(thumbnail_output["view"]["encoding"], "base64");

    let (status, text_output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.text.read"),
        json!({ "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text_output}");
    assert_eq!(
        text_output["view"],
        json!({ "view": "text", "text": "Hello book" })
    );
    let (status, conflict) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.text.read"),
        json!({ "expected_revision": resource["revision"].as_u64().unwrap() + 1, "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "concurrency.revision_conflict");

    let (status, error) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.text.edit"),
        json!({ "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("expected_revision")
    );

    let (status, edit_output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.text.edit"),
        json!({ "expected_revision": resource["revision"], "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edit_output}");
    assert_eq!(
        edit_output["view"],
        json!({ "view": "text", "text": "Hello book" })
    );

    let updated = b"Updated book";
    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri(format!("/resources/{resource_id}/content"))
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::CONTENT_LENGTH, updated.len())
            .header("Content-SHA256", sha256_hex(updated))
            .header("If-Match", format!("\"{}\"", resource["revision"]))
            .body(Body::from(updated.as_slice()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated_resource: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), BODY_LIMIT).await.unwrap()).unwrap();
    assert_eq!(
        updated_resource["revision"],
        resource["revision"].as_u64().unwrap() + 1
    );

    let stale = b"Stale overwrite";
    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri(format!("/resources/{resource_id}/content"))
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::CONTENT_LENGTH, stale.len())
            .header("Content-SHA256", sha256_hex(stale))
            .header("If-Match", format!("\"{}\"", resource["revision"]))
            .body(Body::from(stale.as_slice()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let download = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(output["view"]["url"].as_str().unwrap())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"book.txt\"; filename*=UTF-8''book.txt"
    );
    assert_eq!(
        download.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/plain"
    );
    let body = to_bytes(download.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(body.as_ref(), b"Updated book");

    let inline = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{resource_id}/content"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(inline.status(), StatusCode::OK);
    assert_eq!(
        inline.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "inline"
    );
    let body = to_bytes(inline.into_body(), BODY_LIMIT).await.unwrap();
    assert_eq!(body.as_ref(), b"Updated book");
}

#[tokio::test]
async fn text_content_replacement_streams_beyond_the_action_json_limit_and_enforces_integrity() {
    let app = test_app("streaming-text-replacement").await;
    let (status, resource) =
        stream_upload(&app, "/resources?name=large.txt", "text/plain", b"small").await;
    assert_eq!(status, StatusCode::CREATED, "{resource}");
    let id = resource["id"].as_str().unwrap();
    let content = vec![b'x'; MAX_ACTION_REQUEST_BYTES + 1024];
    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri(format!("/resources/{id}/content"))
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::CONTENT_LENGTH, content.len())
            .header("Content-SHA256", sha256_hex(&content))
            .header("If-Match", format!("\"{}\"", resource["revision"]))
            .body(Body::from(content))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), BODY_LIMIT).await.unwrap()).unwrap();

    let bad = b"checksum mismatch";
    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri(format!("/resources/{id}/content"))
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::CONTENT_LENGTH, bad.len())
            .header("Content-SHA256", "0".repeat(64))
            .header("If-Match", format!("\"{}\"", updated["revision"]))
            .body(Body::from(bad.as_slice()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri(format!("/resources/{id}/content"))
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::CONTENT_LENGTH, 4 * 1024 * 1024 + 1)
            .header("Content-SHA256", "0".repeat(64))
            .header("If-Match", format!("\"{}\"", updated["revision"]))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn core_image_thumbnail_reuses_the_authorized_content_url() {
    let app = test_app("core-image-thumbnail").await;
    let (status, resource) =
        stream_upload(&app, "/resources?name=pixel.png", "image/png", b"png-bytes").await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "core:image");
    assert!(has_action(&resource, "core.image.thumbnail"));
    assert!(!has_action(&resource, "core.resource.thumbnail"));
    let resource_id = resource["id"].as_str().unwrap();
    let (status, output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.image.thumbnail"),
        json!({ "expected_revision": resource["revision"], "input": {} }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["view"]["view"], "media");
    assert_eq!(output["view"]["mime_type"], "image/png");
    assert_eq!(output["view"]["encoding"], "url");
    assert_eq!(
        output["view"]["data"],
        format!("/resources/{resource_id}/content")
    );
}

#[tokio::test]
async fn generic_resource_thumbnail_does_not_special_case_image_content() {
    let app = test_app("generic-resource-image-thumbnail").await;
    let (status, resource) = stream_upload(
        &app,
        "/resources?name=pixel.png&kind=core%3Aresource",
        "image/png",
        b"png-bytes",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "core:resource");
    assert!(has_action(&resource, "core.resource.thumbnail"));
    assert!(!has_action(&resource, "core.image.thumbnail"));
    let resource_id = resource["id"].as_str().unwrap();
    let (status, output) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/core.resource.thumbnail"),
        json!({ "expected_revision": resource["revision"], "input": {} }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["view"]["view"], "media");
    assert_eq!(output["view"]["mime_type"], "image/svg+xml");
    assert_eq!(output["view"]["encoding"], "base64");
}

#[tokio::test]
async fn action_endpoint_has_a_dedicated_request_body_limit() {
    let app = test_app("action-body-limit").await;
    let oversized = format!(
        "{{\"input\":{{\"value\":\"{}\"}}}}",
        "x".repeat(MAX_ACTION_REQUEST_BYTES)
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

    let oversized = format!(
        "{{\"input\":{{\"value\":\"{}\"}}}}",
        "x".repeat(MAX_ACTION_REQUEST_BYTES)
    );
    let response = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/directories/00000000-0000-0000-0000-000000000000/actions/example")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(oversized))
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn legacy_resource_upload_endpoint_is_removed() {
    let app = test_app("legacy-resource-upload-removed").await;
    let response = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/resources")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"name":"metadata-only-resource"}"#))
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn stream_upload_roundtrips_small_blob_and_creates_directories() {
    let app = test_app("small-upload").await;
    let data = b"hello, asset-hub!";

    let (status, resource) = stream_upload(
        &app,
        "/resources?name=hello.txt&kind=core%3Aresource&directory=examples",
        "text/plain",
        data,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["size"], data.len() as u64);
    assert_eq!(resource["content"]["mime_type"], "text/plain");
    assert_eq!(resource["content"]["verification_status"], "verified");
    assert!(resource["content"]["verification_error"].is_null());
    assert_eq!(resource["content"]["checksum"]["kind"], "sha256");
    assert_eq!(
        resource["content"]["checksum"]["value"],
        "ee6d5b2c127b5113e886343345d8f11810024201f0c46f54b76d8cc2908c538c"
    );

    let id = resource["id"].as_str().unwrap();
    let (status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content.as_ref(), data);

    let (status, directory_resource) = stream_upload(
        &app,
        "/resources?name=nested.txt&kind=core%3Aresource&directory=examples%2Fnested",
        "text/plain",
        b"nested",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(directory_resource["directory"], "examples/nested");

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
async fn upload_session_resumes_after_runtime_restart_and_publishes_only_on_complete() {
    let root = unique_temp_root("resumable-restart");
    let app = test_app_at_root(
        root.clone(),
        PluginHostConfig::default(),
        RouterOptions::default(),
    )
    .await;
    let create = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/uploads")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "name": "resume.txt",
                    "directory": "resumable",
                    "mime_type": "text/plain",
                    "size": 11,
                    "expected_sha256": sha256_hex(b"hello world")
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let id = response_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let first_chunk = request(
        &app,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "0")
            .header("upload-checksum", sha256_hex(b"hello "))
            .body(Body::from("hello "))
            .unwrap(),
    )
    .await;
    assert_eq!(first_chunk.status(), StatusCode::NO_CONTENT);
    assert_eq!(first_chunk.headers()["upload-offset"], "6");
    assert!(!root.join("blob/resumable/resume.txt").exists());

    let restarted = test_app_at_root(
        root.clone(),
        PluginHostConfig::default(),
        RouterOptions::default(),
    )
    .await;
    let status = request(
        &restarted,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/uploads/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status = response_json(status).await;
    assert_eq!(status["offset"], 6);
    assert_eq!(status["size"], 11);
    assert_eq!(status["status"], "uploading");

    let conflict = request(
        &restarted,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "0")
            .header("upload-checksum", sha256_hex(b"world"))
            .body(Body::from("world"))
            .unwrap(),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let second_chunk = request(
        &restarted,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "6")
            .header("upload-checksum", sha256_hex(b"world"))
            .body(Body::from("world"))
            .unwrap(),
    )
    .await;
    assert_eq!(second_chunk.status(), StatusCode::NO_CONTENT);
    assert_eq!(second_chunk.headers()["upload-offset"], "11");
    assert!(!root.join("blob/resumable/resume.txt").exists());

    let complete = request(
        &restarted,
        Request::builder()
            .method(Method::POST)
            .uri(format!("/uploads/{id}/complete"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(complete.status(), StatusCode::ACCEPTED);
    assert_eq!(response_json(complete).await["status"], "finalizing");
    let (completion_status, resource) = wait_for_upload_resource(&restarted, &id, None).await;
    assert_eq!(completion_status, StatusCode::CREATED, "{resource}");
    let resource_id = resource["id"].as_str().unwrap().to_string();
    assert_eq!(resource["content"]["size"], 11);
    assert_eq!(
        std::fs::read(root.join("blob/resumable/resume.txt")).unwrap(),
        b"hello world"
    );

    let repeated_complete = request(
        &restarted,
        Request::builder()
            .method(Method::POST)
            .uri(format!("/uploads/{id}/complete"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(repeated_complete.status(), StatusCode::ACCEPTED);
    assert_eq!(
        response_json(repeated_complete).await["resource_id"],
        resource_id
    );

    let acknowledged = request(
        &restarted,
        Request::builder()
            .method(Method::DELETE)
            .uri(format!("/uploads/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(acknowledged.status(), StatusCode::NO_CONTENT);
    let removed = request(
        &restarted,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/uploads/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(removed.status(), StatusCode::NOT_FOUND);
    assert!(!root.join("blob/.asset-hub/uploads").exists());
}

#[tokio::test]
async fn upload_chunk_checksum_mismatch_keeps_server_offset_unchanged() {
    let app = test_app("upload-chunk-checksum-mismatch").await;
    let data = b"verified chunk";
    let create = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/uploads")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "name": "chunk.bin",
                    "directory": "uploads",
                    "size": data.len(),
                    "expected_sha256": sha256_hex(data)
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let id = response_json(create).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let missing_checksum = request(
        &app,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "0")
            .body(Body::from(data.to_vec()))
            .unwrap(),
    )
    .await;
    assert_eq!(missing_checksum.status(), StatusCode::BAD_REQUEST);

    let mismatch = request(
        &app,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "0")
            .header("upload-checksum", sha256_hex(b"different chunk"))
            .body(Body::from(data.to_vec()))
            .unwrap(),
    )
    .await;
    assert_eq!(mismatch.status(), StatusCode::CONFLICT);
    assert!(
        response_json(mismatch).await["error"]
            .as_str()
            .unwrap()
            .contains("chunk checksum mismatch")
    );

    let status = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/uploads/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response_json(status).await["offset"], 0);
    assert_eq!(
        std::fs::metadata(app.root.join(format!("blob/.asset-hub/uploads/{id}")))
            .unwrap()
            .len(),
        0
    );
    assert!(
        !app.root
            .join(format!("blob/.asset-hub/uploads/{id}.chunk"))
            .exists()
    );

    let accepted = request(
        &app,
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "0")
            .header("upload-checksum", sha256_hex(data))
            .body(Body::from(data.to_vec()))
            .unwrap(),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
    assert_eq!(accepted.headers()["upload-offset"], data.len().to_string());
    assert!(
        !app.root
            .join(format!("blob/.asset-hub/uploads/{id}.chunk"))
            .exists()
    );
}

#[tokio::test]
async fn stream_upload_preserves_spaces_in_names_and_physical_paths() {
    let app = test_app("spaced-upload").await;
    let data = b"spaces are part of the path";

    let (status, resource) = stream_upload(
        &app,
        "/resources?name=%20draft%20%2001.txt%20&kind=core%3Aresource&directory=%20library%20%2Fproject%20A%20",
        "text/plain",
        data,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["name"], " draft  01.txt ");
    assert_eq!(resource["directory"], " library /project A ");

    let exact_path = app
        .root
        .join("blob")
        .join(" library /project A / draft  01.txt ");
    assert_eq!(tokio::fs::read(&exact_path).await.unwrap(), data);
    assert!(
        !app.root
            .join("blob/library/project A/draft  01.txt")
            .exists()
    );

    let id = resource["id"].as_str().unwrap();
    let (status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content.as_ref(), data);
}

#[tokio::test]
async fn empty_directories_create_physical_directories() {
    let app = test_app("physical-directories").await;

    let (status, projects) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": ROOT_DIRECTORY_ID, "name": "projects" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, directory) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": projects["id"], "name": "empty" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(directory["path"], "projects/empty");
    assert!(app.root.join("blob/projects/empty").is_dir());

    let (status, _) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": ROOT_DIRECTORY_ID, "name": ".asset-hub" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn directory_crud_uses_stable_ids_and_revision_preconditions() {
    let app = test_app("directory-crud").await;
    let (status, created) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": ROOT_DIRECTORY_ID, "name": "drafts" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap();

    let (status, found) =
        empty_json_request(&app, Method::GET, &format!("/directories/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["id"], created["id"]);

    let (status, renamed) = json_request(
        &app,
        Method::PATCH,
        &format!("/directories/{id}"),
        json!({
            "expected_revision": found["revision"],
            "name": "published"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["id"], created["id"]);
    assert_eq!(renamed["path"], "published");
    assert!(renamed["revision"].as_u64() > found["revision"].as_u64());

    let (status, _) = json_request(
        &app,
        Method::PATCH,
        &format!("/directories/{id}"),
        json!({ "expected_revision": found["revision"], "name": "stale" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, child) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": id, "name": "child" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{child}");
    let child_id = child["id"].as_str().unwrap();

    let (status, _) = empty_json_request(
        &app,
        Method::DELETE,
        &format!(
            "/directories/{id}?expected_revision={}",
            renamed["revision"]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = empty_json_request(
        &app,
        Method::DELETE,
        &format!(
            "/directories/{child_id}?expected_revision={}",
            child["revision"]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = empty_json_request(
        &app,
        Method::DELETE,
        &format!(
            "/directories/{id}?expected_revision={}",
            renamed["revision"]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn resource_content_supports_single_byte_ranges_for_video_seek() {
    let app = test_app("content-range").await;
    let data = b"0123456789";
    let (status, resource) =
        stream_upload(&app, "/resources?name=clip.mp4", "video/mp4", data).await;
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

    let download_range = request(
        &app,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/resources/{id}/download"))
            .header(header::RANGE, "bytes=2-5")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(download_range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        download_range
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap(),
        "attachment; filename=\"clip.mp4\"; filename*=UTF-8''clip.mp4"
    );
    assert_eq!(
        download_range.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 2-5/10"
    );
    let body = to_bytes(download_range.into_body(), BODY_LIMIT)
        .await
        .unwrap();
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
async fn upload_rejects_legacy_checksum_fields_and_existing_resource_path() {
    let app = test_app("upload-security").await;

    let response = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/uploads")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "name": "bad.txt",
                    "directory": "secure",
                    "size": 17,
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let body = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("sha256"));

    let response = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/uploads")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "name": "unsupported.txt",
                    "directory": "secure",
                    "size": 17,
                    "checksum_kind": "sha256"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    let status = response.status();
    let body = to_bytes(response.into_body(), BODY_LIMIT).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("checksum_kind"));

    let id = create_text_resource(&app, "secure/existing.txt").await;
    assert!(!id.is_empty());

    let (status, error) = stream_upload(
        &app,
        "/resources?name=existing.txt&directory=secure",
        "text/plain",
        b"hello, asset-hub!",
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["error"].as_str().unwrap().contains("already exists"));
}

#[tokio::test]
async fn upload_checksum_mismatch_never_publishes_a_resource() {
    let app = test_app("upload-checksum-mismatch").await;
    let (status, error) = resumable_upload_with_expected_checksum(
        &app,
        "/resources?name=mismatch.bin&directory=secure",
        "application/octet-stream",
        b"wrong",
        None,
        &sha256_hex(b"right"),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("checksum mismatch")
    );
    assert!(!app.root.join("blob/secure/mismatch.bin").exists());
    let (status, listing) = empty_json_request(&app, Method::GET, "/directories?path=secure").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["resources"]["total"], 0);
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
    let create = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/uploads")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "name": "slow.txt",
                    "directory": "uploads",
                    "mime_type": "text/plain",
                    "size": 11,
                    "expected_sha256": sha256_hex(b"slow upload")
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let session = response_json(create).await;
    let id = session["id"].as_str().unwrap();
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
            .method(Method::PATCH)
            .uri(format!("/uploads/{id}"))
            .header("upload-offset", "0")
            .header("upload-checksum", sha256_hex(b"slow upload"))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::ORIGIN, "http://127.0.0.1:5173")
            .body(body)
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&header::HeaderValue::from_static("http://127.0.0.1:5173"))
    );
    let exposed = response
        .headers()
        .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(exposed.contains("upload-offset"));
    assert!(exposed.contains("upload-length"));
    let complete = request(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri(format!("/uploads/{id}/complete"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(complete.status(), StatusCode::ACCEPTED);
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
    let (status, resource) =
        stream_upload(&app, "/resources?name=README.md", "text/plain", "# README").await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "azvs:markdown");
    let actions = resource["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|action| action["id"] == "azvs.markdown.read")
    );
    assert!(
        actions
            .iter()
            .any(|action| action["id"] == "azvs.markdown.edit")
    );
    let markdown_read = actions
        .iter()
        .find(|action| action["id"] == "azvs.markdown.read")
        .unwrap();
    assert_eq!(markdown_read["provides"], "text_read");
    let markdown_edit = actions
        .iter()
        .find(|action| action["id"] == "azvs.markdown.edit")
        .unwrap();
    assert_eq!(markdown_edit["provides"], "text_edit");
    assert!(
        actions
            .iter()
            .all(|action| action["id"] != "core.text.read" && action["id"] != "core.text.edit")
    );
    assert!(
        actions
            .iter()
            .any(|action| action["id"] == "core.resource.download")
    );

    let resource_id = resource["id"].as_str().unwrap();
    let (status, rendered) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{resource_id}/actions/azvs.markdown.read"),
        json!({ "expected_revision": resource["revision"], "input": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rendered}");
    assert_eq!(rendered["action"], "azvs.markdown.read");
    assert_eq!(rendered["view"]["view"], "plugin_frame");
    assert!(
        rendered["view"]["url"]
            .as_str()
            .unwrap()
            .starts_with("/plugins/azvs.markdown/index.html#payload=")
    );
}

#[tokio::test]
async fn epub_thumbnail_provider_is_selected_for_the_resource_capability() {
    let app = test_app_with_plugin_manifests(
        "epub-thumbnail-override",
        vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/azvs-epub/manifest.json")],
    )
    .await;
    let (status, resource) = stream_upload(
        &app,
        "/resources?name=book.epub",
        "application/epub+zip",
        b"not-needed-for-action-discovery",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "azvs:epub");
    let actions = resource["actions"].as_array().unwrap();
    let thumbnail = actions
        .iter()
        .find(|action| action["id"] == "azvs.epub.thumbnail")
        .unwrap();
    assert_eq!(thumbnail["provides"], "thumbnail");
    assert!(
        actions
            .iter()
            .all(|action| action["id"] != "core.resource.thumbnail")
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
        "/resources?name=README.md",
        "text/markdown",
        b"# README",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resource}");

    let id = resource["id"].as_str().unwrap();
    let (status, error) = json_request(
        &app,
        Method::POST,
        &format!("/resources/{id}/actions/azvs.markdown.read"),
        json!({"expected_revision": resource["revision"], "input": {}}),
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
    let original_blob_path = app.root.join("blob/delete/me.txt");
    let trash_blob_path = app.root.join(format!("blob/.asset-hub/trash/{id}"));
    let (status, current) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    let revision = current["revision"].as_u64().unwrap();
    let (status, conflict) = empty_json_request(
        &app,
        Method::DELETE,
        &format!(
            "/resources/{id}?expected_revision={}",
            revision.saturating_sub(1)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "concurrency.revision_conflict");
    assert!(original_blob_path.exists());

    let (status, deleted) = empty_json_request(
        &app,
        Method::DELETE,
        &format!("/resources/{id}?expected_revision={revision}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(deleted["deleted_at"].is_string());
    assert!(!original_blob_path.exists());
    assert_eq!(std::fs::read(&trash_blob_path).unwrap(), b"delete me");

    let (status, _) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let replacement_id = create_text_resource(&app, "delete/me.txt").await;
    assert_ne!(replacement_id, id);
    assert_eq!(std::fs::read(&original_blob_path).unwrap(), b"delete me");

    let (status, _) = json_request(
        &app,
        Method::PATCH,
        &format!("/resources/{id}"),
        json!({ "expected_revision": deleted["revision"], "restore": true }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(std::fs::read(&trash_blob_path).unwrap(), b"delete me");
    assert_eq!(std::fs::read(&original_blob_path).unwrap(), b"delete me");

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
    assert!(!trash_blob_path.exists());
    assert!(original_blob_path.exists());

    let (status, _) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) =
        empty_json_request(&app, Method::GET, &format!("/resources/{replacement_id}")).await;
    assert_eq!(status, StatusCode::OK);
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
            .contains("--enable-purge=false")
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

    let preflight = request(
        &app,
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/uploads/example")
            .header(header::ORIGIN, "http://127.0.0.1:5173")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "content-type,upload-offset,upload-checksum",
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(preflight.status(), StatusCode::OK);
    let allowed_headers = preflight
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(allowed_headers.contains("upload-offset"));
    assert!(allowed_headers.contains("upload-checksum"));
}

#[tokio::test]
async fn list_resources_filters_by_kind_and_query() {
    let app = test_app("list-resources").await;

    let (first_status, first) = stream_upload(
        &app,
        "/resources?name=alpha%20document&directory=&kind=core%3Aresource",
        "application/octet-stream",
        b"alpha document",
    )
    .await;
    let (second_status, second) = stream_upload(
        &app,
        "/resources?name=beta%20image&directory=&kind=core%3Aresource",
        "application/octet-stream",
        b"beta image",
    )
    .await;
    let (third_status, third) = stream_upload(
        &app,
        "/resources?name=alpha%20image&directory=&kind=core%3Aresource",
        "application/octet-stream",
        b"alpha image",
    )
    .await;

    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(second_status, StatusCode::CREATED);
    assert_eq!(third_status, StatusCode::CREATED);
    assert!(first["id"].is_string());
    assert!(second["id"].is_string());
    assert!(third["id"].is_string());
    assert_eq!(first["kind"], "core:resource");
    assert_eq!(second["kind"], "core:resource");
    assert_eq!(third["kind"], "core:resource");

    let (status, page) = empty_json_request(
        &app,
        Method::GET,
        "/resources?kind=core%3Aresource&q=alpha%20image&page=1&limit=10",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1);
    assert_eq!(page["page"], 1);
    assert_eq!(page["limit"], 10);
    assert_eq!(page["items"][0]["name"], "alpha image");
    assert!(page["items"][0].get("status").is_none());
}

#[tokio::test]
async fn kind_filter_includes_all_descendants() {
    let app = test_app("kind-descendant-filter").await;

    for (name, kind) in [
        ("generic resource", "core:resource"),
        ("note.txt", "core:text"),
        ("image.png", "core:image"),
    ] {
        let uri = format!(
            "/resources?name={}&directory=&kind={}",
            name.replace(' ', "%20"),
            kind.replace(':', "%3A")
        );
        let (status, resource) =
            stream_upload(&app, &uri, "application/octet-stream", name.as_bytes()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(resource["kind"], kind);
    }

    let (status, hierarchy) =
        empty_json_request(&app, Method::GET, "/resources?kind=core%3Aresource").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hierarchy["total"], 3);

    let (status, kinds) = empty_json_request(&app, Method::GET, "/resource-kinds").await;
    assert_eq!(status, StatusCode::OK);
    let text = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:text")
        .unwrap();
    assert_eq!(text["parent"], "core:resource");
    assert_eq!(text["ancestors"], json!(["core:resource"]));
    assert!(
        text["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "core.resource.download")
    );
}

#[tokio::test]
async fn update_resource_changes_fields_and_restores_soft_deleted_resource() {
    let app = test_app("update-resource").await;
    let id = create_text_resource(&app, "update/me.txt").await;
    let old_blob_path = app.root.join("blob/update/me.txt");
    let new_blob_path = app.root.join("blob/archive/updated.txt");
    assert_eq!(std::fs::read(&old_blob_path).unwrap(), b"delete me");
    let (status, current) =
        empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, updated) = json_request(
        &app,
        Method::PATCH,
        &format!("/resources/{id}"),
        json!({
            "expected_revision": current["revision"],
            "name": "updated.txt",
            "directory": "archive",
            "kind": "core:resource"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "updated.txt");
    assert_eq!(updated["directory"], "archive");
    assert_eq!(updated["kind"], "core:resource");
    assert!(updated.get("status").is_none());
    assert!(!old_blob_path.exists());
    assert_eq!(std::fs::read(&new_blob_path).unwrap(), b"delete me");

    let (status, deleted) = empty_json_request(
        &app,
        Method::DELETE,
        &format!(
            "/resources/{id}?expected_revision={}",
            updated["revision"].as_u64().unwrap()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let trash_blob_path = app.root.join(format!("blob/.asset-hub/trash/{id}"));
    assert!(!new_blob_path.exists());
    assert_eq!(std::fs::read(&trash_blob_path).unwrap(), b"delete me");

    let (status, _) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, restored) = json_request(
        &app,
        Method::PATCH,
        &format!("/resources/{id}"),
        json!({"expected_revision": deleted["revision"], "restore": true}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(restored.get("status").is_none());
    assert!(restored["deleted_at"].is_null());
    assert_eq!(std::fs::read(&new_blob_path).unwrap(), b"delete me");
    assert!(!trash_blob_path.exists());
}

#[tokio::test]
async fn openapi_exposes_current_http_contract() {
    let app = test_app("openapi").await;
    let (status, document) = empty_json_request(&app, Method::GET, "/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);

    let schemas = &document["components"]["schemas"];
    let update_properties = &schemas["UpdateResourceRequest"]["properties"];
    let response_properties = &schemas["ResourceResponse"]["properties"];
    assert!(update_properties.get("description").is_none());
    assert!(response_properties.get("description").is_none());
    assert!(update_properties.get("status").is_none());
    assert!(response_properties.get("status").is_none());
    assert!(schemas.get("CreateResourceRequest").is_none());
    assert!(document["paths"]["/resources"].get("post").is_none());
    assert!(document["paths"]["/uploads"]["post"].is_object());
    assert!(document["paths"]["/uploads/{id}"]["get"].is_object());
    assert!(document["paths"]["/uploads/{id}"].get("head").is_none());
    assert!(document["paths"]["/uploads/{id}"]["patch"].is_object());
    assert!(document["paths"]["/uploads/{id}"]["delete"].is_object());
    assert!(document["paths"]["/uploads/{id}/complete"]["post"].is_object());
    assert!(document["paths"].get("/resources/content/stream").is_none());
    let list_parameters = document["paths"]["/resources"]["get"]["parameters"]
        .as_array()
        .unwrap();
    assert!(
        !list_parameters
            .iter()
            .any(|parameter| parameter["name"] == "include_descendants")
    );
    assert!(document["paths"].get("/resources/{id}/download").is_some());
    assert!(document["paths"].get("/directory-kinds").is_some());
    assert!(
        document["paths"]
            .get("/directories/{id}/download")
            .is_some()
    );
    assert!(
        document["paths"]
            .get("/directories/{id}/actions/{action}")
            .is_some()
    );
    assert!(document["paths"].get("/resources/{id}/read").is_none());
    assert!(document["paths"].get("/resources/{id}/preview").is_none());
    assert!(document["paths"].get("/resources/{id}/thumbnail").is_none());
    assert!(document["paths"].get("/auth/login").is_some());
    assert!(document["paths"].get("/auth/users/{id}").is_some());
    assert!(document["paths"].get("/scan").is_none());
    assert!(document["paths"].get("/auth/directory-grants").is_none());
    assert!(
        document["components"]["schemas"]["AuthenticatedUser"]["properties"]
            .get("workspace_directory")
            .is_none()
    );
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
    test_app_with_config(name, RouterOptions::default()).await
}

async fn test_app_with_router_options(name: &str, options: RouterOptions) -> TestApp {
    test_app_with_config(name, options).await
}

async fn test_app_with_plugin_manifests(name: &str, plugin_manifests: Vec<PathBuf>) -> TestApp {
    test_app_with_plugin_host_config(name, plugin_manifests, PluginHostConfig::default()).await
}

async fn test_app_with_plugin_host_config(
    name: &str,
    plugin_manifests: Vec<PathBuf>,
    plugin: PluginHostConfig,
) -> TestApp {
    let root = unique_temp_root(name);
    for manifest_path in plugin_manifests {
        install_test_plugin(&root.join("blob"), &manifest_path);
    }
    test_app_at_root(root, plugin, RouterOptions::default()).await
}

async fn test_app_with_config(name: &str, options: RouterOptions) -> TestApp {
    test_app_at_root(unique_temp_root(name), PluginHostConfig::default(), options).await
}

async fn test_app_at_root(
    root: PathBuf,
    plugin: PluginHostConfig,
    options: RouterOptions,
) -> TestApp {
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.join("blob"),
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        plugin,
        resource_edit: Default::default(),
    };
    let runtime = AssetRuntime::new(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = build_router(
        runtime.resource_service(),
        options,
        HashMap::new(),
        authorization,
        runtime.upload_finalization_dispatcher(),
    )
    .layer(Extension(test_admin_context()));

    TestApp { router, root }
}

async fn test_app_with_plugin_web_assets(
    root: PathBuf,
    plugin_web_assets: PluginWebAssets,
) -> TestApp {
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.join("blob"),
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        plugin: Default::default(),
        resource_edit: Default::default(),
    };
    let runtime = AssetRuntime::new(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = build_router(
        runtime.resource_service(),
        RouterOptions::default(),
        plugin_web_assets,
        authorization,
        runtime.upload_finalization_dispatcher(),
    )
    .layer(Extension(test_admin_context()));

    TestApp { router, root }
}

async fn create_text_resource(app: &TestApp, path: &str) -> String {
    let (directory, name) = path
        .rsplit_once('/')
        .map_or(("", path), |(directory, name)| (directory, name));
    let uri = format!(
        "/resources?name={name}&directory={}",
        directory.replace('/', "%2F")
    );
    let (status, resource) = stream_upload(app, &uri, "text/plain", b"delete me").await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["kind"], "core:text");

    resource["id"].as_str().unwrap().to_string()
}

async fn stream_upload(
    app: &TestApp,
    uri: &str,
    content_type: &str,
    data: impl AsRef<[u8]>,
) -> (StatusCode, Value) {
    resumable_upload(app, uri, content_type, data.as_ref(), None).await
}

async fn resumable_upload(
    app: &TestApp,
    uri: &str,
    content_type: &str,
    data: &[u8],
    cookie: Option<&str>,
) -> (StatusCode, Value) {
    resumable_upload_with_expected_checksum(app, uri, content_type, data, cookie, &sha256_hex(data))
        .await
}

async fn resumable_upload_with_expected_checksum(
    app: &TestApp,
    uri: &str,
    content_type: &str,
    data: &[u8],
    cookie: Option<&str>,
    expected_sha256: &str,
) -> (StatusCode, Value) {
    let query = uri.split_once('?').map_or("", |(_, query)| query);
    let parameters = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key, percent_decode(value)))
        .collect::<HashMap<_, _>>();
    let create_body = json!({
        "name": parameters.get("name").cloned().unwrap_or_default(),
        "directory": parameters.get("directory").cloned().unwrap_or_default(),
        "kind": parameters.get("kind").cloned(),
        "mime_type": content_type,
        "size": data.len(),
        "expected_sha256": expected_sha256,
    });
    let mut create_request = Request::builder()
        .method(Method::POST)
        .uri("/uploads")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        create_request = create_request.header(header::COOKIE, cookie);
    }
    let create = request(
        app,
        create_request
            .body(Body::from(create_body.to_string()))
            .unwrap(),
    )
    .await;
    if create.status() != StatusCode::CREATED {
        let status = create.status();
        return (status, response_json(create).await);
    }
    let session = response_json(create).await;
    let id = session["id"].as_str().unwrap();
    let mut append_request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/uploads/{id}"))
        .header("upload-offset", "0")
        .header("upload-checksum", sha256_hex(data))
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if let Some(cookie) = cookie {
        append_request = append_request.header(header::COOKIE, cookie);
    }
    let append = request(app, append_request.body(Body::from(data.to_vec())).unwrap()).await;
    if append.status() != StatusCode::NO_CONTENT {
        let status = append.status();
        return (status, response_json(append).await);
    }
    let mut complete_request = Request::builder()
        .method(Method::POST)
        .uri(format!("/uploads/{id}/complete"));
    if let Some(cookie) = cookie {
        complete_request = complete_request.header(header::COOKIE, cookie);
    }
    let complete = request(app, complete_request.body(Body::empty()).unwrap()).await;
    let status = complete.status();
    let body = response_json(complete).await;
    if status != StatusCode::ACCEPTED {
        return (status, body);
    }
    wait_for_upload_resource(app, id, cookie).await
}

async fn wait_for_upload_resource(
    app: &TestApp,
    id: &str,
    cookie: Option<&str>,
) -> (StatusCode, Value) {
    for _ in 0..500 {
        let mut status_request = Request::builder()
            .method(Method::GET)
            .uri(format!("/uploads/{id}"));
        if let Some(cookie) = cookie {
            status_request = status_request.header(header::COOKIE, cookie);
        }
        let response = request(app, status_request.body(Body::empty()).unwrap()).await;
        if response.status() != StatusCode::OK {
            let status = response.status();
            return (status, response_json(response).await);
        }
        let session = response_json(response).await;
        match session["status"].as_str() {
            Some("completed") => {
                let resource_id = session["resource_id"]
                    .as_str()
                    .expect("completed upload should expose its Resource ID");
                let mut resource_request = Request::builder()
                    .method(Method::GET)
                    .uri(format!("/resources/{resource_id}"));
                if let Some(cookie) = cookie {
                    resource_request = resource_request.header(header::COOKIE, cookie);
                }
                let response = request(app, resource_request.body(Body::empty()).unwrap()).await;
                let status = if response.status() == StatusCode::OK {
                    StatusCode::CREATED
                } else {
                    response.status()
                };
                return (status, response_json(response).await);
            }
            Some("failed") => {
                return (
                    StatusCode::CONFLICT,
                    json!({ "error": session["error"].clone() }),
                );
            }
            Some("uploading" | "finalizing") => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            other => panic!("unexpected upload status: {other:?}"),
        }
    }
    panic!("upload `{id}` did not finish in time");
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        decoded.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8(decoded).unwrap()
}

fn test_admin_context() -> AccessContext {
    AccessContext::administrator(
        "01900000-0000-7000-8000-000000000001"
            .parse::<UserId>()
            .unwrap(),
    )
}

#[tokio::test]
async fn authentication_starts_without_users_and_limits_member_workspace_access() {
    let root = unique_temp_root("authenticated-workspace");
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.join("blob"),
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        plugin: Default::default(),
        resource_edit: Default::default(),
    };
    let business_database = root.join("blob/.asset-hub/asset-hub.sqlite");
    let session_database = root.join("session/http-session.sqlite");
    let runtime = AssetRuntime::new(config).await.unwrap();
    let session_runtime = HttpSessionRuntime::open(&session_database).await.unwrap();

    let business_pool =
        SqlitePool::connect_with(SqliteConnectOptions::new().filename(&business_database))
            .await
            .unwrap();
    let session_pool =
        SqlitePool::connect_with(SqliteConnectOptions::new().filename(&session_database))
            .await
            .unwrap();
    let business_tables = query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(&business_pool)
    .await
    .unwrap();
    let session_tables = query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(&session_pool)
    .await
    .unwrap();
    assert!(business_tables.iter().any(|table| table == "resources"));
    assert!(!business_tables.iter().any(|table| table == "http_sessions"));
    assert!(session_tables.iter().any(|table| table == "http_sessions"));
    assert!(!session_tables.iter().any(|table| table == "resources"));
    let authorization = runtime.authorization_service();
    let base = build_router(
        runtime.resource_service(),
        RouterOptions::default(),
        HashMap::from([(
            "azvs.markdown".to_string(),
            HashMap::from([(
                PathBuf::from("viewer.js"),
                std::sync::Arc::from(b"document.body.textContent = 'loaded'".as_slice()),
            )]),
        )]),
        authorization.clone(),
        runtime.upload_finalization_dispatcher(),
    );
    let users = runtime.user_service();
    let router = with_authentication(
        base,
        users.clone(),
        session_runtime.store(),
        session_runtime.health(),
        &SessionOptions {
            cookie_secure: false,
            inactivity_timeout: Duration::from_secs(3600),
        },
    )
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
    assert_eq!(health["session_store"]["status"], "ready");
    assert_eq!(health["blob_storage"]["status"], "ready");

    let login_without_users = request_with_cookie(
        &app,
        Method::POST,
        "/auth/login",
        json!({ "username": "admin", "password": "administrator-password" }),
        "",
    )
    .await;
    assert_eq!(login_without_users.status(), StatusCode::UNAUTHORIZED);

    users
        .create(
            "admin",
            "administrator-password",
            UserRole::Administrator,
            None,
        )
        .await
        .unwrap();

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
        json!({ "username": "admin", "password": "x".repeat(MAX_LOGIN_REQUEST_BYTES) }),
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
    assert!(admin_login["user"].get("workspace_directory").is_none());
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
    assert!(alice["user"].get("workspace_directory").is_none());
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
    assert!(root_member["user"].get("workspace_directory").is_none());
    let default_workspace_member = request_with_cookie(
        &app,
        Method::POST,
        "/auth/users",
        json!({
            "username": "bob",
            "password": "bob-secure-password",
            "is_admin": false
        }),
        &admin_cookie,
    )
    .await;
    assert_eq!(default_workspace_member.status(), StatusCode::CREATED);
    let default_workspace_member = response_json(default_workspace_member).await;
    assert!(
        default_workspace_member["user"]
            .get("workspace_directory")
            .is_none()
    );
    assert!(app.root.join("blob/users/bob").is_dir());
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

    let (alice_cookie, login) = login_with_password(&app, "alice", "alice-secure-password").await;
    assert!(login["user"].get("workspace_directory").is_none());
    assert!(app.root.join("blob/teams/alice").is_dir());
    let alice_root =
        request_with_cookie(&app, Method::GET, "/directories", json!({}), &alice_cookie).await;
    assert_eq!(alice_root.status(), StatusCode::OK);
    let alice_root = response_json(alice_root).await;
    let folder = request_with_cookie(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": alice_root["directory"]["id"], "name": "empty-folder" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(folder.status(), StatusCode::CREATED);
    let folder = response_json(folder).await;
    assert_eq!(folder["path"], "empty-folder");
    assert_eq!(folder["parent_path"], "");
    assert!(app.root.join("blob/teams/alice/empty-folder").is_dir());
    let nested_listing = request_with_cookie(
        &app,
        Method::GET,
        "/directories?path=empty-folder",
        json!({}),
        &alice_cookie,
    )
    .await;
    assert_eq!(nested_listing.status(), StatusCode::OK);
    assert_eq!(response_json(nested_listing).await["path"], "empty-folder");

    let invalid_folder = request_with_cookie(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_id": "not-a-directory-id", "name": "invalid" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(invalid_folder.status(), StatusCode::BAD_REQUEST);

    let (status, uploaded) = resumable_upload(
        &app,
        "/resources?name=member-upload.txt&directory=",
        "text/plain",
        b"member content",
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(uploaded["directory"], "");
    assert!(
        app.root
            .join("blob/teams/alice/member-upload.txt")
            .is_file()
    );
    let workspace_listing = request_with_cookie(
        &app,
        Method::GET,
        "/directories?path=",
        json!({}),
        &alice_cookie,
    )
    .await;
    assert_eq!(workspace_listing.status(), StatusCode::OK);
    let workspace_listing = response_json(workspace_listing).await;
    assert_eq!(workspace_listing["path"], "");
    assert!(
        workspace_listing["resources"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["id"] == uploaded["id"])
    );
    let resource_listing = request_with_cookie(
        &app,
        Method::GET,
        "/resources?directory=",
        json!({}),
        &alice_cookie,
    )
    .await;
    assert_eq!(resource_listing.status(), StatusCode::OK);
    assert!(
        response_json(resource_listing).await["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["id"] == uploaded["id"])
    );
    let allowed_as_admin = request_with_cookie(
        &app,
        Method::GET,
        &format!("/resources/{}", uploaded["id"].as_str().unwrap()),
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(allowed_as_admin.status(), StatusCode::OK);
    assert_eq!(
        response_json(allowed_as_admin).await["directory"],
        "teams/alice"
    );
    let (status, admin_only) = resumable_upload(
        &app,
        "/resources?name=admin-only.txt&directory=",
        "text/plain",
        b"admin content",
        Some(&admin_cookie),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let denied = request_with_cookie(
        &app,
        Method::GET,
        &format!("/resources/{}", admin_only["id"].as_str().unwrap()),
        json!({}),
        &alice_cookie,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let (status, _) = resumable_upload(
        &app,
        "/resources?name=invalid.txt&directory=..%2Fbob",
        "text/plain",
        b"invalid",
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

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
    resource["actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action["id"] == id))
}

fn sha256_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(data);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn has_directory_action(directory: &Value, id: &str) -> bool {
    directory["actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action["id"] == id))
}

fn install_test_plugin(blob_root: &std::path::Path, source_manifest: &std::path::Path) {
    let manifest: Value = serde_json::from_slice(&std::fs::read(source_manifest).unwrap()).unwrap();
    let plugin_id = manifest["plugin"]["id"].as_str().unwrap();
    let source_root = source_manifest.parent().unwrap();
    let package_root = blob_root.join(".asset-hub/plugins").join(plugin_id);
    std::fs::create_dir_all(&package_root).unwrap();
    for file in ["manifest.json", "plugin.wasm"] {
        std::fs::copy(source_root.join(file), package_root.join(file)).unwrap();
    }
    copy_test_plugin_web(&source_root.join("dist"), &package_root);
    asset_infra::plugin_package::generate_plugin_manifest_lock(&package_root.join("manifest.json"))
        .unwrap();
}

fn copy_test_plugin_web(source: &std::path::Path, destination: &std::path::Path) {
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_test_plugin_web(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
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
