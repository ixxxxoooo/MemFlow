// @author ygw
//! OpenAI 兼容协议客户端(DeepSeek、通义、Kimi、智谱、硅基流动、OpenAI 等)。
//!
//! 只负责文本对话,不提供向量化与视觉:
//! - DeepSeek 官方没有 embedding 接口,向量模型始终使用本地 Ollama
//! - 结构化输出统一使用 `response_format: json_object`,并把 JSON Schema 写进提示词
//!   (DeepSeek 不支持 `json_schema` 模式)
//! - 流式使用 SSE(`data: {...}` 逐行,以 `data: [DONE]` 结束)
//! - 推理模型的 `reasoning_content`(思考过程)不会混入最终回答

use crate::error::{AppError, Result};
use crate::ollama::{ChatMessage, GenOptions};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 非流式请求超时
const CHAT_TIMEOUT: Duration = Duration::from_secs(180);
/// 流式读取的空闲超时
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// 一行 SSE 数据的解析结果
#[derive(Debug, PartialEq)]
pub enum SseEvent {
    /// 空行 / 注释 / 与内容无关的字段
    Skip,
    /// 流结束标记
    Done,
    /// 一段正文增量
    Delta(String),
}

/// 解析一行 SSE 文本
///
/// 参数: line 去掉换行后的一行
/// 返回: 解析结果;`data:` 后不是合法 JSON 时返回错误
pub fn parse_sse_line(line: &str) -> Result<SseEvent> {
    let line = line.trim();
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(SseEvent::Skip);
    };
    let data = data.trim();
    if data.is_empty() {
        return Ok(SseEvent::Skip);
    }
    if data == "[DONE]" {
        return Ok(SseEvent::Done);
    }
    let v: Value =
        serde_json::from_str(data).map_err(|e| AppError::Llm(format!("流数据非法: {e}")))?;
    if let Some(msg) = v
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(AppError::Llm(msg.to_string()));
    }
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    if text.is_empty() {
        Ok(SseEvent::Skip)
    } else {
        Ok(SseEvent::Delta(text.to_string()))
    }
}

/// 把 JSON Schema 约束写进系统提示词(json_object 模式要求提示词里出现 json 字样)
///
/// 参数: messages 原消息;schema JSON Schema
/// 返回: 注入后的新消息列表
pub fn inject_schema(messages: &[ChatMessage], schema: &Value) -> Vec<ChatMessage> {
    let hint = format!(
        "\n\n请严格只输出一个合法的 JSON 对象(json),不要使用代码围栏或额外说明,且必须符合如下 JSON Schema:\n{}",
        schema
    );
    let mut out: Vec<ChatMessage> = messages.to_vec();
    match out.first_mut() {
        Some(m) if m.role == "system" => m.content.push_str(&hint),
        _ => out.insert(0, ChatMessage::new("system", hint.trim_start())),
    }
    out
}

/// 创建云端专用 HTTP 客户端:遵循系统代理(与本地 Ollama 的 no_proxy 客户端分开)
///
/// 返回: reqwest 客户端
pub fn build_cloud_http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("创建云端 HTTP 客户端失败")
}

/// OpenAI 兼容客户端
#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    base: String,
    api_key: String,
    /// 密钥为空时的说明(例如钥匙串读取失败),用于给出比"未配置"更准确的错误
    missing_key_reason: Option<String>,
}

impl OpenAiClient {
    /// 创建客户端
    ///
    /// 参数: http 云端专用 HTTP 客户端;base 接口根地址;api_key 密钥
    pub fn new(http: reqwest::Client, base: &str, api_key: &str) -> Self {
        let mut b = base.trim().trim_end_matches('/').to_string();
        // 兼容用户直接粘贴完整的 chat/completions 地址
        if let Some(s) = b.strip_suffix("/chat/completions") {
            b = s.to_string();
        }
        OpenAiClient {
            http,
            base: b,
            api_key: api_key.trim().to_string(),
            missing_key_reason: None,
        }
    }

    /// 记录密钥缺失的原因(如钥匙串授权被拒绝),发送请求时作为错误信息
    pub fn with_missing_key_reason(mut self, reason: String) -> Self {
        self.missing_key_reason = Some(reason);
        self
    }

    /// 对话接口完整地址
    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base)
    }

    /// 构造请求体
    fn body(
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        stream: bool,
        schema: Option<&Value>,
    ) -> Value {
        let msgs = match schema {
            Some(s) => inject_schema(messages, s),
            None => messages.to_vec(),
        };
        let mut body = json!({
            "model": model,
            "messages": msgs,
            "stream": stream,
            "temperature": opts.temperature,
        });
        if schema.is_some() {
            body["response_format"] = json!({ "type": "json_object" });
        }
        body
    }

    /// 发送请求并统一映射 HTTP 错误
    async fn send(&self, body: &Value, timeout: Option<Duration>) -> Result<reqwest::Response> {
        if self.api_key.is_empty() {
            return Err(AppError::LlmUnavailable(
                self.missing_key_reason
                    .clone()
                    .unwrap_or_else(|| "尚未配置 API Key,请在设置中填写".into()),
            ));
        }
        let mut req = self
            .http
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(body);
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_connect() || e.is_timeout() || e.is_request() {
                AppError::LlmUnavailable(format!("无法连接云端模型服务: {e}"))
            } else {
                AppError::Llm(e.to_string())
            }
        })?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message").or(Some(e)))
                    .and_then(|m| m.as_str().map(String::from))
            })
            .unwrap_or(text);
        Err(map_status(status.as_u16(), &msg))
    }

    /// 非流式对话
    ///
    /// 参数: model 模型;messages 消息;opts 生成参数;schema 可选 JSON Schema
    /// 返回: 完整文本
    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        schema: Option<&Value>,
    ) -> Result<String> {
        let body = Self::body(model, messages, opts, false, schema);
        let resp = self.send(&body, Some(CHAT_TIMEOUT)).await?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Llm(format!("响应不是合法 JSON: {e}")))?;
        v.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(String::from)
            .ok_or_else(|| AppError::Llm("响应缺少 choices[0].message.content".into()))
    }

    /// 流式对话:逐段回调,支持取消
    ///
    /// 参数: model 模型;messages 消息;opts 参数;cancel 取消标志;on_token 增量回调
    /// 返回: 完整文本(被取消时返回 Cancelled)
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        cancel: &AtomicBool,
        mut on_token: impl FnMut(&str),
    ) -> Result<String> {
        let body = Self::body(model, messages, opts, true, None);
        let resp = self.send(&body, None).await?;
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
                Err(_) => {
                    if last_data.elapsed() > STREAM_IDLE_TIMEOUT {
                        return Err(AppError::LlmUnavailable("云端模型长时间无输出".into()));
                    }
                    continue;
                }
                Ok(None) => break,
                Ok(Some(c)) => c.map_err(|e| AppError::LlmUnavailable(e.to_string()))?,
            };
            last_data = std::time::Instant::now();
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                match parse_sse_line(&String::from_utf8_lossy(&line))? {
                    SseEvent::Skip => {}
                    SseEvent::Done => return Ok(full),
                    SseEvent::Delta(t) => {
                        full.push_str(&t);
                        on_token(&t);
                    }
                }
            }
        }
        Ok(full)
    }

    /// 连通性测试:发送一个极小请求,验证地址、Key 与模型名
    ///
    /// 参数: model 模型名
    /// 返回: 成功返回提示文本
    pub async fn ping(&self, model: &str) -> Result<String> {
        let body = json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 8,
            "stream": false,
        });
        self.send(&body, Some(Duration::from_secs(30))).await?;
        Ok(format!("连接成功,模型 {model} 可用"))
    }
}

/// 把 HTTP 状态码映射成带中文说明的错误
///
/// 参数: status 状态码;msg 服务端返回的错误信息
/// 返回: 应用错误(鉴权/额度/限流/服务端故障归为"临时不可用")
pub fn map_status(status: u16, msg: &str) -> AppError {
    match status {
        401 | 403 => AppError::LlmUnavailable(format!("API Key 无效或无权限({status}):{msg}")),
        402 => AppError::LlmUnavailable(format!("账户余额不足({status}):{msg}")),
        429 => AppError::LlmUnavailable(format!("请求过于频繁或额度用尽({status}):{msg}")),
        404 => AppError::Llm(format!("接口地址或模型名不存在({status}):{msg}")),
        s if s >= 500 => AppError::LlmUnavailable(format!("云端服务异常({s}):{msg}")),
        s => AppError::Llm(format!("HTTP {s}:{msg}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析sse各类行() {
        assert_eq!(parse_sse_line("").unwrap(), SseEvent::Skip);
        assert_eq!(parse_sse_line(": keep-alive").unwrap(), SseEvent::Skip);
        assert_eq!(parse_sse_line("data: [DONE]").unwrap(), SseEvent::Done);
        assert_eq!(
            parse_sse_line(r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#).unwrap(),
            SseEvent::Delta("你好".into())
        );
        // 推理内容不输出
        assert_eq!(
            parse_sse_line(
                r#"data: {"choices":[{"delta":{"reasoning_content":"想一想","content":null}}]}"#
            )
            .unwrap(),
            SseEvent::Skip
        );
        assert!(parse_sse_line("data: {oops").is_err());
        assert!(parse_sse_line(r#"data: {"error":{"message":"bad"}}"#).is_err());
    }

    #[test]
    fn 地址规整() {
        let c = OpenAiClient::new(reqwest::Client::new(), "https://api.deepseek.com/", "k");
        assert_eq!(c.endpoint(), "https://api.deepseek.com/chat/completions");
        let c = OpenAiClient::new(
            reqwest::Client::new(),
            "https://api.openai.com/v1/chat/completions",
            "k",
        );
        assert_eq!(c.endpoint(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn 注入schema到系统提示词() {
        let schema = json!({"type":"object"});
        let m = inject_schema(
            &[
                ChatMessage::new("system", "你是助手"),
                ChatMessage::new("user", "hi"),
            ],
            &schema,
        );
        assert_eq!(m.len(), 2);
        assert!(m[0].content.contains("json") && m[0].content.starts_with("你是助手"));
        let m = inject_schema(&[ChatMessage::new("user", "hi")], &schema);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].role, "system");
    }

    #[test]
    fn 状态码映射() {
        assert!(map_status(401, "x").is_unavailable());
        assert!(map_status(402, "x").is_unavailable());
        assert!(map_status(429, "x").is_unavailable());
        assert!(map_status(503, "x").is_unavailable());
        assert!(!map_status(400, "x").is_unavailable());
        assert!(!map_status(404, "x").is_unavailable());
    }
}
