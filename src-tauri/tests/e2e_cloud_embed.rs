// @author ygw
//! 集成测试:向量模型与视觉模型选择云端(OpenAI 兼容 `/embeddings` 与多模态对话)。
//! 用本地 mock 服务模拟:Ollama(文本提取)+ 云端向量接口 + 云端多模态对话接口。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::core::{test_core, Core};
use memflow_lib::error::AppError;
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::repo;
use memflow_lib::retrieval::{self, SearchOptions};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 1×1 PNG,用于模拟一张图片
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x64, 0x60, 0x60, 0x60,
    0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0x5e, 0xf3, 0x2a, 0x3a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// mock 服务的可控状态
#[derive(Default)]
struct MockState {
    /// 本地 /api/embed 调用次数(选择云端向量后不应被调用)
    ollama_embed_calls: AtomicUsize,
    /// 本地 /api/chat 的视觉请求次数(选择云端视觉后不应带图)
    ollama_vision_calls: AtomicUsize,
    /// 云端 /embeddings 调用次数
    cloud_embed_calls: AtomicUsize,
    /// 云端对话调用次数
    cloud_chat_calls: AtomicUsize,
    last_auth: Mutex<String>,
    last_embed_body: Mutex<Option<Value>>,
    last_chat_body: Mutex<Option<Value>>,
}

/// 字符二元组哈希向量(512 维)
fn fake_embed(text: &str) -> Vec<f32> {
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

async fn ollama_embed(State(st): State<Arc<MockState>>, Json(body): Json<Value>) -> Json<Value> {
    st.ollama_embed_calls.fetch_add(1, Ordering::SeqCst);
    let inputs: Vec<String> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    Json(json!({"embeddings": inputs.iter().map(|t| fake_embed(t)).collect::<Vec<_>>()}))
}

/// 本地文本提取(结构化)
async fn ollama_chat(State(st): State<Arc<MockState>>, Json(body): Json<Value>) -> Json<Value> {
    let has_image = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m.get("images").is_some());
    if has_image {
        st.ollama_vision_calls.fetch_add(1, Ordering::SeqCst);
    }
    let content = json!({"summary": "摘要", "category": "测试", "tags": ["标签"], "entities": []})
        .to_string();
    Json(json!({"message": {"role": "assistant", "content": content}, "done": true}))
}

/// 云端 /v1/embeddings:故意把返回顺序倒过来,验证按 index 对齐
async fn cloud_embeddings(
    State(st): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    st.cloud_embed_calls.fetch_add(1, Ordering::SeqCst);
    *st.last_auth.lock().unwrap() = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    *st.last_embed_body.lock().unwrap() = Some(body.clone());
    let inputs: Vec<String> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    if inputs.len() > 10 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "batch size is invalid, it should not be larger than 10"}})),
        )
            .into_response();
    }
    let mut data: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(i, t)| json!({"index": i, "embedding": fake_embed(t)}))
        .collect();
    data.reverse();
    Json(json!({"data": data})).into_response()
}

/// 云端多模态对话:返回视觉解析 JSON
async fn cloud_chat(State(st): State<Arc<MockState>>, Json(body): Json<Value>) -> Response {
    st.cloud_chat_calls.fetch_add(1, Ordering::SeqCst);
    *st.last_chat_body.lock().unwrap() = Some(body);
    let content = json!({
        "description": "一张白板照片",
        "ocr_text": "采购预算",
        "intent": "记录会议",
        "tags": ["白板"],
        "category": "会议"
    })
    .to_string();
    Json(json!({"choices": [{"message": {"role": "assistant", "content": content}}]}))
        .into_response()
}

async fn start_mock() -> (String, Arc<MockState>) {
    let st = Arc::new(MockState::default());
    let app = Router::new()
        .route("/api/version", get(version))
        .route("/api/tags", get(tags))
        .route("/api/embed", post(ollama_embed))
        .route("/api/chat", post(ollama_chat))
        .route("/v1/embeddings", post(cloud_embeddings))
        .route("/v1/chat/completions", post(cloud_chat))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

/// 创建核心:默认云端配置指向 mock,且已保存密钥
fn new_core(url: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.score_threshold = 0.12;
    s.llm_base_url = format!("{url}/v1");
    core.update_settings(s).unwrap();
    core.set_api_key_memory("sk-cloud-1");
    (core, dir)
}

fn use_cloud_embed(core: &Arc<Core>) {
    let mut s = core.settings();
    s.embed_provider = "openai_compat".into();
    s.embed_cloud_model = "emb-test".into();
    core.update_settings(s).unwrap();
}

async fn put(core: &Arc<Core>, text: &str) -> i64 {
    ingest::capture(
        core,
        CaptureInput {
            text: Some(text.into()),
            paths: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .created[0]
}

#[tokio::test]
async fn 云端向量_入库与检索都走云端且来源标识带模型名() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    use_cloud_embed(&core);
    let id = put(&core, "今天和老王聊了A项目,他说下个月预算会缩减20%").await;
    assert_eq!(queue::drain_once(&core).await.unwrap(), 1);

    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.status, "done");
    assert_eq!(core.vindex.len(), 1);
    let model: String = core
        .db
        .with(|c| Ok(c.query_row("SELECT embed_model FROM chunks LIMIT 1", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(model, "cloud:default:emb-test");
    assert_eq!(mock.ollama_embed_calls.load(Ordering::SeqCst), 0);
    assert_eq!(mock.last_auth.lock().unwrap().as_str(), "Bearer sk-cloud-1");
    let body = mock.last_embed_body.lock().unwrap().clone().unwrap();
    assert_eq!(body["model"], "emb-test");
    assert_eq!(body["encoding_format"], "float");

    // 检索:查询向量同样来自云端,能命中刚入库的记录
    let hits = retrieval::hybrid_search(
        &core,
        "老王说预算怎么样",
        &SearchOptions {
            top_n: 5,
            threshold: 0.12,
            kb_ids: None,
        },
    )
    .await
    .unwrap();
    assert!(hits.iter().any(|h| h.item_id == id), "{hits:?}");
    assert_eq!(mock.ollama_embed_calls.load(Ordering::SeqCst), 0);
    let stats = core
        .db
        .with_read(|c| repo::stats(c, &core.settings().embed_identity()))
        .unwrap();
    assert_eq!(stats.stale_chunks, 0);
}

#[tokio::test]
async fn 云端向量_超过单次上限时自动分批() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    use_cloud_embed(&core);
    let inputs: Vec<String> = (0..25).map(|i| format!("第{i}段测试文本")).collect();
    let v = core
        .embedder()
        .unwrap()
        .embed(&inputs)
        .await
        .expect("应拆成 10/10/5 三批");
    assert_eq!(v.len(), 25);
    assert_eq!(mock.cloud_embed_calls.load(Ordering::SeqCst), 3);
    // 倒序返回也要按 index 对齐:每条向量与其输入一致
    let mut expect = fake_embed(&inputs[0]);
    memflow_lib::vector::normalize(&mut expect);
    let dot: f32 = v[0].iter().zip(&expect).map(|(a, b)| a * b).sum();
    assert!(dot > 0.999, "dot={dot}");
}

#[tokio::test]
async fn 切换向量来源后旧向量被判为过期且索引按新来源重载() {
    let (url, _mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    put(&core, "本地向量化的一条记录").await;
    queue::drain_once(&core).await.unwrap();
    assert_eq!(core.vindex.len(), 1);

    use_cloud_embed(&core);
    // 新来源下没有任何向量,旧切片全部过期(需要重建)
    assert_eq!(core.vindex.len(), 0);
    let stats = core
        .db
        .with_read(|c| repo::stats(c, &core.settings().embed_identity()))
        .unwrap();
    assert_eq!(stats.stale_chunks, 1);

    // 切回本地:旧向量重新可用
    let mut s = core.settings();
    s.embed_provider = "ollama".into();
    core.update_settings(s).unwrap();
    assert_eq!(core.vindex.len(), 1);
}

#[tokio::test]
async fn 云端向量_密钥缺失时不发请求并提示() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    use_cloud_embed(&core);
    core.set_api_key_memory("");
    let r = core.embedder().unwrap().embed(&["hi".to_string()]).await;
    assert!(matches!(r, Err(AppError::LlmUnavailable(_))), "{r:?}");
    assert_eq!(mock.cloud_embed_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn 云端视觉_图片以data_url发送且不触发本地视觉模型() {
    let (url, mock) = start_mock().await;
    let (core, dir) = new_core(&url);
    let mut s = core.settings();
    s.vision_enabled = true;
    s.vision_provider = "openai_compat".into();
    s.vision_cloud_model = "vl-test".into();
    core.update_settings(s).unwrap();
    let png = dir.path().join("白板.png");
    std::fs::write(&png, TINY_PNG).unwrap();
    let r = ingest::capture(
        &core,
        CaptureInput {
            text: None,
            paths: vec![png.to_string_lossy().into_owned()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let id = r.created[0];
    queue::drain_once(&core).await.unwrap();

    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.status, "done", "{:?}", it.error);
    assert_eq!(it.meta_model.as_deref(), Some("vl-test"));
    assert!(it.vision_text.unwrap_or_default().contains("一张白板照片"));
    assert_eq!(mock.cloud_chat_calls.load(Ordering::SeqCst), 1);
    assert_eq!(mock.ollama_vision_calls.load(Ordering::SeqCst), 0);

    let body = mock.last_chat_body.lock().unwrap().clone().unwrap();
    assert_eq!(body["model"], "vl-test");
    let user = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .unwrap();
    let parts = user["content"]
        .as_array()
        .expect("带图消息应为 content 数组");
    let img = parts.iter().find(|p| p["type"] == "image_url").unwrap();
    assert!(img["image_url"]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    assert!(user.get("images").is_none());
}

#[tokio::test]
async fn 测试连接_向量与视觉() {
    let (url, _mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    let c = core.cloud_client(&format!("{url}/v1"), "sk-x");
    let m = c.ping_embed("emb-test").await.unwrap();
    assert!(m.contains("维度 512"), "{m}");
    let m = c.ping_vision("vl-test").await.unwrap();
    assert!(m.contains("视觉模型"), "{m}");
}
