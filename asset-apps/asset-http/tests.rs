use crate::router;
use asset_apps::AssetRuntime;
use asset_infra::config::{AssetInfraConfig, BlobConfig, DatabaseConfig};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

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
async fn create_resource_accepts_structured_metadata_and_rejects_metadata_string() {
    let app = test_app("create-resource").await;

    let (status, resource) = json_request(
        &app,
        Method::POST,
        "/resources",
        json!({
            "name": "resources_not_blob",
            "kind": "asset:document",
            "metadata": {
                "description": "metadata-only resource",
                "tags": ["demo", "document"],
                "attributes": {
                    "A": "a",
                    "B": "b"
                }
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["name"], "resources_not_blob");
    assert_eq!(resource["kind"], "asset:document");
    assert_eq!(resource["metadata"]["schema_version"], 1);
    assert_eq!(
        resource["metadata"]["description"],
        "metadata-only resource"
    );
    assert_eq!(resource["metadata"]["tags"], json!(["demo", "document"]));
    assert_eq!(resource["metadata"]["attributes"]["A"], "a");

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
            "kind": "asset:text",
            "storage_key": "examples/hello.txt",
            "data_base64": "aGVsbG8sIGFzc2V0LWh1YiE=",
            "metadata": {
                "description": "small text file",
                "tags": ["demo", "text"],
                "attributes": {
                    "source": "test"
                }
            },
            "mime_type": "text/plain",
            "original_filename": "hello.txt"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resource["content"]["key"], "examples/hello.txt");
    assert_eq!(resource["content"]["size"], data.len() as u64);
    assert_eq!(resource["content"]["mime_type"], "text/plain");
    assert_eq!(resource["metadata"]["attributes"]["source"], "test");

    let id = resource["id"].as_str().unwrap();
    let (status, content) =
        empty_bytes_request(&app, Method::GET, &format!("/resources/{id}/content")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content.as_ref(), data);
}

#[tokio::test]
async fn stream_upload_roundtrips_large_blob_without_buffered_request_dto() {
    let app = test_app("stream-upload").await;
    let data = b"large file bytes";

    let response = request(
        &app,
        Request::builder()
            .method(Method::PUT)
            .uri("/resources/content/stream?name=large-file&storage_key=streams/large.bin&kind=asset%3Abinary")
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
async fn openapi_documents_metadata_examples() {
    let app = test_app("openapi").await;
    let (status, document) = empty_json_request(&app, Method::GET, "/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);

    let metadata_example = &document["components"]["schemas"]["ResourceMetadataRequest"]["example"];
    let create_example = &document["components"]["schemas"]["CreateResourceRequest"]["example"];
    let upload_example =
        &document["components"]["schemas"]["UploadResourceContentRequest"]["example"];

    assert_eq!(metadata_example["attributes"]["A"], "a");
    assert_eq!(
        create_example["metadata"]["attributes"]["A"],
        metadata_example["attributes"]["A"]
    );
    assert_eq!(upload_example["data_base64"], "aGVsbG8sIGFzc2V0LWh1YiE=");
}

async fn test_app(name: &str) -> TestApp {
    let root = unique_temp_root(name);
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite_path: root.join("asset-hub.sqlite"),
            max_connections: 1,
        },
        blob: BlobConfig {
            fs_root: root.join("blob"),
        },
    };
    let runtime = AssetRuntime::from_config(config).await.unwrap();
    let router = router::build(runtime.resource_service());

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
