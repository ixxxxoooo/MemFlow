//! 真机端到端冒烟:依赖本机已安装 bge-m3 与 qwen2.5:7b。
//! 运行:`cargo test --test e2e_real_ollama -- --ignored --nocapture`
//! @author ygw

use memflow_lib::chat::{self, ChatEvent};
use memflow_lib::core::test_core;
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::queue;
use memflow_lib::repo;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 等待条目进入终态(done/failed/archived_only)。
/// 用途:真机处理较慢时轮询 drain。
/// 参数:core、item_id、最长等待秒数。
/// 返回:最终 status。
async fn wait_done(core: &Arc<memflow_lib::core::Core>, item_id: i64, secs: u64) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let _ = queue::drain_once(core).await;
        let status = core
            .db
            .with(|c| {
                Ok(
                    c.query_row("SELECT status FROM items WHERE id=?1", [item_id], |r| {
                        r.get::<_, String>(0)
                    })?,
                )
            })
            .unwrap_or_else(|_| "missing".into());
        if matches!(status.as_str(), "done" | "failed" | "archived_only") {
            return status;
        }
        if tokio::time::Instant::now() > deadline {
            return format!("timeout:{status}");
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test]
#[ignore = "需要本机 Ollama 与 bge-m3、qwen2.5:7b"]
async fn 真机冒烟_录入问答流程图重刷溯源() {
    // 先确认 Ollama 可用
    let client = reqwest::Client::new();
    let tags = client
        .get("http://127.0.0.1:11434/api/tags")
        .send()
        .await
        .expect("Ollama 未运行")
        .text()
        .await
        .unwrap();
    assert!(tags.contains("bge-m3"), "缺少 bge-m3: {tags}");
    assert!(tags.contains("qwen2.5:7b"), "缺少 qwen2.5:7b: {tags}");

    let dir = tempfile::tempdir().unwrap();
    let core = test_core(dir.path(), "http://127.0.0.1:11434");

    // 1. 录入
    let r = ingest::capture(
        &core,
        CaptureInput {
            text: Some("明天下午三点和老王开会,讨论 Q4 预算与招聘计划。".into()),
            paths: vec![],
            ..Default::default()
        },
    )
    .await
    .expect("录入失败");
    let item_id = r.created[0];
    let status = wait_done(&core, item_id, 300).await;
    assert_eq!(status, "done", "AI 归档应成功: {status}");

    let it = core
        .db
        .with(|c| repo::get_item(c, item_id))
        .unwrap()
        .unwrap();
    assert_eq!(it.status, "done");
    assert!(it.chunk_count >= 1, "应有切片");
    assert!(!core.vindex.is_empty());

    // 2. 问答 + 溯源
    let events = Arc::new(Mutex::new(Vec::<ChatEvent>::new()));
    let ev = events.clone();
    chat::ask(
        &core,
        None,
        "老王的会议安排是什么?",
        None,
        &move |e| ev.lock().unwrap().push(e),
    )
    .await
    .expect("问答失败");
    let evs = events.lock().unwrap().clone();
    let sources = evs
        .iter()
        .find_map(|e| {
            if let ChatEvent::Sources { sources } = e {
                Some(sources.clone())
            } else {
                None
            }
        })
        .expect("应推送来源");
    assert!(!sources.is_empty(), "应有来源");
    assert_eq!(sources[0].item_id, item_id, "溯源应指向刚录入的记录");
    let done = evs
        .iter()
        .find_map(|e| {
            if let ChatEvent::Done { answer, .. } = e {
                Some(answer.clone())
            } else {
                None
            }
        })
        .expect("应有完成事件");
    assert!(!done.is_empty(), "应有回答");
    assert!(!done.contains("[99]"), "非法引用不应出现");
    let session_id = evs
        .iter()
        .find_map(|e| {
            if let ChatEvent::Session { session_id } = e {
                Some(*session_id)
            } else {
                None
            }
        })
        .expect("应有会话");

    // 3. 流程图
    let events2 = Arc::new(Mutex::new(Vec::<ChatEvent>::new()));
    let ev2 = events2.clone();
    chat::ask(
        &core,
        Some(session_id),
        "请把刚才这条记录整理成 mermaid 流程图",
        None,
        &move |e| ev2.lock().unwrap().push(e),
    )
    .await
    .expect("流程图问答失败");
    let flow = events2
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| {
            if let ChatEvent::Done { answer, .. } = e {
                Some(answer.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let lower = flow.to_lowercase();
    assert!(
        lower.contains("mermaid") || lower.contains("flowchart") || lower.contains("graph "),
        "回答应含流程图: {}",
        &flow[..flow.len().min(300)]
    );

    // 4. 重刷:不产生脏 FTS
    let before = core
        .db
        .with(|c| repo::list_chunks(c, item_id))
        .unwrap()
        .len();
    assert!(ingest::reindex(&core, Some(vec![item_id])).unwrap() >= 1);
    let status2 = wait_done(&core, item_id, 300).await;
    assert_eq!(status2, "done", "重刷后应成功");
    let (chunks, fts): (i64, i64) = core
        .db
        .with(|c| {
            Ok((
                c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?,
                c.query_row("SELECT count(*) FROM chunks_fts", [], |r| r.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!(chunks, fts, "重刷后 FTS 不应有脏数据");
    assert!(chunks >= 1 && before >= 1);

    println!(
        "真机冒烟通过: item={item_id} chunks={chunks} answer_len={} flow_len={}",
        done.len(),
        flow.len()
    );
}
