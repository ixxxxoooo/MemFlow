// @author ixxxxoooo
//! 集成测试:PDF 入库(扫描件 OCR / 文本层 / 视觉模型模式)与页码(仅 macOS)。

#![cfg(target_os = "macos")]

use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::core::{test_core, Core};
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::repo;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 启动只提供向量化与视觉对话的 mock Ollama
async fn start_mock() -> String {
    async fn embed(Json(body): Json<Value>) -> Json<Value> {
        let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
        Json(json!({"embeddings": vec![vec![0.5f32; 16]; n]}))
    }
    async fn tags() -> Json<Value> {
        Json(json!({"models": [
            {"name": "bge-m3:latest", "size": 1},
            {"name": "minicpm-v:latest", "size": 2}
        ]}))
    }
    async fn chat() -> Json<Value> {
        // 模拟视觉模型把整页转成 Markdown(带代码围栏,验证会被剥掉)
        Json(json!({"message": {"role": "assistant",
            "content": "```markdown\n# 视觉识别标题\n\n| 列甲 | 列乙 |\n| --- | --- |\n| 苹果 | 香蕉 |\n```"},
            "done": true}))
    }
    let app = Router::new()
        .route(
            "/api/version",
            get(|| async { Json(json!({"version": "mock"})) }),
        )
        .route("/api/tags", get(tags))
        .route("/api/embed", post(embed))
        .route("/api/chat", post(chat))
        .route(
            "/api/generate",
            post(|| async { Json(json!({"done": true})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// 导入一个 PDF 并跑完入库,返回 (记录, 切片)
async fn ingest_pdf(
    core: &Arc<Core>,
    name: &str,
) -> (
    memflow_lib::repo::ItemView,
    Vec<memflow_lib::repo::ChunkView>,
) {
    // 让 OCR 走与正式应用一致的"子进程"路径(测试程序自身不支持 --ocr-worker)
    std::env::set_var("MEMFLOW_OCR_EXE", env!("CARGO_BIN_EXE_memflow"));
    let r = ingest::capture(
        core,
        CaptureInput {
            text: None,
            paths: vec![fixture(name).to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    queue::drain_once(core).await.unwrap();
    let id = r.created[0];
    core.db
        .with(|c| Ok((repo::get_item(c, id)?.unwrap(), repo::list_chunks(c, id)?)))
        .unwrap()
}

#[tokio::test]
async fn 扫描版pdf经ocr入库并带页码() {
    let url = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    let (item, chunks) = ingest_pdf(&core, "scan_cn.pdf").await;
    assert_eq!(item.status, "done", "错误: {:?}", item.error);
    assert!(!chunks.is_empty());
    let all: String = chunks.iter().map(|c| c.text.as_str()).collect();
    assert!(all.contains("预算") && all.contains("星河科技"), "{all}");
    assert!(chunks.iter().all(|c| c.page == Some(1)), "{chunks:?}");
    assert!(chunks.iter().all(|c| !c.text.contains('\u{E000}')));
}

#[tokio::test]
async fn 表格pdf入库后表格保持markdown() {
    let url = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    let (item, chunks) = ingest_pdf(&core, "scan_table.pdf").await;
    assert_eq!(item.status, "done", "错误: {:?}", item.error);
    let table = chunks
        .iter()
        .find(|c| c.text.contains("| 差旅 |"))
        .expect("应有表格切片");
    assert!(
        table.text.contains("| 项目 | 金额 | 负责人 |"),
        "{}",
        table.text
    );
}

#[tokio::test]
async fn 文本层pdf直接读取() {
    let url = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    let (item, chunks) = ingest_pdf(&core, "text_en.pdf").await;
    assert_eq!(item.status, "done", "错误: {:?}", item.error);
    let all: String = chunks.iter().map(|c| c.text.as_str()).collect();
    assert!(all.contains("Quarterly Review"), "{all}");
}

#[tokio::test]
async fn 视觉模型模式逐页转markdown() {
    let url = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    let mut s = core.settings();
    s.pdf_parse_mode = "vision".into();
    core.update_settings(s).unwrap();
    let (item, chunks) = ingest_pdf(&core, "scan_cn.pdf").await;
    assert_eq!(item.status, "done", "错误: {:?}", item.error);
    let all: String = chunks
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        chunks
            .iter()
            .any(|c| c.heading.as_deref() == Some("视觉识别标题")),
        "标题应进入标题路径: {chunks:?}"
    );
    assert!(all.contains("| 苹果 | 香蕉 |"), "{all}");
    assert!(!all.contains("```"), "代码围栏应被剥掉: {all}");
}

#[tokio::test]
async fn 视觉模型未安装时给出明确错误() {
    let url = start_mock().await;
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), &url);
    let mut s = core.settings();
    s.pdf_parse_mode = "vision".into();
    s.vision_model = "not-installed-vl".into();
    core.update_settings(s).unwrap();
    let (item, chunks) = ingest_pdf(&core, "scan_cn.pdf").await;
    assert!(chunks.is_empty());
    assert_ne!(item.status, "done");
}
