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
    /// 文本模型来源:ollama(本地)或 openai_compat(OpenAI 兼容云端,如 DeepSeek)
    pub llm_provider: String,
    /// 云端预设标识(仅用于界面回显,如 deepseek / custom)
    pub llm_preset: String,
    /// 云端接口根地址(含版本路径,实际请求 `{base}/chat/completions`)
    pub llm_base_url: String,
    /// 云端文本模型名
    pub llm_model: String,
    /// 直连下载使用的 Ollama 模型仓库地址(可换成镜像)
    pub model_registry: String,
    /// 入库并发处理的文档数(0 = 自动,按 CPU 核数;上限 8)
    pub import_concurrency: u32,
    /// PDF 解析模式:auto = 文本层 + 扫描页自动 OCR(推荐);vision = 视觉模型逐页转 Markdown(慢,表格/版面更准)
    pub pdf_parse_mode: String,
    /// 是否启用本地重排序(模型未下载时自动跳过)
    pub rerank_enabled: bool,
    /// 重排序模型标识(见 `rerank::SPECS`)
    pub rerank_model: String,
    /// 重排序模型下载使用的 HuggingFace 镜像地址
    pub hf_mirror: String,
}

/// PDF 解析模式:自动(文本层 + 本机 OCR)
pub const PDF_MODE_AUTO: &str = "auto";
/// PDF 解析模式:视觉模型逐页解析
pub const PDF_MODE_VISION: &str = "vision";

/// 文本模型来源:本地 Ollama
pub const PROVIDER_OLLAMA: &str = "ollama";
/// 文本模型来源:OpenAI 兼容云端
pub const PROVIDER_OPENAI: &str = "openai_compat";

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
            llm_provider: PROVIDER_OLLAMA.into(),
            llm_preset: "deepseek".into(),
            llm_base_url: "https://api.deepseek.com".into(),
            llm_model: "deepseek-chat".into(),
            model_registry: "https://registry.ollama.ai".into(),
            import_concurrency: 0,
            pdf_parse_mode: PDF_MODE_AUTO.into(),
            rerank_enabled: true,
            rerank_model: crate::rerank::DEFAULT_MODEL.into(),
            hf_mirror: "https://hf-mirror.com".into(),
        }
    }
}

impl Settings {
    /// 文本类调用是否使用云端模型
    pub fn uses_cloud_llm(&self) -> bool {
        self.llm_provider == PROVIDER_OPENAI
    }

    /// 当前生效的文本模型名(云端模式取云端模型名,否则取本地文本模型)
    pub fn active_text_model(&self) -> &str {
        if self.uses_cloud_llm() {
            &self.llm_model
        } else {
            &self.text_model
        }
    }

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
        if self.llm_provider != PROVIDER_OLLAMA && self.llm_provider != PROVIDER_OPENAI {
            return Err(AppError::Invalid(
                "文本模型来源只能是 ollama 或 openai_compat".into(),
            ));
        }
        if !(self.model_registry.starts_with("http://")
            || self.model_registry.starts_with("https://"))
        {
            return Err(AppError::Invalid(
                "模型仓库地址必须以 http:// 或 https:// 开头".into(),
            ));
        }
        if self.uses_cloud_llm() {
            if !(self.llm_base_url.starts_with("http://")
                || self.llm_base_url.starts_with("https://"))
            {
                return Err(AppError::Invalid(
                    "云端接口地址必须以 http:// 或 https:// 开头".into(),
                ));
            }
            if self.llm_model.trim().is_empty() {
                return Err(AppError::Invalid("云端模型名不能为空".into()));
            }
        }
        if self.pdf_parse_mode != PDF_MODE_AUTO && self.pdf_parse_mode != PDF_MODE_VISION {
            return Err(AppError::Invalid(
                "PDF 解析模式只能是 auto 或 vision".into(),
            ));
        }
        if crate::rerank::spec_of(&self.rerank_model).is_none() {
            return Err(AppError::Invalid("未知的重排序模型".into()));
        }
        if !(self.hf_mirror.starts_with("http://") || self.hf_mirror.starts_with("https://")) {
            return Err(AppError::Invalid(
                "镜像地址必须以 http:// 或 https:// 开头".into(),
            ));
        }
        if self.import_concurrency > 8 {
            return Err(AppError::Invalid(
                "入库并发数需在 0~8 之间(0 为自动)".into(),
            ));
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
    fn 云端模式校验必填项() {
        let ok = Settings {
            llm_provider: PROVIDER_OPENAI.into(),
            ..Settings::default()
        };
        assert!(ok.validate().is_ok());
        assert_eq!(ok.active_text_model(), "deepseek-chat");
        let bad = Settings {
            llm_provider: PROVIDER_OPENAI.into(),
            llm_model: " ".into(),
            ..Settings::default()
        };
        assert!(bad.validate().is_err());
        let bad = Settings {
            llm_provider: "foo".into(),
            ..Settings::default()
        };
        assert!(bad.validate().is_err());
        assert_eq!(Settings::default().active_text_model(), "qwen2.5:7b");
    }

    #[test]
    fn 入库并发数范围校验() {
        let ok = Settings {
            import_concurrency: 8,
            ..Settings::default()
        };
        assert!(ok.validate().is_ok());
        let bad = Settings {
            import_concurrency: 9,
            ..Settings::default()
        };
        assert!(bad.validate().is_err());
        // 旧版本保存的设置缺少该字段时回落为自动
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.import_concurrency, 0);
        assert_eq!(s.pdf_parse_mode, PDF_MODE_AUTO);
        assert!(s.rerank_enabled);
        assert_eq!(s.rerank_model, crate::rerank::DEFAULT_MODEL);
        let bad_rerank = Settings {
            rerank_model: "nope".into(),
            ..Settings::default()
        };
        assert!(bad_rerank.validate().is_err());
        let bad_mirror = Settings {
            hf_mirror: "ftp://x".into(),
            ..Settings::default()
        };
        assert!(bad_mirror.validate().is_err());
        let bad_mode = Settings {
            pdf_parse_mode: "other".into(),
            ..Settings::default()
        };
        assert!(bad_mode.validate().is_err());
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
