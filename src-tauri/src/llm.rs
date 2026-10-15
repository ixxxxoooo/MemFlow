// @author ygw
//! 文本对话后端的统一入口:按设置在本地 Ollama 与 OpenAI 兼容云端之间切换。
//!
//! 只覆盖"文本类"调用(问答、查询改写、结构化提取、Mermaid 修复)。
//! 向量化、视觉解析、模型下载始终走本地 Ollama。

use crate::error::{AppError, Result};
use crate::ollama::{strip_code_fence, ChatMessage, GenOptions, OllamaClient};
use crate::openai::OpenAiClient;
use serde_json::Value;
use std::sync::atomic::AtomicBool;

/// 文本对话客户端
#[derive(Clone)]
pub enum LlmClient {
    /// 本地 Ollama
    Ollama(OllamaClient),
    /// OpenAI 兼容云端
    OpenAi(OpenAiClient),
}

impl LlmClient {
    /// 是否为云端后端
    pub fn is_cloud(&self) -> bool {
        matches!(self, LlmClient::OpenAi(_))
    }

    /// 非流式对话,可选 JSON Schema 约束输出
    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        schema: Option<&Value>,
    ) -> Result<String> {
        match self {
            LlmClient::Ollama(c) => c.chat(model, messages, opts, schema).await,
            LlmClient::OpenAi(c) => c.chat(model, messages, opts, schema).await,
        }
    }

    /// 结构化对话:要求按 JSON Schema 输出并反序列化
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

    /// 流式对话:逐段回调,支持取消
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        opts: &GenOptions,
        cancel: &AtomicBool,
        on_token: impl FnMut(&str),
    ) -> Result<String> {
        match self {
            LlmClient::Ollama(c) => c.chat_stream(model, messages, opts, cancel, on_token).await,
            LlmClient::OpenAi(c) => c.chat_stream(model, messages, opts, cancel, on_token).await,
        }
    }
}
