// SPDX-License-Identifier: Apache-2.0

use http_body_util::BodyExt;
use opentelemetry::global;
use opentelemetry::trace::{SpanId, SpanKind, Status};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use pliego_dom::{IntoView, el};
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteSpec};
use pliego_runtime::{
    Body, CompleteDocument, CompleteRenderOptions, HttpScheme, NativeRuntime, NativeRuntimeBuilder,
    OpenTelemetryConfig, RemoteTracePolicy, Request, render_complete_document,
};
use std::collections::BTreeSet;
use tower::ServiceExt;

fn runtime(config: Option<OpenTelemetryConfig>) -> NativeRuntime {
    let graph = RouteGraphBuilder::new()
        .route(RouteSpec::new("user", RouteMethod::post(), "/users/:id").unwrap())
        .seal()
        .unwrap();
    let builder = NativeRuntimeBuilder::new(graph, "otel-test")
        .unwrap()
        .handler("user", |_context, _request| async {
            let document = CompleteDocument::new(
                "Telemetry",
                el("main").child("bounded response").into_view(),
            );
            render_complete_document(&document, CompleteRenderOptions::default())
        });
    let builder = match config {
        Some(config) => builder.open_telemetry(config),
        None => builder,
    };
    builder.build().unwrap()
}

async fn exercise(runtime: &NativeRuntime, target: &str, traceparent: &str) {
    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(target)
                .header("authorization", "Bearer secret-authorization")
                .header("cookie", "session=secret-cookie")
                .header("x-private-value", "secret-header")
                .header("traceparent", traceparent)
                .header("tracestate", "vendor=secret-trace-state")
                .body(Body::from("secret-request-body"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("bounded response")
    );
}

#[tokio::test]
async fn exports_standard_bounded_signals_without_request_secrets() {
    let span_exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_simple_exporter(span_exporter.clone())
        .build();
    global::set_tracer_provider(tracer_provider.clone());

    let metric_exporter = InMemoryMetricExporter::default();
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(metric_exporter.clone()).build())
        .build();
    global::set_meter_provider(meter_provider.clone());

    let remote_parent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
    let accepted = runtime(Some(
        OpenTelemetryConfig::new(HttpScheme::Https)
            .remote_trace_policy(RemoteTracePolicy::AcceptW3c),
    ));
    exercise(
        &accepted,
        "/users/secret-user-alpha?token=secret-query-alpha",
        remote_parent,
    )
    .await;
    exercise(
        &accepted,
        "/users/secret-user-gamma?token=secret-query-gamma",
        "00-invalid-trace-id-invalid-parent-id-01",
    )
    .await;

    let ignored = runtime(Some(OpenTelemetryConfig::new(HttpScheme::Https)));
    exercise(
        &ignored,
        "/users/secret-user-beta?token=secret-query-beta",
        remote_parent,
    )
    .await;

    let uninstrumented = runtime(None);
    exercise(
        &uninstrumented,
        "/users/secret-user-delta?token=secret-query-delta",
        remote_parent,
    )
    .await;

    tracer_provider.force_flush().unwrap();
    meter_provider.force_flush().unwrap();

    let spans = span_exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 3);
    assert!(spans.iter().all(|span| span.span_kind == SpanKind::Server));
    assert!(spans.iter().all(|span| span.name == "POST /users/:id"));
    assert!(spans.iter().all(|span| span.status == Status::Unset));
    let span_duration_sum = spans
        .iter()
        .map(|span| {
            span.end_time
                .duration_since(span.start_time)
                .expect("span end must not precede its start")
                .as_secs_f64()
        })
        .sum::<f64>();
    let remote = spans
        .iter()
        .find(|span| span.parent_span_is_remote)
        .expect("opt-in runtime must accept a valid W3C parent");
    assert_eq!(
        remote.parent_span_id,
        SpanId::from_hex("b7ad6b7169203331").unwrap()
    );
    assert_eq!(
        spans
            .iter()
            .filter(|span| span.parent_span_is_remote)
            .count(),
        1,
        "default-ignore and malformed W3C contexts must both start new traces"
    );

    let expected_span_keys = BTreeSet::from([
        "http.request.method",
        "http.response.body.size",
        "http.response.status_code",
        "http.route",
        "network.protocol.version",
        "pliego.render.mode",
        "pliego.route.id",
        "pliego.runtime.outcome",
        "pliego.runtime.receipt",
        "url.scheme",
    ]);
    for span in &spans {
        let keys = span
            .attributes
            .iter()
            .map(|attribute| attribute.key.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(keys, expected_span_keys);
        assert_eq!(
            span.attributes
                .iter()
                .find(|attribute| attribute.key.as_str() == "url.scheme")
                .expect("trusted scheme attribute must exist")
                .value
                .as_str(),
            "https"
        );
        assert!(span.events.is_empty());
    }

    let metrics = metric_exporter.get_finished_metrics().unwrap();
    let mut metric_names = BTreeSet::new();
    let mut data_point_counts = Vec::new();
    for resource in &metrics {
        for scope in resource.scope_metrics() {
            assert_eq!(scope.scope().name(), "dev.pliegors.runtime");
            for metric in scope.metrics() {
                let name = metric.name();
                metric_names.insert(name);
                let count = match metric.data() {
                    AggregatedMetrics::F64(MetricData::Histogram(histogram)) => {
                        assert_eq!(name, "http.server.request.duration");
                        let points = histogram.data_points().collect::<Vec<_>>();
                        assert_eq!(points.len(), 1);
                        assert_eq!(points[0].count(), 3);
                        assert!((points[0].sum() - span_duration_sum).abs() < 0.000_001);
                        assert_eq!(
                            points[0].bounds().collect::<Vec<_>>(),
                            vec![
                                0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5,
                                5.0, 7.5, 10.0,
                            ]
                        );
                        assert_eq!(
                            points[0]
                                .attributes()
                                .find(|attribute| attribute.key.as_str() == "url.scheme")
                                .expect("duration scheme attribute must exist")
                                .value
                                .as_str(),
                            "https"
                        );
                        points.len()
                    }
                    AggregatedMetrics::U64(MetricData::Histogram(histogram)) => {
                        assert_eq!(name, "http.server.response.body.size");
                        let points = histogram.data_points().collect::<Vec<_>>();
                        assert_eq!(points.len(), 1);
                        assert_eq!(points[0].count(), 3);
                        assert_eq!(
                            points[0]
                                .attributes()
                                .find(|attribute| attribute.key.as_str() == "url.scheme")
                                .expect("response size scheme attribute must exist")
                                .value
                                .as_str(),
                            "https"
                        );
                        points.len()
                    }
                    AggregatedMetrics::I64(MetricData::Sum(sum)) => {
                        assert_eq!(name, "http.server.active_requests");
                        let points = sum.data_points().collect::<Vec<_>>();
                        assert_eq!(points.len(), 1);
                        assert_eq!(points[0].value(), 0);
                        assert_eq!(
                            points[0]
                                .attributes()
                                .find(|attribute| attribute.key.as_str() == "url.scheme")
                                .expect("active request scheme attribute must exist")
                                .value
                                .as_str(),
                            "https"
                        );
                        points.len()
                    }
                    other => panic!("unexpected metric aggregation: {other:?}"),
                };
                data_point_counts.push((name.to_owned(), count));
            }
        }
    }
    assert_eq!(
        metric_names,
        BTreeSet::from([
            "http.server.active_requests",
            "http.server.request.duration",
            "http.server.response.body.size",
        ])
    );
    assert!(
        data_point_counts.iter().all(|(_, count)| *count == 1),
        "dynamic route values must collapse to one metric series: {data_point_counts:?}"
    );

    let exported = format!("{spans:?}{metrics:?}");
    for forbidden in [
        "secret-user-alpha",
        "secret-user-beta",
        "secret-user-gamma",
        "secret-user-delta",
        "secret-query-alpha",
        "secret-query-beta",
        "secret-query-gamma",
        "secret-query-delta",
        "secret-authorization",
        "secret-cookie",
        "secret-header",
        "secret-trace-state",
        "secret-request-body",
    ] {
        assert!(
            !exported.contains(forbidden),
            "exported telemetry contains forbidden request value {forbidden}"
        );
    }
}
