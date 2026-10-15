// @author ygw
//! 集成测试:云端 OpenAI 兼容模型(如 DeepSeek)与模型直连下载。
//! 用本地 mock 服务模拟:Ollama(仅向量化)+ OpenAI 兼容对话接口 + Ollama 模型仓库。

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::chat::{self, ChatEvent};
use memflow_lib::core::{test_core, Core};
use memflow_lib::error::AppError;
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::model_fetch;
use memflow_lib::queue;
use memflow_lib::repo;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// mock 服务的可控状态
#[derive(Default)]
struct MockState {
    /// 对话接口是否返回 401
    unauthorized: AtomicBool,
    /// 云端对话请求次数
    chat_calls: AtomicUsize,
    /// 最近一次结构化请求体
    last_json_body: Mutex<Option<Value>>,
    /// 最近一次收到的 Authorization 头
    last_auth: Mutex<String>,
    /// 本地 Ollama /api/chat 是否被调用(云端模式下不应该被调用)
    ollama_chat_calls: AtomicUsize,
}

/// 字符二元组哈希向量,与 e2e_mock_ollama 保持一致
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
    Json(json!({"models": [{"name": "bge-m3:latest", "size": 1}]}))
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

async fn ollama_chat(State(st): State<Arc<MockState>>) -> Json<Value> {
    st.ollama_chat_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({"message": {"role": "assistant", "content": "不应被调用"}, "done": true}))
}

/// OpenAI 兼容的 /v1/chat/completions
async fn cloud_chat(
    State(st): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    st.chat_calls.fetch_add(1, Ordering::SeqCst);
    *st.last_auth.lock().unwrap() = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if st.unauthorized.load(Ordering::SeqCst) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": "Authentication Fails"}})),
        )
            .into_response();
    }
    let stream = body["stream"].as_bool().unwrap_or(false);
    if body.get("response_format").is_some() {
        *st.last_json_body.lock().unwrap() = Some(body.clone());
        let content =
            json!({"summary": "云端摘要", "category": "测试", "tags": ["云标签"], "entities": []})
                .to_string();
        return Json(json!({"choices": [{"message": {"role": "assistant", "content": content}}]}))
            .into_response();
    }
    if stream {
        // 先输出推理内容(不应出现在答案里),再输出正文,最后 [DONE]
        let mut out = String::new();
        let lines = [
            json!({"choices": [{"delta": {"reasoning_content": "思考中"}}]}),
            json!({"choices": [{"delta": {"content": "根据资料"}}]}),
            json!({"choices": [{"delta": {"content": "[1],预算会缩减。"}}]}),
        ];
        for l in lines {
            out.push_str(&format!("data: {l}\n\n"));
        }
        out.push_str("data: [DONE]\n\n");
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(out.into())
            .unwrap();
    }
    Json(json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]})).into_response()
}

/// 启动 mock 服务,返回根地址与状态
async fn start_mock() -> (String, Arc<MockState>) {
    let st = Arc::new(MockState::default());
    let app = Router::new()
        .route("/api/version", get(version))
        .route("/api/tags", get(tags))
        .route("/api/embed", post(embed))
        .route("/api/chat", post(ollama_chat))
        .route("/v1/chat/completions", post(cloud_chat))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

/// 创建使用云端文本模型的核心
fn cloud_core(url: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.score_threshold = 0.12;
    s.llm_provider = "openai_compat".into();
    s.llm_base_url = format!("{url}/v1");
    s.llm_model = "deepseek-chat".into();
    core.update_settings(s).unwrap();
    core.set_api_key_memory("sk-test-123");
    (core, dir)
}

async fn put(core: &Arc<Core>, text: &str) -> i64 {
    ingest::capture(
        core,
        CaptureInput {
            text: Some(text.into()),
            paths: vec![],
        },
    )
    .await
    .unwrap()
    .created[0]
}

#[tokio::test]
async fn 云端模式_提取走json_object且向量仍走本地() {
    let (url, mock) = start_mock().await;
    let (core, _d) = cloud_core(&url);
    let id = put(&core, "今天和老王聊了A项目,他说下个月预算会缩减20%").await;
    assert_eq!(queue::drain_once(&core).await.unwrap(), 1);

    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.status, "done");
    assert_eq!(it.tags, vec!["云标签"]);
    assert_eq!(it.summary.as_deref(), Some("云端摘要"));
    assert_eq!(it.meta_model.as_deref(), Some("deepseek-chat"));
    assert_eq!(core.vindex.len(), 1, "向量化应走本地 Ollama");

    assert_eq!(
        mock.last_auth.lock().unwrap().as_str(),
        "Bearer sk-test-123"
    );
    let body = mock.last_json_body.lock().unwrap().clone().unwrap();
    assert_eq!(body["response_format"]["type"], "json_object");
    assert_eq!(body["model"], "deepseek-chat");
    // schema 被写进了系统提示词,且包含 json 字样
    let sys = body["messages"][0]["content"].as_str().unwrap();
    assert!(sys.contains("JSON Schema") && sys.contains("json"));
    assert_eq!(
        mock.ollama_chat_calls.load(Ordering::SeqCst),
        0,
        "云端模式不应调用本地 /api/chat"
    );
}

#[tokio::test]
async fn 云端模式_流式问答忽略推理内容() {
    let (url, mock) = start_mock().await;
    let (core, _d) = cloud_core(&url);
    put(&core, "今天和老王聊了A项目,他说下个月预算会缩减20%").await;
    queue::drain_once(&core).await.unwrap();

    let events = Arc::new(Mutex::new(Vec::<ChatEvent>::new()));
    let ev = events.clone();
    chat::ask(&core, None, "老王说预算怎么样", &move |e| {
        ev.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let evs = events.lock().unwrap();
    let answer = evs
        .iter()
        .find_map(|e| match e {
            ChatEvent::Done { answer, .. } => Some(answer.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        answer.contains("预算会缩减") && answer.contains("[1]"),
        "{answer}"
    );
    assert!(!answer.contains("思考中"), "推理内容不应混入答案: {answer}");
    assert_eq!(mock.ollama_chat_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn 云端401时任务保持等待且不消耗重试次数() {
    let (url, mock) = start_mock().await;
    mock.unauthorized.store(true, Ordering::SeqCst);
    let (core, _d) = cloud_core(&url);
    let id = put(&core, "密钥失效时也要能先记下来").await;
    queue::drain_once(&core).await.unwrap();

    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.status, "pending");
    assert_eq!(it.content, "密钥失效时也要能先记下来");
    assert!(it.error.unwrap().contains("API Key"), "应提示密钥问题");
    let attempts: i64 = core
        .db
        .with(|c| Ok(c.query_row("SELECT attempts FROM jobs", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn 未配置密钥时不发请求并提示() {
    let (url, mock) = start_mock().await;
    let (core, _d) = cloud_core(&url);
    core.set_api_key_memory("");
    let r = core
        .llm()
        .chat(
            "deepseek-chat",
            &[memflow_lib::ollama::ChatMessage::new("user", "hi")],
            &core.gen_opts(0.1),
            None,
        )
        .await;
    assert!(matches!(r, Err(AppError::LlmUnavailable(_))));
    assert_eq!(mock.chat_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn 测试连接_成功与鉴权失败() {
    let (url, mock) = start_mock().await;
    let (core, _d) = cloud_core(&url);
    let msg = core
        .cloud_client(&format!("{url}/v1"), "sk-x")
        .ping("deepseek-chat")
        .await
        .unwrap();
    assert!(msg.contains("连接成功"));
    mock.unauthorized.store(true, Ordering::SeqCst);
    let err = core
        .cloud_client(&format!("{url}/v1"), "sk-x")
        .ping("deepseek-chat")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("API Key"), "{err}");
}

// ---------------- 直连下载 ----------------

/// mock 模型仓库:支持 Range 的 blob 与 manifest
struct Registry {
    manifest: String,
    blob_hex: String,
    blob: Vec<u8>,
    /// 实际返回的 blob(用于模拟内容被篡改)
    served: Vec<u8>,
    range_requests: AtomicUsize,
}

async fn reg_manifest(State(r): State<Arc<Registry>>) -> Response {
    Response::builder()
        .header(
            "content-type",
            "application/vnd.docker.distribution.manifest.v2+json",
        )
        .body(r.manifest.clone().into())
        .unwrap()
}

async fn reg_blob(
    State(r): State<Arc<Registry>>,
    Path((_ns, _name, digest)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    assert!(digest.contains(&r.blob_hex));
    if let Some(range) = headers.get("range").and_then(|v| v.to_str().ok()) {
        r.range_requests.fetch_add(1, Ordering::SeqCst);
        let start: usize = range
            .trim_start_matches("bytes=")
            .trim_end_matches('-')
            .parse()
            .unwrap();
        if start >= r.served.len() {
            return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
        }
        return Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .body(r.served[start..].to_vec().into())
            .unwrap();
    }
    Response::builder().body(r.served.clone().into()).unwrap()
}

async fn start_registry(tamper: bool) -> (String, Arc<Registry>) {
    let blob: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let hex = hex::encode(Sha256::digest(&blob));
    let manifest = json!({
        "schemaVersion": 2,
        "config": {"digest": format!("sha256:{hex}"), "size": blob.len()},
        "layers": [{"digest": format!("sha256:{hex}"), "size": blob.len()}],
    })
    .to_string();
    let mut served = blob.clone();
    if tamper {
        served[10] ^= 0xff;
    }
    let reg = Arc::new(Registry {
        manifest,
        blob_hex: hex,
        blob,
        served,
        range_requests: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/v2/{ns}/{name}/manifests/{tag}", get(reg_manifest))
        .route("/v2/{ns}/{name}/blobs/{digest}", get(reg_blob))
        .with_state(reg.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), reg)
}

#[tokio::test]
async fn 直连下载_完整下载并写入manifest() {
    let (url, reg) = start_registry(false).await;
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let last = Arc::new(Mutex::new(None));
    let l2 = last.clone();
    model_fetch::pull_direct(
        &reqwest::Client::new(),
        &url,
        dir.path(),
        "tiny:q4",
        &cancel,
        move |p| *l2.lock().unwrap() = Some(p),
    )
    .await
    .unwrap();

    let blob_path = dir
        .path()
        .join("blobs")
        .join(format!("sha256-{}", reg.blob_hex));
    assert_eq!(std::fs::read(&blob_path).unwrap(), reg.blob);
    let mf = dir
        .path()
        .join("manifests/registry.ollama.ai/library/tiny/q4");
    assert_eq!(std::fs::read_to_string(mf).unwrap(), reg.manifest);
    let p = last.lock().unwrap().clone().unwrap();
    assert_eq!(p.status, "success");
    assert_eq!(p.completed, p.total);
    // config 与 layer 是同一个 blob:第二次应直接跳过,不会重复下载
    assert!(!dir
        .path()
        .join("blobs")
        .join(format!("sha256-{}.part", reg.blob_hex))
        .exists());
}

#[tokio::test]
async fn 直连下载_断点续传() {
    let (url, reg) = start_registry(false).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("blobs")).unwrap();
    // 预置前 80000 字节的半成品
    let part = dir
        .path()
        .join("blobs")
        .join(format!("sha256-{}.part", reg.blob_hex));
    std::fs::write(&part, &reg.blob[..80_000]).unwrap();
    model_fetch::pull_direct(
        &reqwest::Client::new(),
        &url,
        dir.path(),
        "tiny",
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        reg.range_requests.load(Ordering::SeqCst),
        1,
        "应使用 Range 续传"
    );
    let blob_path = dir
        .path()
        .join("blobs")
        .join(format!("sha256-{}", reg.blob_hex));
    assert_eq!(std::fs::read(blob_path).unwrap(), reg.blob);
}

#[tokio::test]
async fn 直连下载_校验失败时清理且不写manifest() {
    let (url, reg) = start_registry(true).await;
    let dir = tempfile::tempdir().unwrap();
    let err = model_fetch::pull_direct(
        &reqwest::Client::new(),
        &url,
        dir.path(),
        "tiny",
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("校验失败"), "{err}");
    let blobs = dir.path().join("blobs");
    assert!(!blobs.join(format!("sha256-{}", reg.blob_hex)).exists());
    assert!(!blobs.join(format!("sha256-{}.part", reg.blob_hex)).exists());
    assert!(!dir
        .path()
        .join("manifests/registry.ollama.ai/library/tiny/latest")
        .exists());
}

#[tokio::test]
async fn 直连下载_可取消() {
    let (url, _reg) = start_registry(false).await;
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(true);
    let err = model_fetch::pull_direct(
        &reqwest::Client::new(),
        &url,
        dir.path(),
        "tiny",
        &cancel,
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AppError::Cancelled));
}
