// @author ygw
//! 集成测试:用本地 mock Ollama 服务验证完整链路
//! 录入 → 解析 → 向量化 → 检索 → 问答 → 重刷 → 降级 → 模型切换,无需真实模型。

use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use memflow_lib::chat::{self, ChatEvent};
use memflow_lib::core::{test_core, Core};
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::repo;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// mock 服务的可控状态
#[derive(Default)]
struct MockState {
    /// 结构化请求是否返回非法 JSON
    bad_json: AtomicBool,
    /// 收到的 embed 调用次数
    embed_calls: AtomicUsize,
    /// 记录最近一次问答请求的 system 提示词
    last_system: Mutex<String>,
}

/// 用字符二元组哈希到 512 维,保证"共享词语"的文本相似度更高
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

async fn embed(State(st): State<Arc<MockState>>, Json(body): Json<Value>) -> Json<Value> {
    st.embed_calls.fetch_add(1, Ordering::SeqCst);
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
    if body.get("format").is_some() {
        // 结构化提取
        let content = if st.bad_json.load(Ordering::SeqCst) {
            "这不是 JSON".to_string()
        } else {
            let text: String = user
                .split("内容:")
                .last()
                .unwrap_or("")
                .trim()
                .chars()
                .take(12)
                .collect();
            json!({"summary": format!("摘要:{text}"), "category": "测试", "tags": ["标签甲", "#标签乙"], "entities": ["老王"]}).to_string()
        };
        let body = json!({"message": {"role": "assistant", "content": content}, "done": true});
        return Response::builder()
            .header("content-type", "application/json")
            .body(body.to_string().into())
            .unwrap();
    }
    if stream {
        *st.last_system.lock().unwrap() = messages[0]["content"].as_str().unwrap_or("").to_string();
        // 故意引用一个不存在的编号 [9],用于验证角标校验
        let parts = ["根据资料", "[1],", "预算会缩减。", "[9]"];
        let mut out = String::new();
        for p in parts {
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
    // 普通非流式(如查询改写、Mermaid 修复)
    let body = json!({"message": {"role": "assistant", "content": "flowchart TD\n    A[\"开始\"] --> B[\"结束\"]"}, "done": true});
    Response::builder()
        .header("content-type", "application/json")
        .body(body.to_string().into())
        .unwrap()
}

async fn generate() -> Json<Value> {
    Json(json!({"done": true}))
}

/// 启动 mock 服务,返回地址与状态
async fn start_mock() -> (String, Arc<MockState>) {
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

fn new_core(url: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), url);
    let mut s = core.settings();
    s.score_threshold = 0.12;
    core.update_settings(s).unwrap();
    (core, dir)
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
async fn 完整链路_录入到问答() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    let a = put(&core, "今天和老王聊了A项目,他说下个月预算会缩减20%").await;
    let b = put(&core, "周末去爬山,顺便买了新的登山杖").await;
    assert_eq!(queue::drain_once(&core).await.unwrap(), 2);

    // 1) 解析结果
    for id in [a, b] {
        let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
        assert_eq!(it.status, "done");
        assert_eq!(it.chunk_count, 1);
        assert_eq!(it.tags, vec!["标签甲", "标签乙"], "标签应被清洗(去 # 号)");
        assert!(it.summary.unwrap().starts_with("摘要:"));
    }
    assert_eq!(core.vindex.len(), 2);

    // 2) 问答:命中老王那条,角标 [9] 被移除
    let events = Arc::new(Mutex::new(Vec::<ChatEvent>::new()));
    let ev = events.clone();
    chat::ask(&core, None, "老王说预算怎么样", None, &move |e| {
        ev.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let evs = events.lock().unwrap();
    let sources = evs
        .iter()
        .find_map(|e| {
            if let ChatEvent::Sources { sources } = e {
                Some(sources.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(!sources.is_empty());
    assert_eq!(sources[0].item_id, a, "最相关的应是老王那条");
    let done = evs
        .iter()
        .find_map(|e| {
            if let ChatEvent::Done { answer, .. } = e {
                Some(answer.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(done.contains("[1]"));
    assert!(!done.contains("[9]"), "不存在的角标必须被移除: {done}");
    assert!(
        mock.last_system.lock().unwrap().contains("[1] 记录于"),
        "提示词应带编号资料"
    );

    // 3) 消息已持久化(用户 + 助手,助手带来源)
    let sid = core.db.with(|c| Ok(repo::list_sessions(c)?[0].id)).unwrap();
    let msgs = core.db.with(|c| repo::list_messages(c, sid)).unwrap();
    assert_eq!(msgs.len(), 2);
    assert!(msgs[1].sources.is_some());
}

#[tokio::test]
async fn 无相关资料时拒答且不调用模型() {
    let (url, mock) = start_mock().await;
    let (core, _d) = new_core(&url);
    put(&core, "今天和老王聊了A项目").await;
    queue::drain_once(&core).await.unwrap();
    *mock.last_system.lock().unwrap() = String::new();
    let events = Arc::new(Mutex::new(Vec::<ChatEvent>::new()));
    let ev = events.clone();
    chat::ask(&core, None, "量子计算机的原理", None, &move |e| {
        ev.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let evs = events.lock().unwrap();
    let done = evs
        .iter()
        .find_map(|e| {
            if let ChatEvent::Done { answer, .. } = e {
                Some(answer.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(done, chat::NO_RESULT_ANSWER);
    assert!(
        mock.last_system.lock().unwrap().is_empty(),
        "拒答时不应调用生成模型"
    );
}

#[tokio::test]
async fn ollama不可用时数据不丢失且不消耗重试次数() {
    let (core, _d) = new_core("http://127.0.0.1:9");
    let id = put(&core, "Ollama 没开的时候也要能记下来").await;
    queue::drain_once(&core).await.unwrap();
    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.status, "pending");
    assert_eq!(it.content, "Ollama 没开的时候也要能记下来", "原文必须保留");
    assert!(it.error.is_some(), "应记录等待原因");
    let attempts: i64 = core
        .db
        .with(|c| Ok(c.query_row("SELECT attempts FROM jobs", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(attempts, 0, "临时不可用不应消耗重试次数");
    let st = queue::queue_status(&core).unwrap();
    assert!(st.blocked_reason.is_some());
}

#[tokio::test]
async fn 结构化提取失败时降级为仅原文加向量() {
    let (url, mock) = start_mock().await;
    mock.bad_json.store(true, Ordering::SeqCst);
    let (core, _d) = new_core(&url);
    let id = put(&core, "模型输出坏 JSON 时仍然要能被检索到").await;
    queue::drain_once(&core).await.unwrap();
    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.status, "done", "应降级完成而不是失败");
    assert!(it.tags.is_empty() && it.summary.is_none());
    assert_eq!(it.chunk_count, 1);
    assert_eq!(core.vindex.len(), 1);

    // 模型恢复后重刷即可补齐元数据
    mock.bad_json.store(false, Ordering::SeqCst);
    ingest::reindex(&core, Some(vec![id])).unwrap();
    queue::drain_once(&core).await.unwrap();
    let it = core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap();
    assert_eq!(it.tags.len(), 2);
}

#[tokio::test]
async fn 重刷不会产生重复切片或脏fts() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let id = put(&core, "重刷之后切片数量必须保持稳定").await;
    queue::drain_once(&core).await.unwrap();
    for _ in 0..3 {
        assert_eq!(ingest::reindex(&core, Some(vec![id])).unwrap(), 1);
        queue::drain_once(&core).await.unwrap();
    }
    let (chunks, fts): (i64, i64) = core
        .db
        .with(|c| {
            Ok((
                c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?,
                c.query_row("SELECT count(*) FROM chunks_fts", [], |r| r.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!((chunks, fts), (1, 1));
    assert_eq!(core.vindex.len(), 1);
}

#[tokio::test]
async fn 更换向量模型后旧向量隔离_重刷后恢复() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let id = put(&core, "更换向量模型需要全库重刷才能继续被向量检索").await;
    queue::drain_once(&core).await.unwrap();
    assert_eq!(core.vindex.len(), 1);

    let mut s = core.settings();
    s.embed_model = "nomic-embed-text".into();
    core.update_settings(s).unwrap();
    assert_eq!(core.vindex.len(), 0, "旧模型向量不应参与检索");
    let stats = core
        .db
        .with(|c| repo::stats(c, "nomic-embed-text"))
        .unwrap();
    assert_eq!(stats.stale_chunks, 1, "应提示需要重刷");

    ingest::reindex(&core, None).unwrap();
    queue::drain_once(&core).await.unwrap();
    assert_eq!(core.vindex.len(), 1);
    let stats = core
        .db
        .with(|c| repo::stats(c, "nomic-embed-text"))
        .unwrap();
    assert_eq!(stats.stale_chunks, 0);
    let _ = id;
}

#[tokio::test]
async fn 文档导入_分块并带全局摘要() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    // 开启文档 AI 解读,验证 AI 摘要会挂到每个切片
    let mut s = core.settings();
    s.doc_ai_enrich = true;
    core.update_settings(s).unwrap();
    let doc = dir.path().join("设计文档.md");
    let body = (0..40)
        .map(|i| {
            format!("## 第{i}节\n\n这一节讲述缓存架构的第{i}个要点,包含大量说明文字用于撑长内容。")
                .repeat(3)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(&doc, body).unwrap();
    let r = ingest::capture(
        &core,
        CaptureInput {
            text: None,
            paths: vec![doc.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    queue::drain_once(&core).await.unwrap();
    let detail = core
        .db
        .with(|c| {
            Ok((
                repo::get_item(c, r.created[0])?.unwrap(),
                repo::list_chunks(c, r.created[0])?,
            ))
        })
        .unwrap();
    assert_eq!(detail.0.status, "done");
    assert!(detail.1.len() > 3, "长文档应被切成多个切片");
    assert!(
        detail.1.iter().all(|c| c
            .doc_summary
            .as_deref()
            .map(|s| s.starts_with("摘要:"))
            .unwrap_or(false)),
        "每个切片都应挂全局摘要"
    );
    assert!(
        detail
            .1
            .iter()
            .any(|c| c.heading.as_deref().unwrap_or("").starts_with("第")),
        "应保留标题路径"
    );
}

#[tokio::test]
async fn 无文字线索的图片仅归档() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let img = dir.path().join("x.png");
    std::fs::write(&img, b"\x89PNG fake").unwrap();
    let r = ingest::capture(
        &core,
        CaptureInput {
            text: None,
            paths: vec![img.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    queue::drain_once(&core).await.unwrap();
    let it = core
        .db
        .with(|c| repo::get_item(c, r.created[0]))
        .unwrap()
        .unwrap();
    assert_eq!(it.status, "archived_only");
    assert_eq!(it.chunk_count, 0);
}

#[tokio::test]
async fn 删除记录后不再被检索到() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let id = put(&core, "老王说预算会缩减").await;
    queue::drain_once(&core).await.unwrap();
    ingest::delete_items(&core, &[id]).await.unwrap();
    assert_eq!(core.vindex.len(), 0);
    let hits = memflow_lib::retrieval::hybrid_search(
        &core,
        "老王预算",
        &memflow_lib::retrieval::SearchOptions {
            top_n: 5,
            threshold: 0.1,
        },
    )
    .await
    .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn 文档默认快速索引_不调用模型提取() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let doc = dir.path().join("快速.md");
    std::fs::write(&doc, "# 缓存方案\n\n本文介绍缓存架构与失效策略。").unwrap();
    let r = ingest::capture(
        &core,
        CaptureInput {
            text: None,
            paths: vec![doc.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    queue::drain_once(&core).await.unwrap();
    let (item, chunks) = core
        .db
        .with(|c| {
            Ok((
                repo::get_item(c, r.created[0])?.unwrap(),
                repo::list_chunks(c, r.created[0])?,
            ))
        })
        .unwrap();
    assert_eq!(item.status, "done");
    assert!(!chunks.is_empty());
    assert!(item.meta_model.is_none(), "快速索引不应记录 AI 模型");
    assert!(item.summary.as_deref().unwrap_or("").contains("缓存架构"));
    assert!(item.tags.is_empty());

    // 显式 AI 解读后才记录模型并生成摘要
    ingest::reindex_ai(&core, vec![r.created[0]]).unwrap();
    queue::drain_once(&core).await.unwrap();
    let item = core
        .db
        .with(|c| Ok(repo::get_item(c, r.created[0])?.unwrap()))
        .unwrap();
    assert!(item.meta_model.is_some(), "AI 解读后应记录模型");
}
