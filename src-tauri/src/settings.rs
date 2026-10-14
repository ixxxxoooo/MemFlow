// @author ygw
//! 应用设置:以单行 JSON 存入 settings 表,带默认值与合法性校验。

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};

/// 应用设置(前端使用 camelCase)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Ollama 服务地址
    pub ollama_url: String,
    /// 向量化模型
    pub embed_model: String,
    /// 文本推理/问答模型
    pub text_model: String,
    /// 视觉模型
    pub vision_model: String,
    /// 是否启用视觉解析(默认关闭)
    pub vision_enabled: bool,
    /// 模型常驻时长,如 "5m"
    pub keep_alive: String,
    /// 全局快捷键
    pub shortcut: String,
    /// 向量相似度相关性阈值(0~1)
    pub score_threshold: f32,
    /// 问答引用的资料条数上限
    pub top_k: usize,
    /// 多轮对话时是否改写查询
    pub query_rewrite: bool,
    /// 文本模型上下文长度(所有调用保持一致,避免 Ollama 反复重载模型)
    pub num_ctx: u32,
    /// 导入文档时是否自动 AI 解读(默认关闭:文档只做快速索引)
    pub doc_ai_enrich: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ollama_url: "http://127.0.0.1:11434".into(),
            embed_model: "bge-m3".into(),
            text_model: "qwen2.5:7b".into(),
            vision_model: "minicpm-v".into(),
            vision_enabled: false,
            keep_alive: "5m".into(),
            shortcut: "CommandOrControl+Shift+K".into(),
            score_threshold: 0.40,
            top_k: 8,
            query_rewrite: true,
            num_ctx: 8192,
            doc_ai_enrich: false,
        }
    }
}

impl Settings {
    /// 校验设置合法性
    ///
    /// 返回: 合法返回 Ok,否则返回带说明的 Invalid 错误
    pub fn validate(&self) -> Result<()> {
        if !(self.ollama_url.starts_with("http://") || self.ollama_url.starts_with("https://")) {
            return Err(AppError::Invalid(
                "Ollama 地址必须以 http:// 或 https:// 开头".into(),
            ));
        }
        for (name, v) in [
            ("向量模型", &self.embed_model),
            ("文本模型", &self.text_model),
            ("视觉模型", &self.vision_model),
        ] {
            if v.trim().is_empty() {
                return Err(AppError::Invalid(format!("{name}不能为空")));
            }
        }
        if !(0.0..=1.0).contains(&self.score_threshold) {
            return Err(AppError::Invalid("相关性阈值需在 0~1 之间".into()));
        }
        if !(1..=30).contains(&self.top_k) {
            return Err(AppError::Invalid("引用条数需在 1~30 之间".into()));
        }
        if !(2048..=131072).contains(&self.num_ctx) {
            return Err(AppError::Invalid("上下文长度需在 2048~131072 之间".into()));
        }
        if self.shortcut.trim().is_empty() {
            return Err(AppError::Invalid("快捷键不能为空".into()));
        }
        let ka = self.keep_alive.trim();
        let ok = ka == "-1"
            || (ka.len() > 1
                && ka[..ka.len() - 1].parse::<u32>().is_ok()
                && matches!(ka.chars().last(), Some('s' | 'm' | 'h')));
        if !ok {
            return Err(AppError::Invalid(
                "常驻时长格式应为 30s / 5m / 1h / -1".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认设置合法() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn 非法设置被拒绝() {
        let s = Settings {
            score_threshold: 1.5,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        let s = Settings {
            keep_alive: "abc".into(),
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        let s = Settings {
            ollama_url: "localhost".into(),
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }
}
