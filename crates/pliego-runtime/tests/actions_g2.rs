// SPDX-License-Identifier: Apache-2.0

use axum::body::to_bytes;
use flate2::Compression;
use flate2::write::GzEncoder;
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteSpec};
use pliego_runtime::{
    ActionContentEncoding, ActionInvalidationIntent, ActionMediaType, ActionNavigation,
    ActionPolicy, ActionRequestSecurity, ActionResponse, Body, CacheTag, DataError,
    MultipartFieldKind, MultipartPolicy, NativeRuntime, NativeRuntimeBuilder, Request,
    RequestLimits, Response, RuntimeBuildError, StatusCode, action_failure_to_handler_error,
    decode_action_request, decode_multipart_action_request,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CompressedInput {
    value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UploadInput {
    title: String,
    bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MutationOutput {
    accepted: usize,
}

type FieldErrors = BTreeMap<String, String>;

fn action_policy(id: &str, media_type: ActionMediaType) -> ActionPolicy {
    ActionPolicy::new(
        id,
        1,
        "mutation-input",
        "mutation-errors",
        "mutation-output",
    )
    .unwrap()
    .accept_media_type(media_type)
    .max_encoded_bytes(2 * 1_024)
    .unwrap()
    .max_decoded_bytes(1_024)
    .unwrap()
}

fn compressed_runtime() -> NativeRuntime {
    let policy = action_policy("compressed-submit", ActionMediaType::FormUrlencoded)
        .accept_content_encoding(ActionContentEncoding::Gzip);
    let graph = RouteGraphBuilder::new()
        .route(
            RouteSpec::new("compressed", RouteMethod::post(), "/compressed")
                .unwrap()
                .action("compressed-submit")
                .unwrap(),
        )
        .seal()
        .unwrap();
    NativeRuntimeBuilder::new(graph, "compressed-actions")
        .unwrap()
        .limits(RequestLimits {
            allow_gzip_request_bodies: true,
            ..RequestLimits::default()
        })
        .unwrap()
        .action_policy(policy)
        .handler("compressed", |context: pliego_runtime::RequestContext, request| async move {
            let policy = context
                .action_policy("compressed-submit")
                .expect("sealed action is available")
                .clone();
            let security = ActionRequestSecurity::new("https://example.com")?
                .authenticated(true)
                .authorized(true)
                .csrf_verified(true);
            let (input, admission) =
                decode_action_request::<CompressedInput>(&context, &policy, request, &security)
                    .await?;
            let mutation = |action: pliego_runtime::ActionContext, input: CompressedInput| async move {
                action.commit().begin_commit()?;
                action.commit().committed()?;
                Ok::<_, DataError>(ActionResponse::<MutationOutput, FieldErrors>::Success {
                    output: MutationOutput {
                        accepted: input.value.len(),
                    },
                    navigation: ActionNavigation::Stay,
                })
            };
            context
                .data()
                .act(&policy, &admission, &mutation, input)
                .await
                .map_err(action_failure_to_handler_error)?;
            Ok(Response::new(Body::from("accepted")))
        })
        .build()
        .unwrap()
}

fn gzip(value: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(value).unwrap();
    encoder.finish().unwrap()
}

#[tokio::test]
async fn gzip_action_is_explicit_bounded_and_uses_the_same_admission_path() {
    let runtime = compressed_runtime();
    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/compressed")
                .header("origin", "https://example.com")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("content-encoding", "gzip")
                .body(Body::from(gzip(b"value=PliegoRS")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "accepted"
    );
}

#[tokio::test]
async fn malformed_stacked_and_expanding_gzip_fail_inside_declared_bounds() {
    let router = compressed_runtime().router();
    let malformed = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/compressed")
                .header("origin", "https://example.com")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("content-encoding", "gzip")
                .body(Body::from("not-gzip"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    let expanded = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/compressed")
                .header("origin", "https://example.com")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("content-encoding", "gzip")
                .body(Body::from(gzip(
                    format!("value={}", "a".repeat(2_000)).as_bytes(),
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(expanded.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let field_flood = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/compressed")
                .header("origin", "https://example.com")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "value=ok&{}",
                    std::iter::repeat_n("extra=", 256)
                        .collect::<Vec<_>>()
                        .join("&")
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(field_flood.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let stacked = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/compressed")
                .header("origin", "https://example.com")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("content-encoding", "gzip, identity")
                .body(Body::from(gzip(b"value=hidden")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stacked.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

fn multipart_runtime(policy: MultipartPolicy) -> NativeRuntime {
    let action_policy = action_policy("upload-profile", ActionMediaType::MultipartFormData);
    let graph = RouteGraphBuilder::new()
        .route(
            RouteSpec::new("upload", RouteMethod::post(), "/upload")
                .unwrap()
                .action("upload-profile")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let multipart_policy = Arc::new(policy);
    NativeRuntimeBuilder::new(graph, "multipart-actions")
        .unwrap()
        .limits(RequestLimits {
            max_body_bytes: 4 * 1_024,
            allow_multipart_request_bodies: true,
            ..RequestLimits::default()
        })
        .unwrap()
        .action_policy(action_policy)
        .handler("upload", move |context: pliego_runtime::RequestContext, request| {
            let multipart_policy = multipart_policy.clone();
            async move {
                let policy = context
                    .action_policy("upload-profile")
                    .expect("sealed action is available")
                    .clone();
                let security = ActionRequestSecurity::new("https://example.com")?
                    .authenticated(true)
                    .authorized(true)
                    .csrf_verified(true);
                let (form, admission) = decode_multipart_action_request(
                    &context,
                    &policy,
                    &multipart_policy,
                    request,
                    &security,
                )
                .await?;
                let debug = format!("{form:?}");
                assert!(!debug.contains("private-title"));
                assert!(!debug.contains("../../avatar.txt"));
                let title = form.text("title").unwrap_or_default().to_owned();
                let file = form.file("avatar").ok_or_else(|| {
                    pliego_runtime::HandlerError::internal("avatar file is required")
                })?;
                let bytes = file.read(32).await.map_err(|error| {
                    pliego_runtime::HandlerError::internal(error.code())
                })?;
                assert_eq!(file.original_filename(), Some("../../avatar.txt"));
                let input = UploadInput {
                    title,
                    bytes: bytes.len(),
                };
                let mutation = |action: pliego_runtime::ActionContext, input: UploadInput| async move {
                    action.commit().begin_commit()?;
                    action.commit().committed()?;
                    Ok::<_, DataError>(ActionResponse::<MutationOutput, FieldErrors>::Success {
                        output: MutationOutput {
                            accepted: input.title.len() + input.bytes,
                        },
                        navigation: ActionNavigation::Stay,
                    })
                };
                context
                    .data()
                    .act(&policy, &admission, &mutation, input)
                    .await
                    .map_err(action_failure_to_handler_error)?;
                Ok(Response::new(Body::from("uploaded")))
            }
        })
        .build()
        .unwrap()
}

fn multipart_body(file: &[u8], extra_field: Option<&str>) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"--pliego-boundary\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nprivate-title\r\n");
    body.extend_from_slice(b"--pliego-boundary\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"../../avatar.txt\"\r\nContent-Type: text/plain\r\n\r\n");
    body.extend_from_slice(file);
    body.extend_from_slice(b"\r\n");
    if let Some(name) = extra_field {
        body.extend_from_slice(format!("--pliego-boundary\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\nvalue\r\n").as_bytes());
    }
    body.extend_from_slice(b"--pliego-boundary--\r\n");
    body
}

#[tokio::test]
async fn multipart_upload_uses_random_temp_names_and_cleans_every_request() {
    let directory = tempfile::tempdir().unwrap();
    let policy = MultipartPolicy::new(directory.path())
        .unwrap()
        .allow_field("title", MultipartFieldKind::Text)
        .unwrap()
        .allow_field("avatar", MultipartFieldKind::File)
        .unwrap()
        .limits(4, 1, 64, 32, 1_024, 255)
        .unwrap();
    let router = multipart_runtime(policy).router();
    for _ in 0..20 {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/upload")
                    .header("origin", "https://example.com")
                    .header(
                        "content-type",
                        "multipart/form-data; boundary=pliego-boundary",
                    )
                    .body(Body::from(multipart_body(b"avatar-bytes", None)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 32).await.unwrap(),
            "uploaded"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn hostile_multipart_fields_and_file_sizes_fail_without_temp_leaks() {
    let directory = tempfile::tempdir().unwrap();
    let policy = MultipartPolicy::new(directory.path())
        .unwrap()
        .allow_field("title", MultipartFieldKind::Text)
        .unwrap()
        .allow_field("avatar", MultipartFieldKind::File)
        .unwrap()
        .limits(4, 1, 64, 32, 1_024, 255)
        .unwrap();
    let router = multipart_runtime(policy).router();

    let unknown = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .header("origin", "https://example.com")
                .header(
                    "content-type",
                    "multipart/form-data; boundary=pliego-boundary",
                )
                .body(Body::from(multipart_body(b"small", Some("admin"))))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let _ = to_bytes(unknown.into_body(), 128).await.unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);

    let oversized = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .header("origin", "https://example.com")
                .header(
                    "content-type",
                    "multipart/form-data; boundary=pliego-boundary",
                )
                .body(Body::from(multipart_body(&[b'x'; 64], None)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let _ = to_bytes(oversized.into_body(), 128).await.unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn action_invalidation_policy_must_be_registered_in_the_sealed_runtime() {
    let graph = RouteGraphBuilder::new()
        .route(
            RouteSpec::new("submit", RouteMethod::post(), "/submit")
                .unwrap()
                .action("submit-action")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let policy = action_policy("submit-action", ActionMediaType::FormUrlencoded)
        .invalidation(
            ActionInvalidationIntent::tags(
                "missing-cache",
                [CacheTag::new("submitted-items").unwrap()],
            )
            .unwrap()
            .read_your_writes(),
        )
        .unwrap();
    let result = NativeRuntimeBuilder::new(graph, "missing-invalidation-cache")
        .unwrap()
        .action_policy(policy)
        .handler("submit", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        result,
        Err(RuntimeBuildError::MissingCachePolicy(id)) if id == "missing-cache"
    ));
}
