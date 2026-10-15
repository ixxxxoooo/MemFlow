// @author ygw
//! 集成测试公共设施:本地 mock Ollama(嵌入用字符二元组哈希,共享词越多越相似)。
#![allow(dead_code)]

use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::chat::{self, ChatEvent};
use memflow_lib::core::{test_core, Core};
use memflow_lib::ingest::{self, CaptureInput};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// mock 服务状态
#[derive(Default)]
pub struct MockState {
    /// 最近一次问答请求的 system 提示词
    pub last_system: Mutex<String>,
    /// 视觉模型调用次数
    pub vision_calls: std::sync::atomic::AtomicUsize,
}

/// 字符二元组哈希到 512 维
pub fn fake_embed(text: &str) -> Vec<f32> {
    let chars: Vec<char> = text.chars().collect();
    let mut v = vec![0f32; 512];
    for w in chars.windows(2) {
        let h = (w[0] as u32).wrapping_mul(31).wrapping_add(w[1] as u32) % 512;
        v[h as usize] += 1.0;
    }
    if chars.len() == 1 {
        v[(chars[0] as u32 % 512) as usize] += 1.0;
    }
    v
}

async fn version() -> Json<Value> {
    Json(json!({"version": "mock"}))
}

async fn tags() -> Json<Value> {
    Json(
        json!({"models": [{"name": "bge-m3:latest", "size": 1}, {"name": "qwen2.5:7b", "size": 2}]}),
    )
}

async fn embed(Json(body): Json<Value>) -> Json<Value> {
    let inputs: Vec<String> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    Json(json!({"embeddings": inputs.iter().map(|t| fake_embed(t)).collect::<Vec<_>>()}))
}

async fn chat_handler(State(st): State<Arc<MockState>>, Json(body): Json<Value>) -> Response {
    let stream = body["stream"].as_bool().unwrap_or(false);
    let messages = body["messages"].as_array().unwrap();
    let user = messages.last().unwrap()["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let json_resp = |v: Value| {
        Response::builder()
            .header("content-type", "application/json")
            .body(v.to_string().into())
            .unwrap()
    };
    if messages.last().unwrap().get("images").is_some() {
        st.vision_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        return json_resp(
            json!({"message": {"role": "assistant", "content": "画面描述:一张示意图"}, "done": true}),
        );
    }
    if body.get("format").is_some() {
        let text: String = user
            .split("内容:")
            .last()
            .unwrap_or("")
            .trim()
            .chars()
            .take(12)
            .collect();
        let content = json!({"summary": format!("摘要:{text}"), "category": "测试", "tags": ["标签甲"], "entities": []}).to_string();
        return json_resp(
            json!({"message": {"role": "assistant", "content": content}, "done": true}),
        );
    }
    if stream {
        *st.last_system.lock().unwrap() = messages[0]["content"].as_str().unwrap_or("").to_string();
        let mut out = String::new();
        for p in ["根据资料", "[1]。"] {
            out.push_str(
                &json!({"message": {"role": "assistant", "content": p}, "done": false}).to_string(),
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
    json_resp(json!({"message": {"role": "assistant", "content": "ok"}, "done": true}))
}

async fn generate() -> Json<Value> {
    Json(json!({"done": true}))
}

/// 启动 mock 服务
pub async fn start_mock() -> (String, Arc<MockState>) {
    let st = Arc::new(MockState::default());
    let app = Router::new()
        .route("/api/version", get(version))
        .route("/api/tags", get(tags))
        .route("/api/embed", post(embed))
        .route("/api/chat", post(chat_handler))
        .route("/api/generate", post(generate))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

/// 创建指向 mock 的核心(相关性阈值调低以适配哈希嵌入)
pub fn new_core(url: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.score_threshold = 0.12;
    core.update_settings(s).unwrap();
    (core, dir)
}

/// 向指定知识库录入一段文字,返回记录 id
pub async fn put_in(core: &Arc<Core>, text: &str, kb_id: i64) -> i64 {
    ingest::capture(
        core,
        CaptureInput {
            text: Some(text.into()),
            kb_id: Some(kb_id),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .created[0]
}

/// 在指定知识库范围提问,返回全部事件
pub async fn ask_in(
    core: &Arc<Core>,
    sid: Option<i64>,
    q: &str,
    kbs: Option<&[i64]>,
) -> Vec<ChatEvent> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    chat::ask_in(core, sid, q, None, kbs, &move |e| {
        ev.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let v = events.lock().unwrap().clone();
    v
}

/// 取事件里的来源列表
pub fn sources_of(events: &[ChatEvent]) -> Vec<chat::SourceView> {
    events
        .iter()
        .find_map(|e| match e {
            ChatEvent::Sources { sources } => Some(sources.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// 取最终回答文本
pub fn answer_of(events: &[ChatEvent]) -> String {
    events
        .iter()
        .find_map(|e| match e {
            ChatEvent::Done { answer, .. } => Some(answer.clone()),
            _ => None,
        })
        .unwrap_or_default()
}
