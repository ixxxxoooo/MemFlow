// @author ygw
//! 应用核心上下文:聚合数据库、向量索引、设置、Ollama 客户端与事件通道。
//! 与 Tauri 解耦(通过 EventSink 抽象),便于集成测试。

use crate::db::Db;
use crate::error::Result;
use crate::ollama::{build_http, GenOptions, OllamaClient};
use crate::repo;
use crate::settings::Settings;
use crate::vector::VectorIndex;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::Notify;

/// 事件发送抽象(生产环境对接 Tauri,测试环境可收集或丢弃)
pub trait EventSink: Send + Sync {
    /// 发送事件
    ///
    /// 参数: event 事件名;payload 负载
    fn emit(&self, event: &str, payload: Value);
}

/// 丢弃所有事件的实现
pub struct NullSink;
impl EventSink for NullSink {
    fn emit(&self, _event: &str, _payload: Value) {}
}

/// 应用数据目录
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// 根目录
    pub root: PathBuf,
    /// 数据库文件
    pub db_path: PathBuf,
    /// 附件目录
    pub attachments: PathBuf,
    /// 临时暂存目录(剪贴板图片等)
    pub tmp: PathBuf,
    /// 迁移备份目录
    pub backups: PathBuf,
}

impl AppPaths {
    /// 基于根目录创建并确保子目录存在
    ///
    /// 参数: root 根目录
    /// 返回: 路径集合
    pub fn new(root: PathBuf) -> Result<Self> {
        let p = AppPaths {
            db_path: root.join("memflow.db"),
            attachments: root.join("attachments"),
            tmp: root.join("tmp"),
            backups: root.join("backups"),
            root,
        };
        for d in [&p.root, &p.attachments, &p.tmp, &p.backups] {
            std::fs::create_dir_all(d)?;
        }
        Ok(p)
    }
}

/// 取消令牌
#[derive(Clone, Default)]
pub struct CancelToken(pub Arc<AtomicBool>);

impl CancelToken {
    /// 是否已取消
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    /// 触发取消
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// 当前正在执行的任务
#[derive(Clone)]
pub struct CurrentJob {
    pub job_id: i64,
    pub item_id: i64,
    pub token: CancelToken,
}

/// 应用核心
pub struct Core {
    pub paths: AppPaths,
    pub db: Db,
    pub vindex: VectorIndex,
    settings: RwLock<Settings>,
    http: reqwest::Client,
    sink: Arc<dyn EventSink>,
    /// 唤醒后台 Worker
    pub wake: Notify,
    chat_active: AtomicUsize,
    pub current_job: Mutex<Option<CurrentJob>>,
    /// 最近使用的"对话类"模型(文本/视觉),切换时卸载旧模型以节省内存
    last_chat_model: Mutex<Option<String>>,
    /// 当前问答的取消标志
    pub chat_cancel: Mutex<Option<CancelToken>>,
}

/// 问答活跃守卫:存活期间后台任务让路,避免与用户对话抢占 LLM
pub struct ChatGuard<'a>(&'a Core);

impl Drop for ChatGuard<'_> {
    fn drop(&mut self) {
        self.0.chat_active.fetch_sub(1, Ordering::SeqCst);
        self.0.wake.notify_one();
    }
}

impl Core {
    /// 创建核心:加载设置、载入向量索引、修复上次异常退出遗留的状态
    ///
    /// 参数: paths 数据目录;db 数据库;sink 事件通道
    /// 返回: 核心的 Arc
    pub fn new(paths: AppPaths, db: Db, sink: Arc<dyn EventSink>) -> Result<Arc<Core>> {
        let settings = db
            .with(|c| repo::get_setting(c, "app"))?
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .filter(|s| s.validate().is_ok())
            .unwrap_or_default();
        let core = Core {
            paths,
            db,
            vindex: VectorIndex::new(),
            http: build_http(),
            settings: RwLock::new(settings.clone()),
            sink,
            wake: Notify::new(),
            chat_active: AtomicUsize::new(0),
            current_job: Mutex::new(None),
            last_chat_model: Mutex::new(None),
            chat_cancel: Mutex::new(None),
        };
        let n = core.vindex.load_from_db(&core.db, &settings.embed_model)?;
        tracing::info!("向量索引已载入 {n} 条");
        crate::queue::recover_after_crash(&core.db)?;
        Ok(Arc::new(core))
    }

    /// 当前设置的副本
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// 校验并保存设置;嵌入模型变化时重建向量索引
    ///
    /// 参数: new 新设置
    /// 返回: 保存成功返回 Ok
    pub fn update_settings(&self, new: Settings) -> Result<()> {
        new.validate()?;
        let old = self.settings();
        self.db
            .with(|c| repo::set_setting(c, "app", &serde_json::to_string(&new)?))?;
        *self.settings.write().unwrap() = new.clone();
        if old.embed_model != new.embed_model {
            self.vindex.load_from_db(&self.db, &new.embed_model)?;
        }
        Ok(())
    }

    /// 获取 Ollama 客户端(按当前设置的地址)
    pub fn ollama(&self) -> OllamaClient {
        OllamaClient::new(self.http.clone(), &self.settings().ollama_url)
    }

    /// 构造生成参数(num_ctx 全局一致,避免 Ollama 反复重载)
    ///
    /// 参数: temperature 温度
    pub fn gen_opts(&self, temperature: f32) -> GenOptions {
        let s = self.settings();
        GenOptions {
            temperature,
            num_ctx: s.num_ctx,
            keep_alive: s.keep_alive,
        }
    }

    /// 发送事件
    pub fn emit(&self, event: &str, payload: Value) {
        self.sink.emit(event, payload);
    }

    /// 标记问答开始,返回守卫
    pub fn chat_guard(&self) -> ChatGuard<'_> {
        self.chat_active.fetch_add(1, Ordering::SeqCst);
        ChatGuard(self)
    }

    /// 当前是否有问答进行中
    pub fn chat_busy(&self) -> bool {
        self.chat_active.load(Ordering::SeqCst) > 0
    }

    /// 模型调度:准备使用某个"对话类"模型。若与上次使用的不同,先卸载上一个,
    /// 保证 16GB 内存下文本模型与视觉模型不会同时驻留。
    ///
    /// 参数: model 即将使用的模型
    pub async fn prepare_chat_model(&self, model: &str) {
        let previous = {
            let mut g = self.last_chat_model.lock().unwrap();
            let prev = g.clone();
            *g = Some(model.to_string());
            prev
        };
        if let Some(prev) = previous {
            if prev != model {
                self.ollama().unload(&prev).await;
            }
        }
    }

    /// 取消当前正在进行的问答
    pub fn cancel_chat(&self) {
        if let Some(t) = self.chat_cancel.lock().unwrap().as_ref() {
            t.cancel();
        }
    }
}

/// 测试辅助:创建基于内存库与临时目录的核心(集成测试也会使用)
#[doc(hidden)]
pub fn test_core(root: &std::path::Path, ollama_url: &str) -> Arc<Core> {
    let paths = AppPaths::new(root.to_path_buf()).unwrap();
    let db = Db::open_memory().unwrap();
    let core = Core::new(paths, db, Arc::new(NullSink)).unwrap();
    let mut s = core.settings();
    s.ollama_url = ollama_url.to_string();
    core.update_settings(s).unwrap();
    core
}
