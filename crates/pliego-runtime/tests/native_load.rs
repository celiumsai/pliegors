// SPDX-License-Identifier: Apache-2.0

use http::Response;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteSpec};
use pliego_runtime::{Body, NativeRuntimeBuilder};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const TOTAL_REQUESTS: usize = 2_000;
const CONCURRENCY: usize = 32;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "bounded Linux evidence harness; run explicitly"]
async fn fixed_http2_load_reaches_a_memory_plateau() {
    let graph = RouteGraphBuilder::new()
        .route(RouteSpec::new("health", RouteMethod::get(), "/health").unwrap())
        .seal()
        .unwrap();
    let receipts = Arc::new(AtomicUsize::new(0));
    let runtime = NativeRuntimeBuilder::new(graph, "fixed-load-evidence")
        .unwrap()
        .handler("health", |_context, _request| async {
            Ok(Response::new(Body::from("ok")))
        })
        .receipt_sink({
            let receipts = receipts.clone();
            move |_| {
                receipts.fetch_add(1, Ordering::AcqRel);
            }
        })
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_runtime = runtime.clone();
    let server = tokio::spawn(async move {
        server_runtime
            .serve(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let stream = TcpStream::connect(address).await.unwrap();
    let (sender, connection) = http2::Builder::new(TokioExecutor::new())
        .handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let client = tokio::spawn(connection);

    let warmup = run_wave(sender.clone(), 128, CONCURRENCY).await;
    assert_eq!(warmup.len(), 128);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let baseline_rss_kib = rss_kib().expect("Linux /proc/self/status is required");

    let sampling = Arc::new(AtomicBool::new(true));
    let peak_rss_kib = Arc::new(AtomicUsize::new(baseline_rss_kib));
    let sampler = tokio::spawn({
        let sampling = sampling.clone();
        let peak_rss_kib = peak_rss_kib.clone();
        async move {
            while sampling.load(Ordering::Acquire) {
                if let Some(current) = rss_kib() {
                    peak_rss_kib.fetch_max(current, Ordering::AcqRel);
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    });
    let latencies = run_wave(sender.clone(), TOTAL_REQUESTS, CONCURRENCY).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let settled_rss_kib = rss_kib().unwrap();
    sampling.store(false, Ordering::Release);
    sampler.await.unwrap();

    let peak_rss_kib = peak_rss_kib.load(Ordering::Acquire);
    let peak_growth_kib = peak_rss_kib.saturating_sub(baseline_rss_kib);
    let settled_growth_kib = settled_rss_kib.saturating_sub(baseline_rss_kib);
    let p50_us = percentile_us(&latencies, 50);
    let p95_us = percentile_us(&latencies, 95);
    let p99_us = percentile_us(&latencies, 99);
    assert_eq!(latencies.len(), TOTAL_REQUESTS);
    assert_eq!(receipts.load(Ordering::Acquire), TOTAL_REQUESTS + 128);
    assert_eq!(runtime.active_request_count(), 0);
    assert!(
        peak_growth_kib <= 64 * 1_024,
        "fixed-load RSS grew by {peak_growth_kib} KiB"
    );
    assert!(
        settled_growth_kib <= 32 * 1_024,
        "settled RSS grew by {settled_growth_kib} KiB"
    );
    eprintln!(
        "PLIEGORS_LOAD_EVIDENCE={{\"protocol\":\"h2c\",\"requests\":{TOTAL_REQUESTS},\"concurrency\":{CONCURRENCY},\"p50Us\":{p50_us},\"p95Us\":{p95_us},\"p99Us\":{p99_us},\"baselineRssKiB\":{baseline_rss_kib},\"peakRssKiB\":{peak_rss_kib},\"settledRssKiB\":{settled_rss_kib}}}"
    );

    drop(sender);
    client.await.unwrap().unwrap();
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

async fn run_wave(
    sender: hyper::client::conn::http2::SendRequest<Empty<bytes::Bytes>>,
    total: usize,
    concurrency: usize,
) -> Vec<Duration> {
    let next = Arc::new(AtomicUsize::new(0));
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(total)));
    let mut workers = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let mut sender = sender.clone();
        let next = next.clone();
        let latencies = latencies.clone();
        workers.push(tokio::spawn(async move {
            loop {
                let index = next.fetch_add(1, Ordering::AcqRel);
                if index >= total {
                    break;
                }
                let started = Instant::now();
                let response = sender
                    .send_request(
                        http::Request::builder()
                            .version(http::Version::HTTP_2)
                            .uri("http://localhost/health")
                            .body(Empty::<bytes::Bytes>::new())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), http::StatusCode::OK);
                assert_eq!(
                    response.into_body().collect().await.unwrap().to_bytes(),
                    "ok"
                );
                lock(&latencies).push(started.elapsed());
            }
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }
    Arc::try_unwrap(latencies).unwrap().into_inner().unwrap()
}

fn percentile_us(values: &[Duration], percentile: usize) -> u128 {
    let mut values: Vec<_> = values.iter().map(Duration::as_micros).collect();
    values.sort_unstable();
    let index = (values.len() - 1) * percentile / 100;
    values[index]
}

#[cfg(target_os = "linux")]
fn rss_kib() -> Option<usize> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
}

#[cfg(not(target_os = "linux"))]
fn rss_kib() -> Option<usize> {
    None
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
