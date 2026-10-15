// @author ixxxxoooo
//! 集成测试:对话中自由切换模型(本地 Ollama / 默认云端 / 额外云端档案),
//! 并校验回答携带模型名、耗时、首字耗时,且会话记住所选模型。

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::chat::{self, ChatEvent};
use memflow_lib::chat_models;
use memflow_lib::core::{test_core, Core};
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::repo;
use memflow_lib::settings::LlmProfile;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// 记录每个后端实际收到的模型名
#[derive(Default)]
struct Seen {
    ollama_models: Mutex<Vec<String>>,
    cloud_models: Mutex<Vec<String>>,
    cloud_auth: Mutex<Vec<String>>,
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

async fn ollama_chat(State(st): State<Arc<Seen>>, Json(body): Json<Value>) -> Response {
    st.ollama_models
        .lock()
        .unwrap()
        .push(body["model"].as_str().unwrap_or("").to_string());
    if body.get("format").is_some() {
        // 元数据提取返回非法 JSON,入库降级即可
        return Json(
            json!({"message": {"role": "assistant", "content": "不是JSON"}, "done": true}),
        )
        .into_response();
    }
    if body["stream"].as_bool().unwrap_or(false) {
        let mut out = String::new();
        for p in ["本地模型回答", "[1]"] {
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
    Json(json!({"message": {"role": "assistant", "content": "老王说预算怎么样"}, "done": true}))
        .into_response()
}

async fn cloud_chat(
    State(st): State<Arc<Seen>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    st.cloud_models
        .lock()
        .unwrap()
        .push(body["model"].as_str().unwrap_or("").to_string());
    st.cloud_auth.lock().unwrap().push(
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
    );
    if body["stream"].as_bool().unwrap_or(false) {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let mut out = String::new();
        for p in [format!("云端{model}回答"), "[1]".to_string()] {
            out.push_str(&format!(
                "data: {}\n\n",
                json!({"choices": [{"delta": {"content": p}}]})
            ));
        }
        out.push_str("data: [DONE]\n\n");
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(out.into())
            .unwrap();
    }
    Json(json!({"choices": [{"message": {"role": "assistant", "content": "老王说预算怎么样"}}]}))
        .into_response()
}

async fn start_mock() -> (String, Arc<Seen>) {
    let st = Arc::new(Seen::default());
    let app = Router::new()
        .route(
            "/api/version",
            get(|| async { Json(json!({"version": "mock"})) }),
        )
        .route(
            "/api/tags",
            get(|| async {
                Json(json!({"models": [
                    {"name": "bge-m3:latest", "size": 1},
                    {"name": "qwen2.5:7b", "size": 2},
                    {"name": "llama3.2:3b", "size": 3},
                    {"name": "minicpm-v:latest", "size": 4},
                    {"name": "nomic-embed-text:latest", "size": 5}
                ]}))
            }),
        )
        .route(
            "/api/embed",
            post(|Json(b): Json<Value>| async move {
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
            post(|| async { Json(json!({"done": true})) }),
        )
        .route("/api/chat", post(ollama_chat))
        .route("/v1/chat/completions", post(cloud_chat))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

async fn setup(url: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.score_threshold = 0.12;
    s.llm_base_url = format!("{url}/v1");
    s.llm_model = "deepseek-chat".into();
    s.llm_preset = "deepseek".into();
    s.llm_profiles = vec![LlmProfile {
        id: "kimi1".into(),
        name: "Kimi".into(),
        preset: "moonshot".into(),
        base_url: format!("{url}/v1"),
        model: "moonshot-v1-8k".into(),
    }];
    core.update_settings(s).unwrap();
    core.set_api_key_memory("sk-default");
    core.set_key_memory("kimi1", "sk-kimi");
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

async fn ask(
    core: &Arc<Core>,
    sid: Option<i64>,
    q: &str,
    model_ref: Option<&str>,
) -> Vec<ChatEvent> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    chat::ask(core, sid, q, model_ref, &move |e| {
        ev.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let v = events.lock().unwrap().clone();
    v
}

fn done_meta(events: &[ChatEvent]) -> repo::AnswerMeta {
    events
        .iter()
        .find_map(|e| match e {
            ChatEvent::Done { meta, .. } => meta.clone(),
            _ => None,
        })
        .expect("应有带元数据的完成事件")
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
async fn 可选模型包含本地与云端并过滤向量视觉模型() {
    let (url, _s) = start_mock().await;
    let (core, _d) = setup(&url).await;
    let list = chat_models::list_chat_models(&core).await;
    let refs: Vec<&str> = list.iter().map(|m| m.ref_id.as_str()).collect();
    assert!(refs.contains(&"ollama:qwen2.5:7b") && refs.contains(&"ollama:llama3.2:3b"));
    assert!(refs.contains(&"cloud:default") && refs.contains(&"cloud:kimi1"));
    assert!(!refs
        .iter()
        .any(|r| r.contains("bge") || r.contains("minicpm") || r.contains("nomic")));
    let kimi = list.iter().find(|m| m.ref_id == "cloud:kimi1").unwrap();
    assert_eq!(kimi.preset.as_deref(), Some("moonshot"));
    assert_eq!(kimi.provider, "openai_compat");
}

#[tokio::test]
async fn 同一会话内在本地与多个云端模型间切换() {
    let (url, seen) = start_mock().await;
    let (core, _d) = setup(&url).await;

    let e1 = ask(&core, None, "老王说预算怎么样", Some("ollama:llama3.2:3b")).await;
    let sid = session_of(&e1);
    let m1 = done_meta(&e1);
    assert_eq!(
        (m1.model.as_str(), m1.provider.as_str()),
        ("llama3.2:3b", "ollama")
    );
    assert!(m1.elapsed_ms >= m1.first_token_ms.unwrap());

    let e2 = ask(&core, Some(sid), "老王说预算怎么样", Some("cloud:kimi1")).await;
    let m2 = done_meta(&e2);
    assert_eq!(
        (m2.model.as_str(), m2.provider.as_str()),
        ("moonshot-v1-8k", "openai_compat")
    );
    assert_eq!(m2.preset.as_deref(), Some("moonshot"));

    let e3 = ask(&core, Some(sid), "老王说预算怎么样", Some("cloud:default")).await;
    assert_eq!(done_meta(&e3).model, "deepseek-chat");

    // 各后端收到的模型名与所选一致,密钥按档案分别携带
    assert!(seen
        .ollama_models
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == "llama3.2:3b"));
    let cm = seen.cloud_models.lock().unwrap().clone();
    assert!(
        cm.contains(&"moonshot-v1-8k".to_string()) && cm.contains(&"deepseek-chat".to_string())
    );
    let auth = seen.cloud_auth.lock().unwrap().clone();
    assert!(
        auth.contains(&"Bearer sk-kimi".to_string())
            && auth.contains(&"Bearer sk-default".to_string())
    );

    // 落库:每条回答记住自己的模型与耗时,会话记住最近一次选择
    let (msgs, sessions) = core
        .db
        .with(|c| Ok((repo::list_messages(c, sid)?, repo::list_sessions(c)?)))
        .unwrap();
    let answers: Vec<_> = msgs.iter().filter(|m| m.role == "assistant").collect();
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0].model.as_deref(), Some("llama3.2:3b"));
    assert_eq!(answers[1].model.as_deref(), Some("moonshot-v1-8k"));
    assert_eq!(answers[2].provider.as_deref(), Some("openai_compat"));
    assert!(answers
        .iter()
        .all(|m| m.elapsed_ms.is_some() && m.first_token_ms.is_some()));
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.id == sid)
            .unwrap()
            .model_ref
            .as_deref(),
        Some("cloud:default")
    );
}

#[tokio::test]
async fn 云端模型缺少密钥时报错且不写入会话() {
    let (url, _s) = start_mock().await;
    let (core, _d) = setup(&url).await;
    core.set_key_memory("kimi1", "");
    let r = chat::ask(
        &core,
        None,
        "老王说预算怎么样",
        Some("cloud:kimi1"),
        &|_| {},
    )
    .await;
    assert!(r.is_err());
    assert!(core.db.with(repo::list_sessions).unwrap().is_empty());
    // 没有密钥的档案不会出现在可选列表里
    let list = chat_models::list_chat_models(&core).await;
    assert!(!list.iter().any(|m| m.ref_id == "cloud:kimi1"));
}

#[tokio::test]
async fn 非法模型引用被拒绝() {
    let (url, _s) = start_mock().await;
    let (core, _d) = setup(&url).await;
    for bad in ["foo:bar", "cloud:不存在", "ollama:"] {
        let r = chat::ask(&core, None, "老王说预算怎么样", Some(bad), &|_| {}).await;
        assert!(r.is_err(), "{bad} 应被拒绝");
    }
}

#[tokio::test]
async fn 未指定模型时使用设置里的默认模型() {
    let (url, seen) = start_mock().await;
    let (core, _d) = setup(&url).await;
    let e = ask(&core, None, "老王说预算怎么样", None).await;
    assert_eq!(done_meta(&e).model, "qwen2.5:7b");
    assert!(seen
        .ollama_models
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == "qwen2.5:7b"));
}
