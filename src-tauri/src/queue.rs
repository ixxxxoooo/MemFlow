// @author ygw
//! 持久化任务队列与串行 Worker。
//!
//! - 任务存于 SQLite,应用崩溃/重启后自动恢复
//! - 单 Worker 串行执行(16GB 内存下避免多个模型并发)
//! - 临时不可用(Ollama 未开/模型未装)不消耗重试次数;业务错误指数退避重试
//! - 有用户问答进行时,后台任务让路

use crate::core::{CancelToken, Core, CurrentJob};
use crate::db::{now_ms, Db};
use crate::error::{AppError, Result};
use crate::pipeline;
use crate::repo;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// 任务类型:处理一条记录(解析 → 提取 → 分块 → 向量化)
pub const KIND_PROCESS_ITEM: &str = "process_item";
/// 临时不可用时的重试间隔
const UNAVAILABLE_RETRY_SECS: i64 = 30;

/// 任务负载
#[derive(Debug, Serialize, Deserialize, Default)]
struct Payload {
    /// ingest 首次入库 / reindex 重刷
    mode: String,
}

/// 队列状态(推送给前端)
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    pub pending: i64,
    pub running: i64,
    pub failed: i64,
    /// 正在处理的记录
    pub current_item: Option<serde_json::Value>,
    /// 任务被阻塞的原因(如 Ollama 未运行)
    pub blocked_reason: Option<String>,
}

/// 已领取的任务
struct Job {
    id: i64,
    item_id: i64,
    mode: String,
    attempts: i64,
    max_attempts: i64,
}

/// 异常退出恢复:把 running 复位为 pending
///
/// 参数: db 数据库
pub fn recover_after_crash(db: &Db) -> Result<()> {
    db.with(|c| {
        c.execute(
            "UPDATE jobs SET status='pending' WHERE status='running'",
            [],
        )?;
        c.execute(
            "UPDATE items SET status='pending' WHERE status='processing'",
            [],
        )?;
        Ok(())
    })
}

/// 入队处理任务;已存在待执行任务则不重复入队
///
/// 参数: c 连接/事务;item_id 记录 id;mode ingest 或 reindex
/// 返回: 是否新建了任务
pub fn enqueue_item(c: &Connection, item_id: i64, mode: &str) -> Result<bool> {
    let exists: i64 = c.query_row(
        "SELECT count(*) FROM jobs WHERE item_id = ?1 AND kind = ?2 AND status = 'pending'",
        params![item_id, KIND_PROCESS_ITEM],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(false);
    }
    let now = now_ms();
    c.execute(
        "INSERT INTO jobs(kind,item_id,payload,status,next_run_at,created_at,updated_at) VALUES(?1,?2,?3,'pending',?4,?4,?4)",
        params![KIND_PROCESS_ITEM, item_id, serde_json::to_string(&Payload { mode: mode.into() })?, now],
    )?;
    c.execute(
        "UPDATE items SET status='pending', error=NULL, updated_at=?2 WHERE id=?1 AND status != 'processing'",
        params![item_id, now],
    )?;
    Ok(true)
}

/// 计算队列状态
///
/// 参数: core 核心
pub fn queue_status(core: &Core) -> Result<QueueStatus> {
    let mut st = core.db.with_read(|c| {
        let cnt = |s: &str| -> rusqlite::Result<i64> { c.query_row(&format!("SELECT count(*) FROM jobs WHERE status='{s}'"), [], |r| r.get(0)) };
        let blocked: Option<String> = c
            .query_row(
                "SELECT last_error FROM jobs WHERE status='pending' AND last_error IS NOT NULL ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(QueueStatus { pending: cnt("pending")?, running: cnt("running")?, failed: cnt("failed")?, current_item: None, blocked_reason: blocked })
    })?;
    if let Some(cur) = core.current_job.lock().unwrap().clone() {
        let title = core.db.with_read(|c| repo::get_item(c, cur.item_id))?.map(
            |it| json!({"id": it.id, "kind": it.kind, "title": it.title.or(it.original_name)}),
        );
        st.current_item = title;
    }
    Ok(st)
}

/// 推送队列状态事件
pub fn emit_status(core: &Core) {
    if let Ok(st) = queue_status(core) {
        core.emit("queue-status", serde_json::to_value(st).unwrap_or_default());
    }
}

/// 领取下一个到期任务
fn claim_next(core: &Core) -> Result<Option<Job>> {
    core.db.with(|c| {
        let row = c
            .query_row(
                "SELECT id, item_id, payload, attempts, max_attempts FROM jobs WHERE status='pending' AND next_run_at <= ?1 ORDER BY id LIMIT 1",
                [now_ms()],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?)),
            )
            .optional()?;
        let Some((id, item_id, payload, attempts, max_attempts)) = row else { return Ok(None) };
        c.execute("UPDATE jobs SET status='running', updated_at=?2 WHERE id=?1", params![id, now_ms()])?;
        let mode = serde_json::from_str::<Payload>(&payload).map(|p| p.mode).unwrap_or_default();
        Ok(Some(Job { id, item_id: item_id.unwrap_or(0), mode, attempts, max_attempts }))
    })
}

/// 指数退避秒数:10s、20s、40s…… 上限 5 分钟
fn backoff_secs(attempts: i64) -> i64 {
    (10i64 << attempts.clamp(0, 5)).min(300)
}

/// 是否为不应重试的永久性错误
fn is_permanent(e: &AppError) -> bool {
    matches!(e, AppError::Invalid(_) | AppError::Parse(_))
}

/// 根据执行结果更新任务与记录状态
fn settle(core: &Core, job: &Job, result: Result<()>) -> Result<()> {
    let now = now_ms();
    match result {
        Ok(()) => {
            core.db.with(|c| {
                c.execute(
                    "UPDATE jobs SET status='done', last_error=NULL, updated_at=?2 WHERE id=?1",
                    params![job.id, now],
                )?;
                Ok(())
            })?;
        }
        Err(AppError::Cancelled) => {
            core.db.with(|c| {
                c.execute(
                    "UPDATE jobs SET status='cancelled', updated_at=?2 WHERE id=?1",
                    params![job.id, now],
                )?;
                repo::set_item_status(c, job.item_id, "failed", Some("已取消"))
            })?;
        }
        Err(e) if e.is_unavailable() => {
            // 临时不可用:不消耗次数,稍后重试,记录原因供界面提示
            let msg = e.to_string();
            core.db.with(|c| {
                c.execute(
                    "UPDATE jobs SET status='pending', last_error=?2, next_run_at=?3, updated_at=?4 WHERE id=?1",
                    params![job.id, msg, now + UNAVAILABLE_RETRY_SECS * 1000, now],
                )?;
                repo::set_item_status(c, job.item_id, "pending", Some(&msg))
            })?;
        }
        Err(e) => {
            let msg = e.to_string();
            let attempts = job.attempts + 1;
            let give_up = is_permanent(&e) || attempts >= job.max_attempts;
            core.db.with(|c| {
                if give_up {
                    c.execute(
                        "UPDATE jobs SET status='failed', attempts=?2, last_error=?3, updated_at=?4 WHERE id=?1",
                        params![job.id, attempts, msg, now],
                    )?;
                    repo::set_item_status(c, job.item_id, "failed", Some(&msg))
                } else {
                    c.execute(
                        "UPDATE jobs SET status='pending', attempts=?2, last_error=?3, next_run_at=?4, updated_at=?5 WHERE id=?1",
                        params![job.id, attempts, msg, now + backoff_secs(attempts) * 1000, now],
                    )?;
                    repo::set_item_status(c, job.item_id, "pending", Some(&msg))
                }
            })?;
        }
    }
    Ok(())
}

/// 执行一个任务(在独立 tokio 任务中运行以捕获 panic)
async fn run_job(core: &Arc<Core>, job: Job) -> Result<()> {
    let token = CancelToken::default();
    *core.current_job.lock().unwrap() = Some(CurrentJob {
        job_id: job.id,
        item_id: job.item_id,
        token: token.clone(),
    });
    emit_status(core);
    let (c2, t2, mode, item_id) = (core.clone(), token, job.mode.clone(), job.item_id);
    let handle =
        tokio::spawn(async move { pipeline::process_item(&c2, item_id, &mode, &t2).await });
    let result = match handle.await {
        Ok(r) => r,
        Err(e) => Err(AppError::Other(format!("任务内部崩溃: {e}"))),
    };
    *core.current_job.lock().unwrap() = None;
    settle(core, &job, result)?;
    core.emit("item-updated", json!({ "id": job.item_id }));
    emit_status(core);
    Ok(())
}

/// 处理所有到期任务,直到队列暂时为空
///
/// 参数: core 核心
/// 返回: 本轮处理的任务数
pub async fn drain_once(core: &Arc<Core>) -> Result<usize> {
    let mut n = 0;
    loop {
        // 用户正在问答时让路,避免抢占模型
        while core.chat_busy() {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let Some(job) = claim_next(core)? else { break };
        run_job(core, job).await?;
        n += 1;
    }
    Ok(n)
}

/// 后台 Worker 主循环(永不退出)
///
/// 参数: core 核心
pub async fn run_worker(core: Arc<Core>) {
    emit_status(&core);
    loop {
        if let Err(e) = drain_once(&core).await {
            tracing::error!("队列处理出错: {e}");
        }
        emit_status(&core);
        tokio::select! {
            _ = core.wake.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

/// 取消某记录的待执行/执行中任务
///
/// 参数: core 核心;item_id 记录 id
pub fn cancel_item(core: &Core, item_id: i64) -> Result<()> {
    core.db.with(|c| {
        let n = c.execute(
            "UPDATE jobs SET status='cancelled', updated_at=?2 WHERE item_id=?1 AND status='pending'",
            params![item_id, now_ms()],
        )?;
        if n > 0 {
            repo::set_item_status(c, item_id, "failed", Some("已取消"))?;
        }
        Ok(())
    })?;
    if let Some(cur) = core.current_job.lock().unwrap().as_ref() {
        if cur.item_id == item_id {
            cur.token.cancel();
        }
    }
    emit_status(core);
    core.emit("item-updated", json!({ "id": item_id }));
    Ok(())
}

/// 把失败的记录重新入队
///
/// 参数: core 核心;ids 指定记录(None 表示全部失败记录)
/// 返回: 重新入队的数量
pub fn retry_failed(core: &Core, ids: Option<Vec<i64>>) -> Result<usize> {
    let n = core.db.with_tx(|tx| {
        let targets: Vec<i64> = match ids {
            Some(v) => v,
            None => {
                let mut stmt = tx.prepare("SELECT id FROM items WHERE status='failed'")?;
                let rows = stmt.query_map([], |r| r.get(0))?;
                rows.collect::<std::result::Result<Vec<i64>, _>>()?
            }
        };
        let mut n = 0;
        for id in targets {
            tx.execute(
                "UPDATE jobs SET attempts=0 WHERE item_id=?1 AND status='failed'",
                [id],
            )?;
            if enqueue_item(tx, id, "reindex")? {
                n += 1;
            }
        }
        Ok(n)
    })?;
    core.wake.notify_one();
    emit_status(core);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{insert_item, NewItem};

    fn mk() -> (Db, i64) {
        let db = Db::open_memory().unwrap();
        let id = db
            .with(|c| {
                insert_item(
                    c,
                    &NewItem {
                        kind: "note".into(),
                        content: "x".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        (db, id)
    }

    #[test]
    fn 重复入队被去重() {
        let (db, id) = mk();
        assert!(db.with(|c| enqueue_item(c, id, "ingest")).unwrap());
        assert!(!db.with(|c| enqueue_item(c, id, "ingest")).unwrap());
    }

    #[test]
    fn 崩溃后running任务复位() {
        let (db, id) = mk();
        db.with(|c| enqueue_item(c, id, "ingest")).unwrap();
        db.with(|c| {
            c.execute("UPDATE jobs SET status='running'", [])?;
            c.execute("UPDATE items SET status='processing'", [])?;
            Ok(())
        })
        .unwrap();
        recover_after_crash(&db).unwrap();
        let (js, is): (String, String) = db
            .with(|c| {
                Ok((
                    c.query_row("SELECT status FROM jobs", [], |r| r.get(0))?,
                    c.query_row("SELECT status FROM items", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((js.as_str(), is.as_str()), ("pending", "pending"));
    }

    #[test]
    fn 退避时间递增并封顶() {
        assert_eq!(backoff_secs(0), 10);
        assert_eq!(backoff_secs(1), 20);
        assert_eq!(backoff_secs(3), 80);
        assert_eq!(backoff_secs(10), 300);
    }

    #[test]
    fn 永久错误判定() {
        assert!(is_permanent(&AppError::Parse("x".into())));
        assert!(!is_permanent(&AppError::Ollama("x".into())));
        assert!(AppError::OllamaUnavailable("x".into()).is_unavailable());
    }
}
