// @author ixxxxoooo
//! 集成测试:首字延迟优化——阶段事件、查询向量缓存、改写超时降级、模型预热。

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::chat::{self, ChatEvent};
use memflow_lib::chat_models;
use memflow_lib::core::{test_core, Core};
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Probe {
    /// 向量化请求次数
    embeds: AtomicUsize,
    /// 查询改写(非流式、非 JSON 格式)请求的人工延迟毫秒数
    rewrite_delay_ms: AtomicUsize,
    /// 预热请求(/api/generate)里的模型名
    preloaded: Mutex<Vec<String>>,
}

fn fake_embed(text: &str) -> Vec<f32> {
    let chars: Vec<char> = text.chars().collect();
    let mut v = vec![0f32; 512];
    for w in chars.windows(2) {
        let h = (w[0] as u32).wrapping_mul(31).wrapping_add(w[1] as u32) % 512;
        v[h as usize] += 1.0;
    }
    v
}

async fn chat_handler(State(p): State<Arc<Probe>>, Json(body): Json<Value>) -> Response {
    if body.get("format").is_some() {
        return Json(
            json!({"message": {"role": "assistant", "content": "不是JSON"}, "done": true}),
        )
        .into_response();
    }
    if body["stream"].as_bool().unwrap_or(false) {
        let mut out = String::new();
        for t in ["回答", "[1]"] {
            out.push_str(
                &json!({"message": {"role": "assistant", "content": t}, "done": false}).to_string(),
            );
            out.push('\n');
        }
        out.push_str(
            &json!({"message": {"role": "assistant", "content": ""}, "done": true}).to_string(),
        );
        out.push('\n');
        return Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(out.into())
            .unwrap();
    }
    // 其余非流式请求即查询改写,按需人为变慢
    let d = p.rewrite_delay_ms.load(Ordering::SeqCst) as u64;
    tokio::time::sleep(Duration::from_millis(d)).await;
    Json(json!({"message": {"role": "assistant", "content": "老王说下个月预算怎么样"}, "done": true}))
        .into_response()
}

async fn start_mock() -> (String, Arc<Probe>) {
    let probe = Arc::new(Probe::default());
    let app = Router::new()
        .route(
            "/api/version",
            get(|| async { Json(json!({"version": "mock"})) }),
        )
        .route(
            "/api/tags",
            get(|| async { Json(json!({"models": [{"name": "bge-m3:latest", "size": 1}, {"name": "qwen2.5:7b", "size": 2}]})) }),
        )
        .route(
            "/api/embed",
            post(|State(p): State<Arc<Probe>>, Json(b): Json<Value>| async move {
                p.embeds.fetch_add(1, Ordering::SeqCst);
                let v: Vec<Vec<f32>> = b["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| fake_embed(t.as_str().unwrap()))
                    .collect();
                Json(json!({ "embeddings": v }))
            }),
        )
        .route(
            "/api/generate",
            post(|State(p): State<Arc<Probe>>, Json(b): Json<Value>| async move {
                if b.get("prompt").is_none() && b["keep_alive"] != json!(0) {
                    p.preloaded
                        .lock()
                        .unwrap()
                        .push(b["model"].as_str().unwrap_or("").to_string());
                }
                Json(json!({"done": true}))
            }),
        )
        .route("/api/chat", post(chat_handler))
        .with_state(probe.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), probe)
}

async fn setup(url: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.score_threshold = 0.12;
    core.update_settings(s).unwrap();
    ingest::capture(
        &core,
        CaptureInput {
            text: Some("今天和老王聊了A项目,他说下个月预算会缩减20%".into()),
            paths: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    queue::drain_once(&core).await.unwrap();
    (core, dir)
}

async fn ask(core: &Arc<Core>, sid: Option<i64>, q: &str) -> Vec<ChatEvent> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    chat::ask(core, sid, q, Some("ollama:qwen2.5:7b"), &move |e| {
        ev.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let v = events.lock().unwrap().clone();
    v
}

fn stages(events: &[ChatEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::Stage { stage } => Some(stage.clone()),
            _ => None,
        })
        .collect()
}

fn session_of(events: &[ChatEvent]) -> i64 {
    events
        .iter()
        .find_map(|e| match e {
            ChatEvent::Session { session_id } => Some(*session_id),
            _ => None,
        })
        .unwrap()
}

#[tokio::test]
async fn 自包含问题依次经历检索与生成阶段且不改写() {
    let (url, _p) = start_mock().await;
    let (core, _d) = setup(&url).await;
    let ev = ask(&core, None, "老王说下个月预算会怎么样").await;
    let st = stages(&ev);
    assert_eq!(st.first().map(String::as_str), Some("retrieve"));
    assert_eq!(st.last().map(String::as_str), Some("generate"));
    assert!(!st.contains(&"rewrite".to_string()));
}

#[tokio::test]
async fn 相同问题第二次提问不再请求向量化() {
    let (url, probe) = start_mock().await;
    let (core, _d) = setup(&url).await;
    ask(&core, None, "老王说下个月预算会怎么样").await;
    let after_first = probe.embeds.load(Ordering::SeqCst);
    ask(&core, None, "老王说下个月预算会怎么样").await;
    assert_eq!(
        probe.embeds.load(Ordering::SeqCst),
        after_first,
        "第二次应命中查询向量缓存"
    );
}

#[tokio::test]
async fn 追问改写超时后用原问题结果且不被拖慢() {
    let (url, probe) = start_mock().await;
    let (core, _d) = setup(&url).await;
    let first = ask(&core, None, "老王说下个月预算会怎么样").await;
    let sid = session_of(&first);

    // 改写要 5 秒,远超 1.2 秒上限
    probe.rewrite_delay_ms.store(5000, Ordering::SeqCst);
    let t = Instant::now();
    // 含指代词的追问会触发改写;用与记录强相关的措辞,保证原问题也能检索到
    let ev = ask(&core, Some(sid), "老王说下个月预算怎么样,它会缩减多少").await;
    let elapsed = t.elapsed();
    assert!(
        elapsed < Duration::from_millis(3500),
        "改写超时应被截断,实际 {elapsed:?}"
    );
    assert!(stages(&ev).contains(&"rewrite".to_string()));
    assert!(ev
        .iter()
        .any(|e| matches!(e, ChatEvent::Done { meta: Some(_), .. })));
}

#[tokio::test]
async fn 预热会载入向量模型与所选本地模型() {
    let (url, probe) = start_mock().await;
    let (core, _d) = setup(&url).await;
    chat_models::warm(&core, Some("ollama:qwen2.5:7b")).await;
    let loaded = probe.preloaded.lock().unwrap().clone();
    assert!(loaded.contains(&"qwen2.5:7b".to_string()));
    assert!(loaded.contains(&core.settings().embed_model));
}

#[tokio::test]
async fn 预热云端模型不触碰本地对话模型() {
    let (url, probe) = start_mock().await;
    let (core, _d) = setup(&url).await;
    chat_models::warm(&core, Some("cloud:default")).await;
    let loaded = probe.preloaded.lock().unwrap().clone();
    assert!(!loaded.contains(&"qwen2.5:7b".to_string()));
}
