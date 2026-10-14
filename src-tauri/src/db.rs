// @author ygw
//! SQLite 连接封装与版本化迁移。
//!
//! 设计要点:
//! - 单文件数据库,WAL 模式,外键开启
//! - 通过 `PRAGMA user_version` 做版本化迁移,升级前自动备份旧库
//! - 连接用 Mutex 串行化,所有查询都是短事务

use crate::error::{AppError, Result};
use rusqlite::{Connection, Transaction};
use std::path::Path;
use std::sync::Mutex;

/// 当前最新的数据库版本
pub const LATEST_VERSION: i64 = 1;

/// 迁移脚本列表,下标 i 对应从版本 i 升级到 i+1
const MIGRATIONS: &[&str] = &[
    // v1:初始结构
    r#"
    CREATE TABLE items(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL CHECK(kind IN ('note','image','document')),
        title TEXT,
        content TEXT NOT NULL DEFAULT '',
        attachment_path TEXT,
        original_name TEXT,
        mime TEXT,
        content_hash TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        error TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX idx_items_kind_created ON items(kind, created_at DESC);
    CREATE INDEX idx_items_hash ON items(content_hash);
    CREATE INDEX idx_items_status ON items(status);

    CREATE TABLE item_meta(
        item_id INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
        summary TEXT,
        category TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        entities TEXT NOT NULL DEFAULT '[]',
        vision_text TEXT,
        model TEXT,
        prompt_version INTEGER,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE chunks(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
        seq INTEGER NOT NULL,
        text TEXT NOT NULL,
        heading TEXT,
        doc_summary TEXT,
        embed_model TEXT,
        dim INTEGER,
        embedding BLOB
    );
    CREATE INDEX idx_chunks_item ON chunks(item_id, seq);
    CREATE INDEX idx_chunks_model ON chunks(embed_model);

    CREATE VIRTUAL TABLE chunks_fts USING fts5(tokens, tokenize='unicode61 remove_diacritics 2');

    CREATE TABLE jobs(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL,
        item_id INTEGER,
        payload TEXT NOT NULL DEFAULT '{}',
        status TEXT NOT NULL DEFAULT 'pending',
        attempts INTEGER NOT NULL DEFAULT 0,
        max_attempts INTEGER NOT NULL DEFAULT 5,
        next_run_at INTEGER NOT NULL,
        last_error TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX idx_jobs_status_run ON jobs(status, next_run_at);
    CREATE INDEX idx_jobs_item ON jobs(item_id);

    CREATE TABLE settings(
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE chat_sessions(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE chat_messages(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id INTEGER NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        sources TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX idx_chat_messages_session ON chat_messages(session_id, id);
    "#,
];

/// 当前毫秒时间戳
///
/// 返回: Unix 毫秒
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 数据库句柄(线程安全)
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// 打开(或创建)数据库并执行迁移
    ///
    /// 参数: path 数据库文件路径;backup_dir 迁移前备份目录(None 则不备份)
    /// 返回: 数据库句柄
    pub fn open(path: &Path, backup_dir: Option<&Path>) -> Result<Db> {
        let existing_version = if path.exists() {
            let c = Connection::open(path)?;
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?
        } else {
            0
        };
        // 旧库需要升级时先备份,防止迁移失败损坏数据
        if existing_version > 0 && existing_version < LATEST_VERSION {
            if let Some(dir) = backup_dir {
                std::fs::create_dir_all(dir)?;
                let dest = dir.join(format!("memflow-v{}-{}.db", existing_version, now_ms()));
                std::fs::copy(path, dest)?;
            }
        }
        if existing_version > LATEST_VERSION {
            return Err(AppError::Other(format!(
                "数据库版本 {existing_version} 高于当前程序支持的 {LATEST_VERSION},请升级应用"
            )));
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// 创建内存数据库(测试使用)
    ///
    /// 返回: 数据库句柄
    pub fn open_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    /// 设置 PRAGMA 并执行迁移
    fn init(mut conn: Connection) -> Result<Db> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for v in version..LATEST_VERSION {
            let tx = conn.transaction()?;
            tx.execute_batch(MIGRATIONS[v as usize])?;
            tx.execute_batch(&format!("PRAGMA user_version = {};", v + 1))?;
            tx.commit()?;
        }
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    /// 在连接上执行只读/单语句操作
    ///
    /// 参数: f 使用连接的闭包
    /// 返回: 闭包返回值
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("数据库锁已损坏".into()))?;
        f(&guard)
    }

    /// 在事务中执行,闭包返回 Err 时自动回滚
    ///
    /// 参数: f 使用事务的闭包
    /// 返回: 闭包返回值
    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("数据库锁已损坏".into()))?;
        let tx = guard.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 迁移后版本正确且表存在() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            assert_eq!(v, LATEST_VERSION);
            for t in [
                "items",
                "item_meta",
                "chunks",
                "chunks_fts",
                "jobs",
                "settings",
                "chat_sessions",
                "chat_messages",
            ] {
                let n: i64 = c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name=?",
                    [t],
                    |r| r.get(0),
                )?;
                assert_eq!(n, 1, "缺少表 {t}");
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn 事务出错会回滚() {
        let db = Db::open_memory().unwrap();
        let r: Result<()> = db.with_tx(|tx| {
            tx.execute("INSERT INTO settings(key,value) VALUES('a','1')", [])?;
            Err(AppError::Other("boom".into()))
        });
        assert!(r.is_err());
        let n: i64 = db
            .with(|c| Ok(c.query_row("SELECT count(*) FROM settings", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn 重复打开已有库不会重复迁移() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        drop(Db::open(&p, None).unwrap());
        let db = Db::open(&p, None).unwrap();
        let v: i64 = db
            .with(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(v, LATEST_VERSION);
    }

    #[test]
    fn fts5可用() {
        let db = Db::open_memory().unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO chunks_fts(rowid, tokens) VALUES(1, '老王 预算 缩减')",
                [],
            )?;
            let n: i64 = c.query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH '\"预算\"'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 1);
            Ok(())
        })
        .unwrap();
    }
}
