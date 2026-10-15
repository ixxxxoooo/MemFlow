// @author ixxxxoooo
//! 集成测试:Ollama 探测缓存——短时间内多次读取状态只访问一次 Ollama,失效后重新探测。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use memflow_lib::core::test_core;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

async fn start_mock() -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/version",
            get(|State(h): State<Arc<AtomicUsize>>| async move {
                h.fetch_add(1, Ordering::SeqCst);
                Json(json!({"version": "mock"}))
            }),
        )
        .route(
            "/api/tags",
            get(|| async { Json::<Value>(json!({"models": [{"name": "qwen2.5:7b", "size": 1}]})) }),
        )
        .with_state(hits.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), hits)
}

#[tokio::test]
async fn 探测结果五秒内复用且失效后重新探测() {
    let (url, hits) = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);

    let a = core.probe_ollama(false).await;
    let b = core.probe_ollama(false).await;
    let c = core.probe_ollama(false).await;
    assert!(a.running && a.installed == vec!["qwen2.5:7b".to_string()]);
    assert_eq!(a, b);
    assert_eq!(b, c);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "缓存期内只应访问一次");

    core.invalidate_probe();
    core.probe_ollama(false).await;
    assert_eq!(hits.load(Ordering::SeqCst), 2, "失效后应重新探测");

    core.probe_ollama(true).await;
    assert_eq!(hits.load(Ordering::SeqCst), 3, "force 应跳过缓存");
}

#[tokio::test]
async fn 改地址后不复用旧地址的缓存() {
    let (url, _h) = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    assert!(core.probe_ollama(false).await.running);

    let mut s = core.settings();
    s.ollama_url = "http://127.0.0.1:1".into();
    core.update_settings(s).unwrap();
    assert!(!core.probe_ollama(false).await.running);
}
