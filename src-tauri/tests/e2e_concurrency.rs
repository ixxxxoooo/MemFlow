// @author ygw
//! 集成测试:多任务并发入库
//! 验证并发度受设置控制、各阶段信号量生效、结果完整无重复,以及崩溃恢复后并发续跑。

use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::core::{test_core, Core, EMBED_CONCURRENCY};
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::repo;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 记录某类请求的当前并发数与历史峰值
#[derive(Default)]
struct Gauge {
    now: AtomicUsize,
    peak: AtomicUsize,
}

impl Gauge {
    fn enter(&self) {
        let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(n, Ordering::SeqCst);
    }
    fn leave(&self) {
        self.now.fetch_sub(1, Ordering::SeqCst);
    }
    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Mock {
    embed: Gauge,
    chat: Gauge,
    embed_calls: AtomicUsize,
}

fn fake_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; 64];
    for (i, c) in text.chars().enumerate() {
        v[(c as usize + i) % 64] += 1.0;
    }
    v
}

async fn embed(State(m): State<Arc<Mock>>, Json(body): Json<Value>) -> Json<Value> {
    m.embed.enter();
    m.embed_calls.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(120)).await;
    let inputs: Vec<String> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    m.embed.leave();
    Json(json!({"embeddings": inputs.iter().map(|t| fake_embed(t)).collect::<Vec<_>>()}))
}

async fn chat(State(m): State<Arc<Mock>>, Json(_body): Json<Value>) -> Response {
    m.chat.enter();
    tokio::time::sleep(Duration::from_millis(40)).await;
    m.chat.leave();
    let content = json!({"summary": "摘要", "category": "测试", "tags": ["标签"], "entities": []})
        .to_string();
    let body = json!({"message": {"role": "assistant", "content": content}, "done": true});
    Response::builder()
        .header("content-type", "application/json")
        .body(body.to_string().into())
        .unwrap()
}

async fn start_mock() -> (String, Arc<Mock>) {
    let m = Arc::new(Mock::default());
    let app = Router::new()
        .route(
            "/api/version",
            get(|| async { Json(json!({"version": "mock"})) }),
        )
        .route(
            "/api/tags",
            get(|| async {
                Json(json!({"models": [{"name": "bge-m3:latest"}, {"name": "qwen2.5:7b"}]}))
            }),
        )
        .route("/api/embed", post(embed))
        .route("/api/chat", post(chat))
        .route(
            "/api/generate",
            post(|| async { Json(json!({"done": true})) }),
        )
        .with_state(m.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), m)
}

fn new_core(url: &str, concurrency: u32) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.import_concurrency = concurrency;
    core.update_settings(s).unwrap();
    (core, dir)
}

async fn put_many(core: &Arc<Core>, n: usize) -> Vec<i64> {
    let mut ids = vec![];
    for i in 0..n {
        let r = ingest::capture(
            core,
            CaptureInput {
                text: Some(format!("并发测试记录第{i}条:今天讨论了方案{i}与预算{i}")),
                paths: vec![],
            },
        )
        .await
        .unwrap();
        ids.push(r.created[0]);
    }
    ids
}

fn counts(core: &Arc<Core>) -> (i64, i64, i64) {
    core.db
        .with(|c| {
            Ok((
                c.query_row("SELECT count(*) FROM items WHERE status='done'", [], |r| {
                    r.get(0)
                })?,
                c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?,
                c.query_row("SELECT count(*) FROM chunks_fts", [], |r| r.get(0))?,
            ))
        })
        .unwrap()
}

#[tokio::test]
async fn 并发入库_全部完成且无重复且受信号量限制() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url, 4);
    let ids = put_many(&core, 12).await;

    let started = std::time::Instant::now();
    let n = queue::drain_once(&core).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(n, 12);
    let (done, chunks, fts) = counts(&core);
    assert_eq!(
        (done, chunks, fts),
        (12, 12, 12),
        "每条记录恰好一个切片,无重复"
    );
    assert_eq!(core.vindex.len(), 12);
    for id in ids {
        let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
        assert_eq!(it.status, "done");
    }
    // 向量化阶段确实并行,且不超过全局信号量上限
    assert!(
        mock.embed.peak() >= 2,
        "向量化应并行,实际峰值 {}",
        mock.embed.peak()
    );
    assert!(
        mock.embed.peak() <= EMBED_CONCURRENCY,
        "不得超过向量化并发上限"
    );
    // 本地文本模型调用必须串行
    assert_eq!(mock.chat.peak(), 1, "本地文本模型应串行");
    // 串行下限约 12 * (120ms 向量化 + 40ms 提取) ≈ 1.9s,并发应明显更快
    assert!(
        elapsed < Duration::from_millis(1700),
        "并发应快于串行,实际 {elapsed:?}"
    );
}

#[tokio::test]
async fn 并发度为1时严格串行() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url, 1);
    put_many(&core, 5).await;
    assert_eq!(queue::drain_once(&core).await.unwrap(), 5);
    assert_eq!(counts(&core).0, 5);
    assert_eq!(mock.embed.peak(), 1, "并发度 1 时向量化不应并行");
}

#[tokio::test]
async fn 崩溃恢复后并发续跑() {
    let (url, _mock) = start_mock().await;
    let (core, _d) = new_core(&url, 3);
    put_many(&core, 6).await;
    // 模拟崩溃:三个任务停在 running,对应记录停在 processing
    core.db
        .with(|c| {
            c.execute(
                "UPDATE jobs SET status='running' WHERE id IN (SELECT id FROM jobs ORDER BY id LIMIT 3)",
                [],
            )?;
            c.execute(
                "UPDATE items SET status='processing' WHERE id IN (SELECT item_id FROM jobs WHERE status='running')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    queue::recover_after_crash(&core.db).unwrap();
    assert_eq!(queue::drain_once(&core).await.unwrap(), 6);
    assert_eq!(counts(&core), (6, 6, 6));
}

#[tokio::test]
async fn 并发处理中取消指定记录() {
    let (url, _mock) = start_mock().await;
    let (core, _d) = new_core(&url, 3);
    let ids = put_many(&core, 6).await;
    let victim = ids[5];
    // 排队中的任务被取消后不应被执行
    queue::cancel_item(&core, victim).unwrap();
    assert_eq!(queue::drain_once(&core).await.unwrap(), 5);
    let it = core
        .db
        .with(|c| repo::get_item(c, victim))
        .unwrap()
        .unwrap();
    assert_eq!(it.status, "failed");
    assert_eq!(counts(&core).0, 5);
}
