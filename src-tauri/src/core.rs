// @author ygw
//! 应用核心上下文:聚合数据库、向量索引、设置、Ollama 客户端与事件通道。
//! 与 Tauri 解耦(通过 EventSink 抽象),便于集成测试。

use crate::db::Db;
use crate::error::Result;
use crate::llm::LlmClient;
use crate::ollama::{build_http, GenOptions, OllamaClient};
use crate::openai::{build_cloud_http, OpenAiClient};
use crate::repo;
use crate::secrets;
use crate::settings::Settings;
use crate::vector::VectorIndex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

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

/// 入库流水线的分阶段并发限制。
///
/// 多个任务同时处理时,各阶段的资源占用不同,用信号量分别限流,避免互相抢占:
/// - cpu:文档解析/OCR/分块(吃 CPU)
/// - embed:向量化(Ollama 并行度有限)
/// - llm_local:本地文本/视觉模型(内存大户,串行)
/// - llm_cloud:云端文本模型(远程服务,可并发)
pub struct Stages {
    pub cpu: Arc<Semaphore>,
    pub embed: Arc<Semaphore>,
    pub llm_local: Arc<Semaphore>,
    pub llm_cloud: Arc<Semaphore>,
}

/// 向量化阶段并发上限
pub const EMBED_CONCURRENCY: usize = 2;
/// 云端模型阶段并发上限
pub const CLOUD_LLM_CONCURRENCY: usize = 4;

impl Stages {
    /// 按 CPU 核数创建
    pub fn new() -> Stages {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        Stages {
            cpu: Arc::new(Semaphore::new((cores / 2).clamp(1, 4))),
            embed: Arc::new(Semaphore::new(EMBED_CONCURRENCY)),
            llm_local: Arc::new(Semaphore::new(1)),
            llm_cloud: Arc::new(Semaphore::new(CLOUD_LLM_CONCURRENCY)),
        }
    }
}

impl Default for Stages {
    fn default() -> Self {
        Self::new()
    }
}

/// 获取某个阶段的并发许可(信号量被关闭时返回错误,正常运行中不会发生)
pub async fn permit(sem: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit> {
    sem.clone()
        .acquire_owned()
        .await
        .map_err(|_| crate::error::AppError::Other("并发控制已关闭".into()))
}

/// 自动并发度:物理核数的一半,限制在 1~4
pub fn auto_import_concurrency() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    (cores / 2).clamp(1, 4)
}

/// Ollama 探测结果(版本与已安装模型)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaProbe {
    pub running: bool,
    pub version: Option<String>,
    pub installed: Vec<String>,
}

/// 探测结果缓存时长:状态接口被多处高频调用,5 秒内复用
const PROBE_TTL: std::time::Duration = std::time::Duration::from_secs(5);
/// 后台观察 Ollama 状态变化的间隔
/// 钥匙串读取失败后,后台任务的重试冷却(用户主动操作不受限),避免队列反复弹授权窗口
const KEY_RETRY_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);
const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// 应用核心
pub struct Core {
    pub paths: AppPaths,
    pub db: Db,
    pub vindex: VectorIndex,
    settings: RwLock<Settings>,
    http: reqwest::Client,
    /// 云端模型专用 HTTP 客户端(遵循系统代理)
    cloud_http: reqwest::Client,
    /// API Key 缓存:外层 None 表示尚未读取过钥匙串
    api_keys: Mutex<HashMap<String, Option<String>>>,
    /// 密钥是否存在的缓存(只查存在性,不读内容,不触发钥匙串授权)
    key_presence: Mutex<HashMap<String, bool>>,
    /// 最近一次钥匙串读取失败的原因与时间(读取成功后清除);失败不会被当作"未保存"缓存
    key_errors: Mutex<HashMap<String, (String, std::time::Instant)>>,
    /// Ollama 探测缓存:(地址, 时间, 结果)
    probe_cache: Mutex<Option<(String, std::time::Instant, OllamaProbe)>>,
    /// 查询向量缓存(容量 64)
    pub query_vecs: Mutex<crate::query_cache::QueryCache>,
    sink: Arc<dyn EventSink>,
    /// 唤醒后台 Worker
    pub wake: Notify,
    chat_active: AtomicUsize,
    /// 正在执行的任务(多 worker 并发时可能有多个)
    pub current_jobs: Mutex<Vec<CurrentJob>>,
    /// 入库流水线分阶段限流
    pub stages: Stages,
    /// 最近使用的"对话类"模型(文本/视觉),切换时卸载旧模型以节省内存
    last_chat_model: Mutex<Option<String>>,
    /// 当前问答的取消标志
    pub chat_cancel: Mutex<Option<CancelToken>>,
    /// 本地重排序器(懒加载、常驻)
    pub reranker: crate::rerank::Reranker,
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
            cloud_http: build_cloud_http(),
            api_keys: Mutex::new(HashMap::new()),
            key_presence: Mutex::new(HashMap::new()),
            key_errors: Mutex::new(HashMap::new()),
            probe_cache: Mutex::new(None),
            query_vecs: Mutex::new(crate::query_cache::QueryCache::new(64)),
            settings: RwLock::new(settings.clone()),
            sink,
            wake: Notify::new(),
            chat_active: AtomicUsize::new(0),
            current_jobs: Mutex::new(Vec::new()),
            stages: Stages::new(),
            last_chat_model: Mutex::new(None),
            chat_cancel: Mutex::new(None),
            reranker: crate::rerank::Reranker::default(),
        };
        let n = core.vindex.load_from_db(&core.db, &settings.embed_model)?;
        tracing::info!("向量索引已载入 {n} 条");
        crate::queue::recover_after_crash(&core.db)?;
        Ok(Arc::new(core))
    }

    /// 实际生效的入库并发 worker 数(设置为 0 表示自动)
    pub fn import_concurrency(&self) -> usize {
        match self.settings().import_concurrency {
            0 => auto_import_concurrency(),
            n => n as usize,
        }
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
        self.invalidate_probe();
        if old.embed_model != new.embed_model {
            self.vindex.load_from_db(&self.db, &new.embed_model)?;
        }
        // 关闭重排序或切换模型后释放旧会话(约数百 MB 内存)
        if !new.rerank_enabled || old.rerank_model != new.rerank_model {
            self.reranker.unload();
        }
        Ok(())
    }

    /// 探测 Ollama 状态与已安装模型;`force` 为 false 时 5 秒内复用缓存
    pub async fn probe_ollama(&self, force: bool) -> OllamaProbe {
        let url = self.settings().ollama_url;
        if !force {
            if let Some((u, at, p)) = self.probe_cache.lock().unwrap().as_ref() {
                if *u == url && at.elapsed() < PROBE_TTL {
                    return p.clone();
                }
            }
        }
        let client = self.ollama();
        let probe = match client.version().await {
            Ok(v) => OllamaProbe {
                running: true,
                version: Some(v),
                installed: client
                    .list_models()
                    .await
                    .map(|m| m.into_iter().map(|x| x.name).collect())
                    .unwrap_or_default(),
            },
            Err(_) => OllamaProbe {
                running: false,
                version: None,
                installed: vec![],
            },
        };
        *self.probe_cache.lock().unwrap() = Some((url, std::time::Instant::now(), probe.clone()));
        probe
    }

    /// 让探测缓存失效(拉取/启动/改地址后调用,下一次状态查询会重新探测)
    pub fn invalidate_probe(&self) {
        *self.probe_cache.lock().unwrap() = None;
    }

    /// 后台观察 Ollama 状态,仅在变化时推送 `ollama-status` 事件,前端据此刷新,
    /// 这样前端不必高频轮询。
    pub async fn watch_ollama(self: Arc<Self>) {
        let mut last: Option<OllamaProbe> = None;
        loop {
            let p = self.probe_ollama(true).await;
            if last.as_ref() != Some(&p) {
                if last.is_some() {
                    self.emit(
                        "ollama-status",
                        serde_json::to_value(&p).unwrap_or_default(),
                    );
                }
                last = Some(p);
            }
            tokio::time::sleep(WATCH_INTERVAL).await;
        }
    }

    /// 获取 Ollama 客户端(按当前设置的地址)
    pub fn ollama(&self) -> OllamaClient {
        OllamaClient::new(self.http.clone(), &self.settings().ollama_url)
    }

    /// 某云端档案的 API Key(首次访问时从钥匙串读取并缓存)
    ///
    /// 参数: profile 档案标识,default 为默认云端模型
    /// 读取失败(如用户拒绝授权)返回 None,但不会缓存失败,下次仍会重试
    pub fn key_for(&self, profile: &str) -> Option<String> {
        self.key_try(profile, false).ok().flatten()
    }

    /// 读取密钥并区分"没有"与"读取失败"
    ///
    /// 参数: profile 档案标识;force 为 true 时忽略失败冷却,立即重试(用户主动操作时使用)
    /// 返回: Ok(Some) 有密钥;Ok(None) 确实未保存;Err 为读取失败的原因。
    /// 后台任务读取失败后 30 秒内不会再次触发钥匙串弹窗,避免队列重试时反复打扰用户。
    pub fn key_try(
        &self,
        profile: &str,
        force: bool,
    ) -> std::result::Result<Option<String>, String> {
        if let Some(v) = self.api_keys.lock().unwrap().get(profile) {
            return Ok(v.clone());
        }
        if !force {
            if let Some((msg, at)) = self.key_errors.lock().unwrap().get(profile) {
                if at.elapsed() < KEY_RETRY_COOLDOWN {
                    return Err(msg.clone());
                }
            }
        }
        match secrets::load_api_key(profile) {
            secrets::KeyLoad::Found(k) => {
                self.key_errors.lock().unwrap().remove(profile);
                self.api_keys
                    .lock()
                    .unwrap()
                    .insert(profile.to_string(), Some(k.clone()));
                Ok(Some(k))
            }
            secrets::KeyLoad::Missing => {
                self.key_errors.lock().unwrap().remove(profile);
                self.api_keys
                    .lock()
                    .unwrap()
                    .insert(profile.to_string(), None);
                Ok(None)
            }
            secrets::KeyLoad::Failed(msg) => {
                self.key_errors.lock().unwrap().insert(
                    profile.to_string(),
                    (msg.clone(), std::time::Instant::now()),
                );
                Err(msg)
            }
        }
    }

    /// 某档案最近一次钥匙串读取失败的原因(无失败返回 None)
    pub fn key_error(&self, profile: &str) -> Option<String> {
        self.key_errors
            .lock()
            .unwrap()
            .get(profile)
            .map(|(m, _)| m.clone())
    }

    /// 某档案是否已保存密钥。只查存在性、不读取内容,因此不会弹出钥匙串授权窗口;
    /// 界面状态与模型列表应使用它,真正调用云端时才用 `key_for` 读取密钥。
    pub fn key_present(&self, profile: &str) -> bool {
        if let Some(v) = self.api_keys.lock().unwrap().get(profile) {
            return v.is_some();
        }
        let mut g = self.key_presence.lock().unwrap();
        *g.entry(profile.to_string())
            .or_insert_with(|| secrets::key_exists(profile))
    }

    /// 默认云端模型的 API Key
    pub fn api_key(&self) -> Option<String> {
        self.key_for(secrets::DEFAULT_PROFILE)
    }

    /// 是否已配置默认云端模型的 API Key
    pub fn has_api_key(&self) -> bool {
        self.key_present(secrets::DEFAULT_PROFILE)
    }

    /// 保存某档案的 API Key 到钥匙串并更新缓存;传入空串表示清除
    ///
    /// 参数: profile 档案标识;key 密钥
    pub fn set_key_for(&self, profile: &str, key: &str) -> Result<()> {
        secrets::save_api_key(profile, key)?;
        self.key_errors.lock().unwrap().remove(profile);
        self.set_key_memory(profile, key);
        self.key_presence
            .lock()
            .unwrap()
            .insert(profile.to_string(), !key.trim().is_empty());
        Ok(())
    }

    /// 保存默认云端模型的 API Key
    pub fn set_api_key(&self, key: &str) -> Result<()> {
        self.set_key_for(secrets::DEFAULT_PROFILE, key)
    }

    /// 仅更新内存缓存,不触碰钥匙串(测试使用)
    #[doc(hidden)]
    pub fn set_key_memory(&self, profile: &str, key: &str) {
        let v = (!key.trim().is_empty()).then(|| key.trim().to_string());
        self.api_keys.lock().unwrap().insert(profile.to_string(), v);
    }

    /// 仅更新默认档案的内存缓存(测试使用)
    #[doc(hidden)]
    pub fn set_api_key_memory(&self, key: &str) {
        self.set_key_memory(secrets::DEFAULT_PROFILE, key);
    }

    /// 云端/下载用 HTTP 客户端(遵循系统代理,无总超时)
    pub fn cloud_http(&self) -> reqwest::Client {
        self.cloud_http.clone()
    }

    /// 构造指定地址的云端客户端(用于"测试连接",此时设置尚未保存)
    ///
    /// 参数: base 接口根地址;key 密钥
    pub fn cloud_client(&self, base: &str, key: &str) -> OpenAiClient {
        OpenAiClient::new(self.cloud_http.clone(), base, key)
    }

    /// 获取文本对话后端:云端模式走 OpenAI 兼容接口,否则走本地 Ollama
    pub fn llm(&self) -> LlmClient {
        let s = self.settings();
        if s.uses_cloud_llm() {
            let (key, why) = match self.key_try(secrets::DEFAULT_PROFILE, false) {
                Ok(k) => (k.unwrap_or_default(), None),
                Err(e) => (String::new(), Some(e)),
            };
            let mut c = OpenAiClient::new(self.cloud_http.clone(), &s.llm_base_url, &key);
            if let Some(w) = why {
                c = c.with_missing_key_reason(w);
            }
            LlmClient::OpenAi(c)
        } else {
            LlmClient::Ollama(self.ollama())
        }
    }

    /// 当前文本类调用使用的模型名
    pub fn text_model(&self) -> String {
        self.settings().active_text_model().to_string()
    }

    /// 文本模型调度:仅本地 Ollama 需要处理内存驻留,云端模式直接跳过
    pub async fn prepare_text_model(&self) {
        let s = self.settings();
        if !s.uses_cloud_llm() {
            self.prepare_chat_model(&s.text_model).await;
        }
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
