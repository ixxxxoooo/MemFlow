// @author ixxxxoooo
//! Ollama REST 客户端:状态检测、模型管理、向量化、对话(流式/结构化)、拉取与卸载。
//!
//! 健壮性约定:
//! - 连接失败/超时统一映射为 `OllamaUnavailable`(临时性错误)
//! - HTTP 404 映射为 `ModelMissing`
//! - 流式读取带空闲超时与取消标志

use crate::error::{AppError, Result};
use crate::vector;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 非流式请求默认超时(7B 模型在忙碌时可能较慢)
const CHAT_TIMEOUT: Duration = Duration::from_secs(300);
/// 向量化请求超时
const EMBED_TIMEOUT: Duration = Duration::from_secs(180);
/// 流式读取的空闲超时(多久没收到任何数据视为卡死)
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// 对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// 角色:system / user / assistant
    pub role: String,
    /// 文本内容
    pub content: String,
    /// 图片(base64),仅视觉模型使用
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub images: Option<Vec<String>>,
}

impl ChatMessage {
    /// 构造文本消息
    ///
    /// 参数: role 角色;content 内容
    /// 返回: 消息
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            images: None,
        }
    }
}

/// 本地已安装模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// 模型全名,如 qwen2.5:7b
    pub name: String,
    /// 体积(字节)
    #[serde(default)]
    pub size: u64,
}

/// 模型拉取进度
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullProgress {
    /// 阶段描述
    pub status: String,
    /// 总字节数
    pub total: u64,
    /// 已完成字节数
    pub completed: u64,
}

/// 生成参数
#[derive(Debug, Clone)]
pub struct GenOptions {
    /// 温度
    pub temperature: f32,
    /// 上下文长度
    pub num_ctx: u32,
    /// 模型常驻时长
    pub keep_alive: String,
}

/// Ollama 客户端
#[derive(Clone)]
pub struct OllamaClient {
    http: reqwest::Client,
    base: String,
}

/// 创建共享的 HTTP 客户端(仅连接超时,总超时按请求设置)
///
/// 返回: reqwest 客户端
pub fn build_http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
        .expect("创建 HTTP 客户端失败")
}

impl OllamaClient {
    /// 创建客户端
    ///
    /// 参数: http 共享客户端;base 服务地址
    pub fn new(http: reqwest::Client, base: &str) -> Self {
        OllamaClient {
            http,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// 把 reqwest 错误映射为应用错误
    fn map_err(e: reqwest::Error) -> AppError {
        if e.is_connect() || e.is_timeout() || e.is_request() {
            AppError::OllamaUnavailable(e.to_string())
        } else {
            AppError::Ollama(e.to_string())
        }
    }

    /// 检查响应状态,非 2xx 时读取错误体并映射
    async fn check(resp: reqwest::Response, model: &str) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str().map(String::from)))
            .unwrap_or(body);
        if status.as_u16() == 404 || msg.contains("not found") {
            return Err(AppError::ModelMissing(model.to_string()));
        }
        Err(AppError::Ollama(format!(
            "HTTP {}: {}",
            status.as_u16(),
            msg
        )))
    }

    /// 查询服务版本(用于探活)
    ///
    /// 返回: 版本号;服务不可用时返回 OllamaUnavailable
    pub async fn version(&self) -> Result<String> {
        let resp = self
            .http
            .get(self.url("/api/version"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(Self::map_err)?;
        let resp = Self::check(resp, "").await?;
        let v: Value = resp.json().await.map_err(Self::map_err)?;
        Ok(v.get("version")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    /// 列出本地已安装模型
    ///
    /// 返回: 模型列表
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let resp = self
            .http
            .get(self.url("/api/tags"))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(Self::map_err)?;
        let resp = Self::check(resp, "").await?;
        let v: Value = resp.json().await.map_err(Self::map_err)?;
        let models = v
            .get("models")
            .cloned()
            .map(serde_json::from_value::<Vec<ModelInfo>>)
            .transpose()?
            .unwrap_or_default();
        Ok(models)
    }

    /// 批量向量化,结果已做 L2 归一化并校验合法性
    ///
    /// 参数: model 嵌入模型;inputs 文本列表;keep_alive 常驻时长
    /// 返回: 与输入等长的向量列表
    pub async fn embed(
        &self,
        model: &str,
        inputs: &[String],
        keep_alive: &str,
    ) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        let body =
            json!({ "model": model, "input": inputs, "truncate": true, "keep_alive": keep_alive });
        let resp = self
            .http
            .post(self.url("/api/embed"))
            .timeout(EMBED_TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_err)?;
        let resp = Self::check(resp, model).await?;
        let v: Value = resp.json().await.map_err(Self::map_err)?;
        let arr = v
            .get("embeddings")
            .and_then(|e| e.as_array())
            .ok_or_else(|| AppError::Ollama("embed 响应缺少 embeddings".into()))?;
        if arr.len() != inputs.len() {
            return Err(AppError::Ollama(format!(
                "embed 返回 {} 条,期望 {} 条",
                arr.len(),
                inputs.len()
            )));
        }
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let mut vec: Vec<f32> = item
                .as_array()
                .ok_or_else(|| AppError::Ollama("embeddings 元素不是数组".into()))?
                .iter()
                .map(|x| x.as_f64().unwrap_or(f64::NAN) as f32)
                .collect();
            if !vector::is_valid(&vec) {
                return Err(AppError::Ollama("embed 返回了非法向量".into()));
            }
            vector::normalize(&mut vec);
            out.push(vec);
        }
        Ok(out)
    }

    /// 构造 chat 请求体
    fn chat_body(
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        stream: bool,
        format: Option<&Value>,
    ) -> Value {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": stream,
            "keep_alive": opts.keep_alive,
            "options": { "temperature": opts.temperature, "num_ctx": opts.num_ctx },
        });
        if let Some(f) = format {
            body["format"] = f.clone();
        }
        body
    }

    /// 非流式对话,可选 JSON Schema 约束输出
    ///
    /// 参数: model 模型;messages 消息;opts 生成参数;format 可选 JSON Schema
    /// 返回: 模型输出的完整文本
    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        format: Option<&Value>,
    ) -> Result<String> {
        let body = Self::chat_body(model, messages, opts, false, format);
        let resp = self
            .http
            .post(self.url("/api/chat"))
            .timeout(CHAT_TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_err)?;
        let resp = Self::check(resp, model).await?;
        let v: Value = resp.json().await.map_err(Self::map_err)?;
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            return Err(AppError::Ollama(e.to_string()));
        }
        v.get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(String::from)
            .ok_or_else(|| AppError::Ollama("chat 响应缺少 message.content".into()))
    }

    /// 结构化对话:要求模型按 JSON Schema 输出并反序列化
    ///
    /// 参数: 同 chat,T 为目标类型
    /// 返回: 解析后的结构;JSON 非法时返回 Parse 错误
    pub async fn chat_json<T: serde::de::DeserializeOwned>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        schema: &Value,
    ) -> Result<T> {
        let text = self.chat(model, messages, opts, Some(schema)).await?;
        let cleaned = strip_code_fence(&text);
        serde_json::from_str::<T>(cleaned)
            .map_err(|e| AppError::Parse(format!("模型 JSON 非法: {e}")))
    }

    /// 流式对话:逐 token 回调,支持取消
    ///
    /// 参数: model 模型;messages 消息;opts 参数;cancel 取消标志;on_token 每个文本片段的回调
    /// 返回: 完整文本(被取消时返回 Cancelled)
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        cancel: &AtomicBool,
        mut on_token: impl FnMut(&str),
    ) -> Result<String> {
        let body = Self::chat_body(model, messages, opts, true, None);
        let resp = self
            .http
            .post(self.url("/api/chat"))
            .json(&body)
            .send()
            .await
            .map_err(Self::map_err)?;
        let resp = Self::check(resp, model).await?;
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut full = String::new();
        let mut last_data = std::time::Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let next = tokio::time::timeout(Duration::from_millis(500), stream.next()).await;
            let chunk = match next {
                // 500ms 无数据:继续循环以便及时响应取消,同时检查总体空闲超时
                Err(_) => {
                    if last_data.elapsed() > STREAM_IDLE_TIMEOUT {
                        return Err(AppError::OllamaUnavailable("模型长时间无输出".into()));
                    }
                    continue;
                }
                Ok(None) => break,
                Ok(Some(c)) => c.map_err(Self::map_err)?,
            };
            last_data = std::time::Instant::now();
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let v: Value = serde_json::from_str(line)
                    .map_err(|e| AppError::Ollama(format!("流数据非法: {e}")))?;
                if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
                    return Err(AppError::Ollama(e.to_string()));
                }
                if let Some(t) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    if !t.is_empty() {
                        full.push_str(t);
                        on_token(t);
                    }
                }
                if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                    return Ok(full);
                }
            }
        }
        Ok(full)
    }

    /// 拉取模型,流式回调进度
    ///
    /// 参数: model 模型名;cancel 取消标志;on_progress 进度回调
    /// 返回: 成功返回 Ok
    pub async fn pull(
        &self,
        model: &str,
        cancel: &AtomicBool,
        mut on_progress: impl FnMut(PullProgress),
    ) -> Result<()> {
        let resp = self
            .http
            .post(self.url("/api/pull"))
            .json(&json!({ "model": model, "stream": true }))
            .send()
            .await
            .map_err(Self::map_err)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Ollama(format!(
                "拉取失败 HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let next = tokio::time::timeout(Duration::from_secs(60), stream.next()).await;
            let chunk = match next {
                Err(_) => return Err(AppError::OllamaUnavailable("模型拉取长时间无响应".into())),
                Ok(None) => break,
                Ok(Some(c)) => c.map_err(Self::map_err)?,
            };
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let v: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
                    return Err(AppError::Ollama(e.to_string()));
                }
                on_progress(PullProgress {
                    status: v
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    total: v.get("total").and_then(|x| x.as_u64()).unwrap_or(0),
                    completed: v.get("completed").and_then(|x| x.as_u64()).unwrap_or(0),
                });
            }
        }
        Ok(())
    }

    /// 预热:让 Ollama 提前把模型载入内存(空请求不生成文字),失败忽略
    ///
    /// 参数: model 模型名;keep_alive 常驻时长;num_ctx 上下文长度(需与后续请求一致,否则会重载)
    pub async fn preload(&self, model: &str, keep_alive: &str, num_ctx: u32) {
        let r = self
            .http
            .post(self.url("/api/generate"))
            .timeout(Duration::from_secs(120))
            .json(&json!({
                "model": model,
                "keep_alive": keep_alive,
                "options": { "num_ctx": num_ctx },
            }))
            .send()
            .await;
        if let Err(e) = r {
            tracing::debug!("预热模型 {model} 失败: {e}");
        }
    }

    /// 立即卸载模型释放内存(失败忽略,仅记录日志)
    ///
    /// 参数: model 模型名
    pub async fn unload(&self, model: &str) {
        let r = self
            .http
            .post(self.url("/api/generate"))
            .timeout(Duration::from_secs(10))
            .json(&json!({ "model": model, "keep_alive": 0 }))
            .send()
            .await;
        if let Err(e) = r {
            tracing::debug!("卸载模型 {model} 失败: {e}");
        }
    }
}

/// 去掉模型可能包裹的 ``` 代码围栏
///
/// 参数: s 原始文本
/// 返回: 去围栏并 trim 后的文本切片
pub fn strip_code_fence(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        let rest = rest.trim_start_matches('\n');
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim();
        }
        return rest.trim();
    }
    t
}

/// 在 PATH 与各平台常见安装位置查找 ollama 可执行文件
///
/// 返回: 找到的路径
pub fn find_ollama_binary() -> Option<std::path::PathBuf> {
    ollama_candidates().into_iter().find(|p| p.is_file())
}

/// 可能的 ollama 可执行文件路径(按优先级)
fn ollama_candidates() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let exe = if cfg!(windows) {
        "ollama.exe"
    } else {
        "ollama"
    };
    let mut v: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(exe)).collect())
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        v.extend(
            [
                "/usr/local/bin/ollama",
                "/opt/homebrew/bin/ollama",
                "/Applications/Ollama.app/Contents/Resources/ollama",
            ]
            .map(PathBuf::from),
        );
    } else if cfg!(windows) {
        // 官方安装包默认装到 %LOCALAPPDATA%\Programs\Ollama
        if let Some(d) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(d).join("Programs").join("Ollama").join(exe));
        }
        if let Some(d) = std::env::var_os("ProgramFiles") {
            v.push(PathBuf::from(d).join("Ollama").join(exe));
        }
    } else {
        v.extend(
            [
                "/usr/local/bin/ollama",
                "/usr/bin/ollama",
                "/opt/ollama/bin/ollama",
            ]
            .map(PathBuf::from),
        );
    }
    v
}

/// 尝试用系统方式打开 Ollama 桌面应用(仅 macOS 有 `open -a`)
async fn open_ollama_app() -> bool {
    #[cfg(target_os = "macos")]
    {
        tokio::process::Command::new("open")
            .args(["-a", "Ollama"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// 后台启动 `ollama serve`(Windows 下不弹出控制台窗口)
fn spawn_ollama_serve(bin: &std::path::Path) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
}

/// 尝试拉起本机 Ollama(优先打开桌面应用,其次后台执行 `ollama serve`)
///
/// 参数: client 用于轮询探活的客户端
/// 返回: 在 20 秒内启动成功返回 Ok
pub async fn try_start_ollama(client: &OllamaClient) -> Result<()> {
    if client.version().await.is_ok() {
        return Ok(());
    }
    if !open_ollama_app().await {
        if let Some(bin) = find_ollama_binary() {
            spawn_ollama_serve(&bin)
                .map_err(|e| AppError::OllamaUnavailable(format!("无法启动 ollama: {e}")))?;
        } else {
            return Err(AppError::OllamaUnavailable(
                "未找到 Ollama,请先安装 https://ollama.com".into(),
            ));
        }
    }
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if client.version().await.is_ok() {
            return Ok(());
        }
    }
    Err(AppError::OllamaUnavailable("Ollama 启动超时".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama候选路径包含当前平台的常见位置() {
        let c = ollama_candidates();
        assert!(!c.is_empty());
        if cfg!(windows) {
            assert!(c.iter().all(|p| p.extension().is_some_and(|e| e == "exe")));
        } else {
            assert!(c.iter().any(|p| p.ends_with("ollama")));
        }
    }

    #[test]
    fn 去除代码围栏() {
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(
            strip_code_fence("```mermaid\nflowchart TD\nA-->B\n```"),
            "flowchart TD\nA-->B"
        );
        assert_eq!(strip_code_fence("  {\"a\":1} "), "{\"a\":1}");
    }
}
