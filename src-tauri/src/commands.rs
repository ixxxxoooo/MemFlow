// @author ygw
//! Tauri 命令层:对前端暴露的全部接口。所有参数做合法性校验,统一返回 Result。

use crate::chat::{self, ChatEvent};
use crate::core::{CancelToken, Core};
use crate::error::{AppError, Result};
use crate::ingest::{self, CaptureInput, CaptureResult};
use crate::model_fetch;
use crate::ollama::{self, PullProgress};
use crate::pipeline::model_installed;
use crate::queue::{self, QueueStatus};
use crate::repo::{
    self, ChunkView, ItemFilter, ItemView, MessageView, SessionView, Stats, TagCount,
};
use crate::settings::Settings;
use crate::windows;
use base64::Engine;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::{Channel, InvokeBody, Request};
use tauri::{AppHandle, State};

type S<'a> = State<'a, Arc<Core>>;

/// 当前模型拉取的取消令牌(同一时间只允许一个拉取)
static PULL_TOKEN: OnceLock<Mutex<Option<CancelToken>>> = OnceLock::new();

/// 模型需求项
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelNeed {
    pub role: String,
    pub name: String,
    pub installed: bool,
    pub required: bool,
    /// 是否为本地 Ollama 模型(云端模型此项为 false,不可下载)
    pub local: bool,
}

/// 应用整体状态
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub ollama_running: bool,
    pub ollama_version: Option<String>,
    pub installed_models: Vec<String>,
    pub models: Vec<ModelNeed>,
    pub stats: Stats,
    pub queue: QueueStatus,
    pub settings: Settings,
    pub data_dir: String,
    pub vector_count: usize,
    /// 是否已配置云端 API Key(密钥本身不会返回前端)
    pub has_api_key: bool,
    /// 已配置密钥的云端档案标识(含 default)
    pub keyed_profiles: Vec<String>,
    /// 已保存但最近一次读取失败的云端档案及原因(如用户拒绝了钥匙串授权),界面据此提示重新授权
    pub key_errors: Vec<KeyErrorView>,
}

/// 钥匙串读取失败信息
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyErrorView {
    pub profile: String,
    pub message: String,
}

/// 记录详情
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub item: ItemView,
    pub chunks: Vec<ChunkView>,
    /// 文档引用的图片(按出现顺序)
    pub images: Vec<repo::LinkedImage>,
    /// 图片被哪些文档引用
    pub parents: Vec<repo::ParentRef>,
    /// AI 整理的修订历史(新的在前),非空时可撤销
    pub revisions: Vec<repo::RevisionView>,
    /// 归并产物引用的来源笔记(按编号)
    pub merged_sources: Vec<repo::NoteBrief>,
    /// 本笔记已被归并到哪些主题笔记
    pub merged_into: Vec<repo::NoteBrief>,
}

/// 获取应用状态(Ollama、模型、统计、队列、设置)
#[tauri::command]
pub async fn app_status(core: S<'_>) -> Result<AppStatus> {
    let settings = core.settings();
    // 5 秒内复用探测结果,多处同时刷新状态也只访问一次 Ollama
    let probe = core.probe_ollama(false).await;
    let (running, version, installed) = (probe.running, probe.version, probe.installed);
    let cloud = settings.uses_cloud_llm();
    let mut models = vec![ModelNeed {
        role: "向量模型".into(),
        name: settings.embed_model.clone(),
        installed: model_installed(&installed, &settings.embed_model),
        required: true,
        local: true,
    }];
    if cloud {
        models.push(ModelNeed {
            role: "文本模型(云端)".into(),
            name: settings.llm_model.clone(),
            installed: core.has_api_key(),
            required: true,
            local: false,
        });
    } else {
        models.push(ModelNeed {
            role: "文本模型".into(),
            name: settings.text_model.clone(),
            installed: model_installed(&installed, &settings.text_model),
            required: true,
            local: true,
        });
    }
    models.push(ModelNeed {
        role: "视觉模型".into(),
        name: settings.vision_model.clone(),
        installed: model_installed(&installed, &settings.vision_model),
        required: settings.vision_enabled,
        local: true,
    });
    let stats = core
        .db
        .with_read(|c| repo::stats(c, &settings.embed_model))?;
    Ok(AppStatus {
        ollama_running: running,
        ollama_version: version,
        installed_models: installed,
        models,
        stats,
        queue: queue::queue_status(&core)?,
        data_dir: core.paths.root.to_string_lossy().into_owned(),
        vector_count: core.vindex.len(),
        has_api_key: core.has_api_key(),
        keyed_profiles: std::iter::once(crate::secrets::DEFAULT_PROFILE.to_string())
            .chain(settings.llm_profiles.iter().map(|p| p.id.clone()))
            .filter(|id| core.key_present(id))
            .collect(),
        key_errors: std::iter::once(crate::secrets::DEFAULT_PROFILE.to_string())
            .chain(settings.llm_profiles.iter().map(|p| p.id.clone()))
            .filter_map(|id| {
                core.key_error(&id).map(|message| KeyErrorView {
                    profile: id,
                    message,
                })
            })
            .collect(),
        settings,
    })
}

/// 尝试拉起本机 Ollama
#[tauri::command]
pub async fn ollama_start(core: S<'_>) -> Result<()> {
    ollama::try_start_ollama(&core.ollama()).await?;
    core.invalidate_probe();
    core.wake.notify_one();
    Ok(())
}

/// 占用"同一时间只允许一个下载"的名额
fn acquire_pull_token() -> Result<CancelToken> {
    let token = CancelToken::default();
    let mut g = PULL_TOKEN.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if g.is_some() {
        return Err(AppError::Invalid(
            "已有模型正在下载,请等待完成或先取消".into(),
        ));
    }
    *g = Some(token.clone());
    Ok(token)
}

/// 释放下载名额
fn release_pull_token() {
    if let Some(m) = PULL_TOKEN.get() {
        *m.lock().unwrap() = None;
    }
}

/// 校验模型名
fn check_model_name(name: &str) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.len() > 100 || name.chars().any(|c| c.is_whitespace()) {
        return Err(AppError::Invalid("模型名不合法".into()));
    }
    Ok(name)
}

/// 拉取模型(经由 Ollama 服务端,流式进度)
#[tauri::command]
pub async fn pull_model(
    core: S<'_>,
    name: String,
    on_progress: Channel<PullProgress>,
) -> Result<()> {
    let name = check_model_name(&name)?;
    let token = acquire_pull_token()?;
    let r = core
        .ollama()
        .pull(&name, &token.0, |p| {
            let _ = on_progress.send(p);
        })
        .await;
    release_pull_token();
    core.invalidate_probe();
    r?;
    core.wake.notify_one();
    Ok(())
}

/// 直连下载模型(绕过 Ollama 服务端的重定向检查,适用于代理 fake-ip 环境)
#[tauri::command]
pub async fn pull_model_direct(
    core: S<'_>,
    name: String,
    on_progress: Channel<PullProgress>,
) -> Result<()> {
    let name = check_model_name(&name)?;
    let token = acquire_pull_token()?;
    let registry = core.settings().model_registry;
    let r = model_fetch::pull_direct(
        &core.cloud_http(),
        &registry,
        &model_fetch::default_models_root(),
        &name,
        &token.0,
        |p| {
            let _ = on_progress.send(p);
        },
    )
    .await;
    release_pull_token();
    core.invalidate_probe();
    r?;
    core.wake.notify_one();
    Ok(())
}

/// 列出本地重排序模型及安装状态
#[tauri::command]
pub fn rerank_models(core: S<'_>) -> Vec<crate::rerank::RerankModelStatus> {
    crate::rerank::list_status(&core.paths.root)
}

/// 下载重排序模型(镜像优先,失败回退官方站点;断点续传 + sha256 校验)
#[tauri::command]
pub async fn download_rerank_model(
    core: S<'_>,
    id: String,
    on_progress: Channel<PullProgress>,
) -> Result<()> {
    let token = acquire_pull_token()?;
    let mirror = core.settings().hf_mirror;
    let r = crate::rerank::download(
        &core.cloud_http(),
        &mirror,
        &core.paths.root,
        &id,
        &token.0,
        |p| {
            let _ = on_progress.send(p);
        },
    )
    .await;
    release_pull_token();
    r?;
    // 下载完成后后台预热,首次提问就不用等加载
    let core2 = core.inner().clone();
    tauri::async_runtime::spawn(async move {
        let s = core2.settings();
        if s.rerank_enabled && s.rerank_model == id {
            if let Err(e) = core2.reranker.warm(&core2.paths.root, &id).await {
                tracing::warn!("重排序模型预热失败: {e}");
            }
        }
    });
    Ok(())
}

/// 保存云端 API Key 到系统钥匙串(传空串表示清除;profile_id 为空表示默认云端模型)
#[tauri::command]
pub fn set_llm_api_key(core: S<'_>, key: String, profile_id: Option<String>) -> Result<()> {
    if key.len() > 400 {
        return Err(AppError::Invalid("API Key 过长".into()));
    }
    let pid = profile_id.unwrap_or_else(|| crate::secrets::DEFAULT_PROFILE.to_string());
    let known = pid == crate::secrets::DEFAULT_PROFILE
        || core.settings().llm_profiles.iter().any(|p| p.id == pid)
        // 刚新增的档案可能尚未保存设置;只要标识合法也允许先写入密钥
        || (!pid.is_empty()
            && pid.len() <= 40
            && pid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    if !known {
        return Err(AppError::Invalid("云端模型档案标识不合法".into()));
    }
    core.set_key_for(&pid, &key)?;
    queue::emit_status(&core);
    Ok(())
}

/// 重新读取钥匙串中的密钥(用户在授权弹窗点过「拒绝」后,用它重新触发授权)
///
/// 参数: profile_id 档案标识,为空表示默认云端模型
/// 返回: 读取成功 Ok;失败返回原因(如授权被拒绝)
#[tauri::command]
pub fn retry_key_access(core: S<'_>, profile_id: Option<String>) -> Result<()> {
    let pid = profile_id.unwrap_or_else(|| crate::secrets::DEFAULT_PROFILE.to_string());
    let r = core.key_try(&pid, true);
    queue::emit_status(&core);
    match r {
        Ok(Some(_)) => {
            core.wake.notify_one();
            Ok(())
        }
        Ok(None) => Err(AppError::Invalid(
            "钥匙串中没有找到已保存的 API Key,请重新输入并保存".into(),
        )),
        Err(e) => Err(AppError::Invalid(e)),
    }
}

/// 测试云端模型连接(使用界面上尚未保存的地址与模型;key 为空则用已保存的密钥)
#[tauri::command]
pub async fn test_llm_connection(
    core: S<'_>,
    base_url: String,
    model: String,
    key: Option<String>,
    profile_id: Option<String>,
) -> Result<String> {
    let base = base_url.trim();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(AppError::Invalid(
            "云端接口地址必须以 http:// 或 https:// 开头".into(),
        ));
    }
    if model.trim().is_empty() {
        return Err(AppError::Invalid("云端模型名不能为空".into()));
    }
    // 用户主动测试时强制重试钥匙串(忽略后台冷却),读取失败要如实告知原因而不是说"未配置"
    let pid = profile_id
        .as_deref()
        .unwrap_or(crate::secrets::DEFAULT_PROFILE);
    let key = match key.filter(|k| !k.trim().is_empty()) {
        Some(k) => k,
        None => {
            let r = core.key_try(pid, true);
            // 无论成败都刷新状态,让界面同步"授权失败"提示
            queue::emit_status(&core);
            r.map_err(AppError::Invalid)?.unwrap_or_default()
        }
    };
    core.cloud_client(base, &key).ping(model.trim()).await
}

/// 取消模型拉取
#[tauri::command]
pub fn cancel_pull() {
    if let Some(m) = PULL_TOKEN.get() {
        if let Some(t) = m.lock().unwrap().as_ref() {
            t.cancel();
        }
    }
}

/// 读取设置
#[tauri::command]
pub fn get_settings(core: S<'_>) -> Settings {
    core.settings()
}

/// 保存设置(快捷键变化会同步重新注册,失败则整体回滚)
#[tauri::command]
pub fn update_settings(app: AppHandle, core: S<'_>, settings: Settings) -> Result<()> {
    settings.validate()?;
    let old = core.settings();
    if old.shortcut != settings.shortcut {
        windows::register_shortcut(&app, &settings.shortcut, Some(&old.shortcut))?;
    }
    if let Err(e) = core.update_settings(settings) {
        let _ = windows::register_shortcut(&app, &old.shortcut, None);
        return Err(e);
    }
    queue::emit_status(&core);
    Ok(())
}

/// 知识库列表
#[tauri::command]
pub fn list_kbs(core: S<'_>) -> Result<Vec<repo::KbView>> {
    crate::kb::list(&core)
}

/// 新建知识库
#[tauri::command]
pub fn create_kb(core: S<'_>, name: String, description: Option<String>) -> Result<repo::KbView> {
    crate::kb::create(&core, &name, description.as_deref().unwrap_or(""))
}

/// 重命名知识库
#[tauri::command]
pub fn rename_kb(core: S<'_>, id: i64, name: String, description: Option<String>) -> Result<()> {
    crate::kb::rename(&core, id, &name, description.as_deref())
}

/// 删除知识库(mode:moveToDefault 转移到默认库 / cascade 连同记录删除)
#[tauri::command]
pub async fn delete_kb(core: S<'_>, id: i64, mode: crate::kb::DeleteMode) -> Result<usize> {
    let core = core.inner().clone();
    crate::kb::delete(&core, id, mode).await
}

/// 把记录转移到另一个知识库
#[tauri::command]
pub fn move_items(core: S<'_>, ids: Vec<i64>, kb_id: i64) -> Result<crate::kb::MoveResult> {
    let core = core.inner().clone();
    crate::kb::move_items(&core, &ids, kb_id)
}

/// 生成单条笔记的 AI 整理方案(只预览,不写入)
#[tauri::command]
pub async fn organize_propose(
    core: S<'_>,
    id: i64,
    model_ref: Option<String>,
) -> Result<crate::organize::SingleProposal> {
    let core = core.inner().clone();
    crate::organize::propose_single(&core, id, model_ref.as_deref()).await
}

/// 确认单条整理(覆盖原文,旧版本进入修订历史)
#[tauri::command]
pub fn organize_apply(
    core: S<'_>,
    id: i64,
    title: String,
    content: String,
    base_updated_at: i64,
) -> Result<()> {
    let core = core.inner().clone();
    crate::organize::apply_single(&core, id, &title, &content, base_updated_at)
}

/// 撤销最近一次 AI 整理
#[tauri::command]
pub fn organize_undo(core: S<'_>, id: i64) -> Result<()> {
    let core = core.inner().clone();
    crate::organize::undo_single(&core, id)
}

/// 按向量相似度给出可归并的碎片分组(不调用模型)
#[tauri::command]
pub fn organize_suggest(
    core: S<'_>,
    options: Option<crate::organize::GroupOptions>,
) -> Result<Vec<crate::organize::GroupSuggestion>> {
    crate::organize::suggest_groups(&core, &options.unwrap_or_default())
}

/// "可整理"提示:每条记录对应的高度相似碎片
#[tauri::command]
pub fn organize_hints(
    core: S<'_>,
    ids: Vec<i64>,
) -> Result<std::collections::HashMap<i64, Vec<i64>>> {
    crate::organize::peer_hints(&core, &ids)
}

/// 生成归并方案(一次模型调用,只预览,不写入)
#[tauri::command]
pub async fn organize_merge_propose(
    core: S<'_>,
    ids: Vec<i64>,
    model_ref: Option<String>,
) -> Result<crate::organize::MergeProposal> {
    let core = core.inner().clone();
    crate::organize::propose_merge(&core, &ids, model_ref.as_deref()).await
}

/// 确认归并:新建主题笔记并关联来源,返回新笔记 id
#[tauri::command]
pub fn organize_merge_apply(
    core: S<'_>,
    ids: Vec<i64>,
    title: String,
    content: String,
) -> Result<i64> {
    let core = core.inner().clone();
    crate::organize::apply_merge(&core, &ids, &title, &content)
}

/// 闪念提交(文字/图片/文档)
#[tauri::command]
pub async fn capture_submit(core: S<'_>, input: CaptureInput) -> Result<CaptureResult> {
    ingest::capture(&core, input).await
}

/// 暂存剪贴板中的二进制文件(原始字节通过请求体传入,文件名在 x-filename 头中)
#[tauri::command]
pub async fn stage_file(core: S<'_>, request: Request<'_>) -> Result<String> {
    let name = request
        .headers()
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("paste.png")
        .to_string();
    match request.body() {
        InvokeBody::Raw(bytes) => ingest::stage_bytes(&core, &name, bytes.clone()).await,
        _ => Err(AppError::Invalid("请求体必须是二进制数据".into())),
    }
}

/// 记录列表
#[tauri::command]
pub fn list_items(core: S<'_>, filter: ItemFilter) -> Result<Vec<ItemView>> {
    core.db.with_read(|c| repo::list_items(c, &filter))
}

/// 记录详情(含完整原文与切片)
#[tauri::command]
pub fn get_item(core: S<'_>, id: i64) -> Result<ItemDetail> {
    core.db.with_read(|c| {
        let item = repo::get_item(c, id)?.ok_or_else(|| AppError::Invalid("记录不存在".into()))?;
        let chunks = repo::list_chunks(c, id)?;
        let images = repo::images_of(c, id)?;
        let parents = repo::parents_of(c, id)?;
        let revisions = repo::list_revisions(c, id)?;
        let merged_sources = repo::merged_sources(c, id)?;
        let merged_into = repo::merged_into(c, id)?;
        Ok(ItemDetail {
            item,
            chunks,
            images,
            parents,
            revisions,
            merged_sources,
            merged_into,
        })
    })
}

/// 标签统计
#[tauri::command]
pub fn list_tags(core: S<'_>) -> Result<Vec<TagCount>> {
    core.db.with_read(repo::tag_counts)
}

/// 编辑原文(会触发重新解析)
#[tauri::command]
pub fn update_item_content(core: S<'_>, id: i64, content: String) -> Result<()> {
    ingest::update_content(&core, id, &content)
}

/// 删除记录
#[tauri::command]
pub async fn delete_items(core: S<'_>, ids: Vec<i64>) -> Result<usize> {
    if ids.is_empty() {
        return Err(AppError::Invalid("未选择记录".into()));
    }
    ingest::delete_items(&core, &ids).await
}

/// 重刷(ids 为空数组表示全库)
#[tauri::command]
pub fn reindex(core: S<'_>, ids: Option<Vec<i64>>) -> Result<usize> {
    ingest::reindex(&core, ids.filter(|v| !v.is_empty()))
}

/// AI 解读(强制调用模型生成摘要/标签/实体)
#[tauri::command]
pub fn reindex_ai(core: S<'_>, ids: Vec<i64>) -> Result<usize> {
    ingest::reindex_ai(&core, ids)
}

/// 重试失败任务
#[tauri::command]
pub fn retry_failed(core: S<'_>, ids: Option<Vec<i64>>) -> Result<usize> {
    queue::retry_failed(&core, ids.filter(|v| !v.is_empty()))
}

/// 取消某记录的处理
#[tauri::command]
pub fn cancel_item(core: S<'_>, id: i64) -> Result<()> {
    queue::cancel_item(&core, id)
}

/// 问答(通过 Channel 流式推送事件)
#[tauri::command]
pub async fn chat_ask(
    core: S<'_>,
    session_id: Option<i64>,
    question: String,
    model_ref: Option<String>,
    kb_ids: Option<Vec<i64>>,
    on_event: Channel<ChatEvent>,
) -> Result<()> {
    let core = core.inner().clone();
    chat::ask_in(
        &core,
        session_id,
        &question,
        model_ref.as_deref(),
        kb_ids.as_deref(),
        &|ev| {
            let _ = on_event.send(ev);
        },
    )
    .await
}

/// 对话可选模型(本地 Ollama 文本模型 + 已配置密钥的云端模型)
#[tauri::command]
pub async fn list_chat_models(core: S<'_>) -> Result<Vec<crate::chat_models::ChatModelOption>> {
    Ok(crate::chat_models::list_chat_models(&core).await)
}

/// 预热对话与向量模型(进入问答页或切换模型时调用,后台执行不阻塞界面)
#[tauri::command]
pub async fn warm_chat(core: S<'_>, model_ref: Option<String>) -> Result<()> {
    crate::chat_models::warm(&core, model_ref.as_deref()).await;
    Ok(())
}

/// 停止当前问答
#[tauri::command]
pub fn chat_cancel(core: S<'_>) {
    core.cancel_chat();
}

/// 会话列表
#[tauri::command]
pub fn list_sessions(core: S<'_>) -> Result<Vec<SessionView>> {
    core.db.with_read(repo::list_sessions)
}

/// 会话消息
#[tauri::command]
pub fn get_session_messages(core: S<'_>, session_id: i64) -> Result<Vec<MessageView>> {
    core.db.with_read(|c| repo::list_messages(c, session_id))
}

/// 删除会话
#[tauri::command]
pub fn delete_session(core: S<'_>, session_id: i64) -> Result<()> {
    core.db.with(|c| repo::delete_session(c, session_id))
}

/// 修复 Mermaid 语法
#[tauri::command]
pub async fn fix_mermaid(core: S<'_>, code: String, error: String) -> Result<String> {
    if code.trim().is_empty() {
        return Err(AppError::Invalid("代码为空".into()));
    }
    chat::fix_mermaid(&core, &code, &error).await
}

/// 把前端生成的导出文件(SVG/PNG)写入用户选择的路径
#[tauri::command]
pub async fn write_export(path: String, data_base64: String) -> Result<()> {
    let p = PathBuf::from(&path);
    if !p.is_absolute() {
        return Err(AppError::Invalid("路径必须是绝对路径".into()));
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !matches!(ext.as_str(), "svg" | "png" | "md" | "txt") {
        return Err(AppError::Invalid("仅支持导出 svg / png / md / txt".into()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| AppError::Invalid(format!("数据编码错误: {e}")))?;
    tokio::fs::write(&p, bytes).await?;
    Ok(())
}

/// 递归复制目录
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// 备份:一致性快照数据库 + 附件目录
///
/// 参数: dest_dir 目标目录
/// 返回: 备份目录路径
#[tauri::command]
pub async fn export_backup(core: S<'_>, dest_dir: String) -> Result<String> {
    let dest_root = PathBuf::from(&dest_dir);
    if !dest_root.is_dir() {
        return Err(AppError::Invalid("目标目录不存在".into()));
    }
    let dest = dest_root.join(format!(
        "MemFlow-backup-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    std::fs::create_dir_all(&dest)?;
    let db_file = dest.join("memflow.db");
    core.db.with(|c| {
        c.execute("VACUUM INTO ?1", [db_file.to_string_lossy().as_ref()])?;
        Ok(())
    })?;
    let (src, dst) = (core.paths.attachments.clone(), dest.join("attachments"));
    tokio::task::spawn_blocking(move || copy_dir(&src, &dst))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    Ok(dest.to_string_lossy().into_owned())
}

/// 显示主工作台
#[tauri::command]
pub fn show_main(app: AppHandle) {
    windows::show_main(&app);
}

/// 显示闪念胶囊
#[tauri::command]
pub fn show_capsule(app: AppHandle) {
    windows::show_capsule(&app);
}

/// 隐藏闪念胶囊
#[tauri::command]
pub fn hide_capsule(app: AppHandle) {
    windows::hide_capsule(&app);
}

/// 调整胶囊高度(显示附件预览时增高)
#[tauri::command]
pub fn resize_capsule(app: AppHandle, height: f64) {
    windows::resize_capsule(&app, height.clamp(72.0, 420.0));
}

/// 在屏幕右下角显示轻量提示
#[tauri::command]
pub fn notify_toast(app: AppHandle, message: String) {
    let msg: String = message.chars().take(60).collect();
    windows::show_toast(&app, &msg);
}

/// 记录前端错误到日志文件
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    crate::logging::log_frontend(&level, &message);
}

/// 用系统文件管理器打开日志目录
#[tauri::command]
pub fn open_log_dir() -> Result<String> {
    let dir = crate::logging::log_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(&dir).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer").arg(&dir).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    std::process::Command::new("xdg-open").arg(&dir).spawn()?;
    Ok(dir.to_string_lossy().into_owned())
}
