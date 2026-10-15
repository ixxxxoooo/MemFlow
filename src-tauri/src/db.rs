// @author ixxxxoooo
//! SQLite 连接封装与版本化迁移。
//!
//! 设计要点:
//! - 单文件数据库,WAL 模式,外键开启
//! - 通过 `PRAGMA user_version` 做版本化迁移,升级前自动备份旧库
//! - 读写分离:1 个写连接 + 若干只读连接(WAL 下读写互不阻塞),
//!   检索/列表/聊天走读池,入库的长写事务不会卡住界面查询

use crate::error::{AppError, Result};
use rusqlite::{Connection, OpenFlags, Transaction};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// 当前最新的数据库版本
pub const LATEST_VERSION: i64 = 6;

/// 只读连接池大小
const READ_POOL_SIZE: usize = 4;

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
    // v2:回答记录模型与耗时;会话记住所选模型;切片记录页码
    r#"
    ALTER TABLE chat_messages ADD COLUMN model TEXT;
    ALTER TABLE chat_messages ADD COLUMN provider TEXT;
    ALTER TABLE chat_messages ADD COLUMN elapsed_ms INTEGER;
    ALTER TABLE chat_messages ADD COLUMN first_token_ms INTEGER;
    ALTER TABLE chat_sessions ADD COLUMN model_ref TEXT;
    ALTER TABLE chunks ADD COLUMN page INTEGER;
    "#,
    // v3:多知识库。默认知识库 id=1;记录归属 kb_id;会话记住所选范围(JSON 数组,空 = 全部)
    // 注意:SQLite 在开启外键时不允许新增带 REFERENCES 且默认非空的列,归属有效性由代码保证
    r#"
    CREATE TABLE knowledge_bases(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE,
        description TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL
    );
    INSERT INTO knowledge_bases(id, name, description, created_at)
        VALUES(1, '默认知识库', '', CAST(strftime('%s','now') AS INTEGER) * 1000);
    ALTER TABLE items ADD COLUMN kb_id INTEGER NOT NULL DEFAULT 1;
    CREATE INDEX idx_items_kb ON items(kb_id, id DESC);
    ALTER TABLE chat_sessions ADD COLUMN kb_ids TEXT;
    "#,
    // v4:记录来源路径(文件夹导入时为「文件夹名/相对路径」)
    r#"
    ALTER TABLE items ADD COLUMN source_path TEXT;
    "#,
    // v5:图文关联。item_links 为记录间的多对多关系(relation: image=文档引用图片,merged_from=归并来源),
    // anchor 为引用处在原文中的写法(如 md 的相对路径);chunk_images 记录哪个切片里出现了哪张图;
    // embedded=1 表示图片由文档内嵌/引用自动导入(最后一个引用它的文档被删除时一并清理);
    // item_meta 拆分出本机 OCR 文字与视觉模型描述(vision_text 保留作兼容与展示)
    r#"
    ALTER TABLE items ADD COLUMN embedded INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE item_meta ADD COLUMN ocr_text TEXT;
    ALTER TABLE item_meta ADD COLUMN vision_desc TEXT;
    CREATE TABLE item_links(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        parent_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
        child_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
        relation TEXT NOT NULL DEFAULT 'image',
        anchor TEXT NOT NULL DEFAULT '',
        seq INTEGER NOT NULL DEFAULT 0,
        UNIQUE(parent_id, relation, seq)
    );
    CREATE INDEX idx_links_child ON item_links(child_id);
    CREATE TABLE chunk_images(
        chunk_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
        image_item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
        PRIMARY KEY(chunk_id, image_item_id)
    );
    CREATE INDEX idx_chunk_images_img ON chunk_images(image_item_id);
    "#,
    // v6:AI 整理的修订历史。整理覆盖原文前先把旧版本存进来,支持撤销;随记录一起删除
    r#"
    CREATE TABLE item_revisions(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
        title TEXT,
        content TEXT NOT NULL,
        reason TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL
    );
    CREATE INDEX idx_revisions_item ON item_revisions(item_id, id);
    "#,
];

/// 默认知识库 id(不可删除)
pub const DEFAULT_KB_ID: i64 = 1;

/// 性能相关 PRAGMA:64MB 页缓存、256MB 内存映射、临时表放内存
const PERF_PRAGMAS: &str =
    "PRAGMA cache_size=-65536; PRAGMA mmap_size=268435456; PRAGMA temp_store=MEMORY;";

/// 当前毫秒时间戳
///
/// 返回: Unix 毫秒
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 数据库句柄(线程安全)
pub struct Db {
    /// 唯一的写连接
    conn: Mutex<Connection>,
    /// 只读连接池(内存库为空,此时读操作回退到写连接)
    readers: Vec<Mutex<Connection>>,
    /// 轮询起点,让并发读尽量分散到不同连接
    next_reader: AtomicUsize,
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
        let mut db = Self::init(conn)?;
        // 写连接完成迁移并建立 WAL 后再开只读连接
        for _ in 0..READ_POOL_SIZE {
            let c = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            c.busy_timeout(std::time::Duration::from_secs(10))?;
            c.execute_batch(PERF_PRAGMAS)?;
            db.readers.push(Mutex::new(c));
        }
        Ok(db)
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
        conn.execute_batch(PERF_PRAGMAS)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for v in version..LATEST_VERSION {
            let tx = conn.transaction()?;
            tx.execute_batch(MIGRATIONS[v as usize])?;
            tx.execute_batch(&format!("PRAGMA user_version = {};", v + 1))?;
            tx.commit()?;
        }
        Ok(Db {
            conn: Mutex::new(conn),
            readers: Vec::new(),
            next_reader: AtomicUsize::new(0),
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

    /// 在只读连接池上执行查询(闭包内不得写库)。
    ///
    /// WAL 模式下读连接看到的是其开始读时已提交的最新数据,且不会被写事务阻塞。
    ///
    /// 参数: f 使用只读连接的闭包
    /// 返回: 闭包返回值
    pub fn with_read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let n = self.readers.len();
        if n == 0 {
            return self.with(f);
        }
        let start = self.next_reader.fetch_add(1, Ordering::Relaxed) % n;
        // 先找一个空闲连接,都忙时才阻塞等待起点连接
        for i in 0..n {
            if let Ok(guard) = self.readers[(start + i) % n].try_lock() {
                return f(&guard);
            }
        }
        let guard = self.readers[start]
            .lock()
            .map_err(|_| AppError::Other("数据库读锁已损坏".into()))?;
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
                "knowledge_bases",
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
    fn v1库可升级到最新版并保留数据() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("old.db");
        {
            // 模拟旧版本:只执行 v1 迁移
            let c = Connection::open(&p).unwrap();
            c.execute_batch(MIGRATIONS[0]).unwrap();
            c.execute_batch("PRAGMA user_version = 1;").unwrap();
            c.execute(
                "INSERT INTO chat_sessions(title, created_at, updated_at) VALUES('旧会话', 1, 1)",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&p, Some(&dir.path().join("bk"))).unwrap();
        db.with(|c| {
            let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            assert_eq!(v, LATEST_VERSION);
            let t: String = c.query_row("SELECT title FROM chat_sessions", [], |r| r.get(0))?;
            assert_eq!(t, "旧会话");
            // 新列可用
            c.execute("UPDATE chat_sessions SET model_ref='ollama:x'", [])?;
            c.execute("UPDATE chunks SET page=1", [])?;
            c.execute("UPDATE chat_sessions SET kb_ids='[1]'", [])?;
            // v3:默认知识库存在
            let kb: String =
                c.query_row("SELECT name FROM knowledge_bases WHERE id=1", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(kb, "默认知识库");
            Ok(())
        })
        .unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path().join("bk")).unwrap().count(),
            1,
            "升级前应自动备份"
        );
    }

    #[test]
    fn v2库升级到v3后旧记录归入默认知识库() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v2.db");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(MIGRATIONS[0]).unwrap();
            c.execute_batch(MIGRATIONS[1]).unwrap();
            c.execute_batch("PRAGMA user_version = 2;").unwrap();
            c.execute(
                "INSERT INTO items(kind,content,status,created_at,updated_at) VALUES('note','旧笔记','done',1,1)",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&p, Some(&dir.path().join("bk"))).unwrap();
        db.with(|c| {
            let (kb, content): (i64, String) =
                c.query_row("SELECT kb_id, content FROM items", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
            assert_eq!(kb, DEFAULT_KB_ID);
            assert_eq!(content, "旧笔记");
            let kb_ids: Option<String> = c
                .query_row("SELECT kb_ids FROM chat_sessions", [], |r| r.get(0))
                .unwrap_or(None);
            assert!(kb_ids.is_none());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path().join("bk")).unwrap().count(),
            1,
            "升级前应自动备份"
        );
    }

    #[test]
    fn 读池能读到写连接已提交的数据且读连接只读() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), None).unwrap();
        db.with(|c| {
            c.execute("INSERT INTO settings(key,value) VALUES('k','v')", [])?;
            Ok(())
        })
        .unwrap();
        let v: String = db
            .with_read(|c| {
                Ok(c.query_row("SELECT value FROM settings WHERE key='k'", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(v, "v");
        // 读连接禁止写入
        let w = db.with_read(|c| Ok(c.execute("DELETE FROM settings", [])?));
        assert!(w.is_err());
    }

    #[test]
    fn 写事务进行中读池不被阻塞() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(Db::open(&dir.path().join("t.db"), None).unwrap());
        let (tx_started, rx_started) = std::sync::mpsc::channel();
        let (tx_done, rx_done) = std::sync::mpsc::channel::<()>();
        let d2 = db.clone();
        let h = std::thread::spawn(move || {
            d2.with_tx(|tx| {
                tx.execute("INSERT INTO settings(key,value) VALUES('a','1')", [])?;
                tx_started.send(()).unwrap();
                // 持有写事务,直到主线程读完
                rx_done.recv().unwrap();
                Ok(())
            })
            .unwrap();
        });
        rx_started.recv().unwrap();
        // 写连接被占用时,读池仍可立即查询(且看不到未提交数据)
        let n: i64 = db
            .with_read(|c| Ok(c.query_row("SELECT count(*) FROM settings", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
        tx_done.send(()).unwrap();
        h.join().unwrap();
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
