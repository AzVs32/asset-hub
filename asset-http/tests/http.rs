use asset_core::domain::{AccessContext, UserId};
use asset_http::{
    CorsPolicy, MAX_ACTION_REQUEST_BYTES, MAX_LOGIN_REQUEST_BYTES, RouterOptions, SessionOptions,
    build_router, with_authentication,
};
use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, KindRegistryConfig, LocalBlobConfig,
    LocalBlobSyncConfig, PluginHostConfig, ResourceKindConfig, SqliteDatabaseConfig,
};
use asset_plugin_api::PluginWebAssets;
use asset_runtime::AssetRuntime;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::{Extension, Router};
use bytes::Bytes;
use futures_util::stream;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;
use tower_sessions_sqlx_store::SqliteStore;

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
    let default = kinds["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|kind| kind["kind"] == "core:resource")
        .unwrap();
    assert!(default["parent"].is_null());
    for (kind, source) in [
        ("core:resource", "plugin:core.resource"),
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
        .find(|kind| kind["kind"] == "core:resource")
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
            "name": "contentless note",
            "kind": "doc:note"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{resource}");
    assert_eq!(resource["kind"], "doc:note");
    assert!(!has_action(&resource, "core.resource.download"));

    let (status, error) = stream_upload(
        &app,
        "/resources/content/stream?name=note.txt&kind=doc%3Anote&directory=notes",
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
async fn core_document_resource_inherits_core_download_action() {
    let app = test_app("core-document-download").await;

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
        .find(|action| action["id"] == "core.resource.download")
        .unwrap();
    assert_eq!(download["label"], "Download");
    assert_eq!(download["access"], "read_only");
    assert_eq!(download["executor"]["type"], "builtin");
    assert_eq!(download["executor"]["handler"], "builtin.resource.download");
    assert_eq!(download["requires"]["content_delivery"], "reference");
    assert_eq!(download["output"]["view"], json!(["download"]));
    assert_eq!(
        download["ui"]["locations"],
        json!(["resource_detail", "context_menu"])
    );

    assert_eq!(actions.len(), 1);

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=book.txt&kind=core%3Adocument&directory=books",
        "text/plain",
        b"Hello book",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(has_action(&resource, "core.resource.download"));
    assert_eq!(
        resource["actions"]["available_actions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

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
    assert_eq!(body.as_ref(), b"Hello book");

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
}

#[tokio::test]
async fn create_resource_accepts_tags() {
    let app = test_app("create-resource").await;

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "resources_not_blob",
            "kind": "core:resource",
            "tags": ["demo", "document"]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["name"], "resources_not_blob");
    assert_eq!(resource["kind"], "core:resource");
    assert_eq!(resource["tags"], json!(["demo", "document"]));

    let id = resource["id"].as_str().unwrap();
    let (status, found) = empty_json_request(&app, Method::GET, &format!("/resources/{id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["id"], id);
}

#[tokio::test]
async fn stream_upload_roundtrips_small_blob_and_creates_directories() {
    let app = test_app("small-upload").await;
    let data = b"hello, asset-hub!";

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=hello.txt&kind=core%3Aresource&directory=examples",
        "text/plain",
        data,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["size"], data.len() as u64);
    assert_eq!(resource["content"]["mime_type"], "text/plain");
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
        "/resources/content/stream?name=nested.txt&kind=core%3Aresource&directory=examples%2Fnested",
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
async fn stream_upload_preserves_spaces_in_names_and_physical_paths() {
    let app = test_app("spaced-upload").await;
    let data = b"spaces are part of the path";

    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=%20draft%20%2001.txt%20&kind=core%3Aresource&directory=%20library%20%2Fproject%20A%20",
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
async fn empty_directories_and_contentless_resources_create_physical_directories() {
    let app = test_app("physical-directories").await;

    let (status, _) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_path": "", "name": "projects" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, directory) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_path": "projects", "name": "empty" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(directory["path"], "projects/empty");
    assert!(app.root.join("blob/projects/empty").is_dir());

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({ "name": "placeholder", "directory": "projects/contentless" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(resource["content"].is_null());
    assert!(app.root.join("blob/projects/contentless").is_dir());

    let (status, _) = json_request(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_path": "", "name": ".asset-hub" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn resource_content_supports_single_byte_ranges_for_video_seek() {
    let app = test_app("content-range").await;
    let data = b"0123456789";
    let (status, resource) = stream_upload(
        &app,
        "/resources/content/stream?name=clip.mp4",
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
async fn upload_rejects_client_supplied_checksum_and_existing_resource_path() {
    let app = test_app("upload-security").await;

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=bad.txt&directory=secure&sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("hello, asset-hub!"))
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
            .method(Method::PUT)
            .uri("/resources/content/stream?name=unsupported.txt&directory=secure&checksum_kind=sha256")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("hello, asset-hub!"))
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
        "/resources/content/stream?name=existing.txt&directory=secure",
        "text/plain",
        b"hello, asset-hub!",
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["error"].as_str().unwrap().contains("already exists"));
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
            .uri("/resources/content/stream?name=slow.txt&directory=uploads")
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
            .uri("/resources/content/stream?name=README.md")
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
            .any(|action| action["id"] == "core.resource.download")
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
        "/resources/content/stream?name=README.md",
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
    let original_blob_path = app.root.join("blob/delete/me.txt");
    let trash_blob_path = app.root.join(format!("blob/.asset-hub/trash/{id}"));

    let (status, deleted) =
        empty_json_request(&app, Method::DELETE, &format!("/resources/{id}")).await;

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
        json!({ "restore": true }),
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
            "kind": "core:resource",
            "tags": ["alpha", "docs"]
        }),
    )
    .await;
    let (_, second) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "beta image",
            "kind": "core:resource",
            "tags": ["beta", "media"]
        }),
    )
    .await;
    let (_, third) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "alpha image",
            "kind": "core:resource",
            "tags": ["alpha", "media"]
        }),
    )
    .await;

    assert!(first["id"].is_string());
    assert!(second["id"].is_string());
    assert!(third["id"].is_string());

    let (status, page) = empty_json_request(
        &app,
        Method::GET,
        "/resources?kind=core%3Aresource&tag=alpha&q=image&page=1&limit=10",
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
        json!(["core:code", "core:document", "core:resource"])
    );
    assert!(
        c["actions"]
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

    let (status, updated) = json_request(
        &app,
        Method::PATCH,
        &format!("/resources/{id}"),
        json!({
            "name": "updated.txt",
            "directory": "archive",
            "kind": "core:resource",
            "status": "archived",
            "tags": ["updated"]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "updated.txt");
    assert_eq!(updated["directory"], "archive");
    assert_eq!(updated["kind"], "core:resource");
    assert_eq!(updated["status"], "archived");
    assert_eq!(updated["tags"], json!(["updated"]));
    assert!(!old_blob_path.exists());
    assert_eq!(std::fs::read(&new_blob_path).unwrap(), b"delete me");

    let (status, _) = empty_json_request(&app, Method::DELETE, &format!("/resources/{id}")).await;
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
        json!({
            "restore": true,
            "status": "active"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["status"], "active");
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
    let create_properties = &schemas["CreateResourceRequest"]["properties"];
    let update_properties = &schemas["UpdateResourceRequest"]["properties"];
    let response_properties = &schemas["ResourceResponse"]["properties"];
    assert!(create_properties.get("description").is_none());
    assert!(update_properties.get("description").is_none());
    assert!(response_properties.get("description").is_none());
    assert!(create_properties.get("tags").is_some());
    let upload_parameters = document["paths"]["/resources/content/stream"]["put"]["parameters"]
        .as_array()
        .unwrap();
    assert!(
        !upload_parameters
            .iter()
            .any(|parameter| { parameter["name"] == "description" && parameter["in"] == "query" })
    );
    assert!(document["paths"].get("/resources/content/stream").is_some());
    assert!(document["paths"].get("/resources/{id}/download").is_some());
    assert!(document["paths"].get("/resources/{id}/read").is_none());
    assert!(document["paths"].get("/resources/{id}/preview").is_none());
    assert!(document["paths"].get("/resources/{id}/thumbnail").is_none());
    assert!(document["paths"].get("/auth/login").is_some());
    assert!(document["paths"].get("/auth/users/{id}").is_some());
    assert!(document["paths"].get("/scan").is_none());
    assert!(document["paths"].get("/audit").is_none());
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
        kind,
        plugin,
    };
    let runtime = AssetRuntime::new(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = build_router(
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
        kind: KindRegistryConfig::default(),
        plugin: Default::default(),
    };
    let runtime = AssetRuntime::new(config).await.unwrap();
    let authorization = runtime.authorization_service();
    let router = build_router(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        RouterOptions::default(),
        plugin_web_assets,
        authorization,
    )
    .layer(Extension(AccessContext::administrator(UserId::new())));

    TestApp { router, root }
}

async fn create_text_resource(app: &TestApp, path: &str) -> String {
    let (directory, name) = path
        .rsplit_once('/')
        .map_or(("", path), |(directory, name)| (directory, name));
    let uri = format!(
        "/resources/content/stream?name={name}&directory={}",
        directory.replace('/', "%2F")
    );
    let (status, resource) = stream_upload(app, &uri, "text/plain", b"delete me").await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["kind"], "core:resource");

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
async fn member_access_is_limited_to_the_workspace_subtree() {
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
        kind: KindRegistryConfig::default(),
        plugin: Default::default(),
    };
    let runtime = AssetRuntime::new(config).await.unwrap();
    assert_eq!(
        runtime.database_pool().options().get_max_connections(),
        1,
        "HTTP sessions must reuse the configured infrastructure pool",
    );
    let authorization = runtime.authorization_service();
    let base = build_router(
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
    let session_store = SqliteStore::new(runtime.database_pool());
    session_store.migrate().await.unwrap();
    let router = with_authentication(
        base,
        runtime.user_service(),
        runtime.security_audit_repository(),
        session_store,
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
    let folder = request_with_cookie(
        &app,
        Method::POST,
        "/directories",
        json!({ "parent_path": "", "name": "empty-folder" }),
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
        json!({ "parent_path": "../bob", "name": "invalid" }),
        &alice_cookie,
    )
    .await;
    assert_eq!(invalid_folder.status(), StatusCode::BAD_REQUEST);

    let allowed = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "allowed", "directory": ""
        }),
        &alice_cookie,
    )
    .await;
    assert_eq!(allowed.status(), StatusCode::CREATED);
    let allowed = response_json(allowed).await;
    assert_eq!(allowed["directory"], "");
    let uploaded = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=member-upload.txt&directory=")
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("member content"))
            .unwrap(),
    )
    .await;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let uploaded = response_json(uploaded).await;
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
            .any(|resource| resource["id"] == allowed["id"])
    );
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
        &format!("/resources/{}", allowed["id"].as_str().unwrap()),
        json!({}),
        &admin_cookie,
    )
    .await;
    assert_eq!(allowed_as_admin.status(), StatusCode::OK);
    assert_eq!(
        response_json(allowed_as_admin).await["directory"],
        "teams/alice"
    );
    let admin_only = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({ "name": "admin-only", "directory": "" }),
        &admin_cookie,
    )
    .await;
    assert_eq!(admin_only.status(), StatusCode::CREATED);
    let admin_only = response_json(admin_only).await;
    let denied = request_with_cookie(
        &app,
        Method::GET,
        &format!("/resources/{}", admin_only["id"].as_str().unwrap()),
        json!({}),
        &alice_cookie,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let invalid = request_with_cookie(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "invalid", "directory": "../bob"
        }),
        &alice_cookie,
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

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
            && event["source"] == "http"
            && event["outcome"] == "failure"
            && event["target"] == "admin"
            && event["actor_user_id"].is_null()
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == "auth.user.status"
            && event["source"] == "http"
            && event["actor_user_id"].is_string()
            && event["outcome"] == "success"
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
