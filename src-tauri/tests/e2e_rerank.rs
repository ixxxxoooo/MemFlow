// @author ygw
//! 集成测试:本地重排序接入检索。
//! 默认用例验证"未安装模型时检索不受影响";真实模型用例需要先下载模型,默认忽略。

use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::core::{test_core, Core};
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::rerank;
use memflow_lib::retrieval::{hybrid_search, SearchOptions};
use serde_json::{json, Value};
use std::sync::Arc;

/// 所有文本得到几乎相同的向量:让向量路无法区分,排序完全取决于全文路与重排序
async fn start_mock() -> String {
    async fn embed(Json(body): Json<Value>) -> Json<Value> {
        let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
        Json(json!({"embeddings": vec![vec![0.5f32; 16]; n]}))
    }
    let app = Router::new()
        .route(
            "/api/version",
            get(|| async { Json(json!({"version": "mock"})) }),
        )
        .route(
            "/api/tags",
            get(|| async { Json(json!({"models": [{"name": "bge-m3:latest", "size": 1}]})) }),
        )
        .route("/api/embed", post(embed))
        // 元数据提取返回非法 JSON,入库会降级为"仅原文 + 向量",不影响本测试
        .route(
            "/api/chat",
            post(|| async {
                Json(json!({"message": {"role": "assistant", "content": "不是JSON"}, "done": true}))
            }),
        )
        .route(
            "/api/generate",
            post(|| async { Json(json!({"done": true})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn setup() -> (Arc<Core>, tempfile::TempDir) {
    let url = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    let mut s = core.settings();
    s.score_threshold = 0.1;
    core.update_settings(s).unwrap();
    for t in [
        "团建活动安排在下个月,地点是郊外的度假村,预算每人三百元。",
        "差旅费用报销需要在出差结束后七天内提交发票,逾期不予受理。",
        "新员工入职需要准备身份证复印件与学历证明,办理工牌。",
    ] {
        ingest::capture(
            &core,
            CaptureInput {
                text: Some(t.into()),
                paths: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    queue::drain_once(&core).await.unwrap();
    (core, dir)
}

fn opts() -> SearchOptions {
    SearchOptions {
        top_n: 3,
        threshold: 0.1,
        kb_ids: None,
    }
}

#[tokio::test]
async fn 未安装重排序模型时检索照常工作() {
    let (core, _d) = setup().await;
    assert!(core.settings().rerank_enabled);
    assert!(!rerank::is_installed(
        &core.paths.root,
        &core.settings().rerank_model
    ));
    let hits = hybrid_search(&core, "报销期限", &opts()).await.unwrap();
    assert!(!hits.is_empty());
}

#[tokio::test]
async fn 关闭重排序后即使模型存在也不加载() {
    let (core, _d) = setup().await;
    let mut s = core.settings();
    s.rerank_enabled = false;
    core.update_settings(s.clone()).unwrap();
    let hits = hybrid_search(&core, "报销期限", &opts()).await.unwrap();
    assert!(!hits.is_empty());
    assert!(!core.reranker.is_loaded(&s.rerank_model));
}

/// 真实模型:先运行 `cargo test --lib rerank::tests::真实模型 -- --ignored` 下载到临时目录,
/// 再 `cargo test --test e2e_rerank -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn 真实模型把最相关的片段排到第一() {
    let (core, _d) = setup().await;
    let id = core.settings().rerank_model;
    let src = rerank::model_dir(&std::env::temp_dir().join("memflow_rerank_real"), &id);
    let dst = rerank::model_dir(&core.paths.root, &id);
    std::fs::create_dir_all(&dst).unwrap();
    for e in std::fs::read_dir(&src).expect("请先下载模型") {
        let e = e.unwrap();
        std::fs::copy(e.path(), dst.join(e.file_name())).unwrap();
    }
    assert!(rerank::is_installed(&core.paths.root, &id));
    let hits = hybrid_search(&core, "出差报销要在多久之内提交", &opts())
        .await
        .unwrap();
    println!(
        "{:?}",
        hits.iter()
            .map(|h| (&h.text[..12], h.score))
            .collect::<Vec<_>>()
    );
    assert!(hits[0].text.contains("差旅费用报销"), "{:?}", hits[0].text);
    assert!(core.reranker.is_loaded(&id));
}
