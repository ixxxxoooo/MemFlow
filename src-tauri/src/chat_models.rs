// @author ygw
//! 对话可选模型:本地 Ollama 已安装的文本模型 + 云端档案(默认云端配置与额外档案)。
//!
//! 引用格式(`ref`):
//! - `ollama:<模型名>`
//! - `cloud:default`(设置里的默认云端配置)
//! - `cloud:<档案 id>`
//!
//! 对话时按引用即时构造客户端,后台任务(元数据提取等)仍使用设置里的默认模型,互不影响。

use crate::core::Core;
use crate::error::{AppError, Result};
use crate::llm::LlmClient;
use crate::openai::OpenAiClient;
use crate::secrets::DEFAULT_PROFILE;
use crate::settings::{Settings, PROVIDER_OLLAMA, PROVIDER_OPENAI};
use serde::Serialize;

/// 供前端展示的可选模型
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModelOption {
    /// 引用标识
    #[serde(rename = "ref")]
    pub ref_id: String,
    /// 来源:ollama / openai_compat
    pub provider: String,
    /// 显示名
    pub label: String,
    /// 模型名
    pub model: String,
    /// 云端预设标识(挑图标用)
    pub preset: Option<String>,
}

/// 解析后的对话模型
#[derive(Clone)]
pub struct ResolvedModel {
    pub client: LlmClient,
    pub model: String,
    pub provider: String,
    pub ref_id: String,
    pub preset: Option<String>,
}

impl ResolvedModel {
    /// 是否为本地 Ollama 模型
    pub fn is_local(&self) -> bool {
        self.provider == PROVIDER_OLLAMA
    }
}

/// 名称是否像"非对话"模型(向量、重排、视觉专用)
pub fn is_non_chat_model(name: &str, s: &Settings) -> bool {
    let n = name.to_lowercase();
    let base = |x: &str| x.split(':').next().unwrap_or(x).to_lowercase();
    if base(name) == base(&s.embed_model) || base(name) == base(&s.vision_model) {
        return true;
    }
    const KEYS: [&str; 10] = [
        "embed",
        "bge-",
        "bge:",
        "rerank",
        "minicpm-v",
        "llava",
        "-vl",
        "vision",
        "moondream",
        "clip",
    ];
    KEYS.iter().any(|k| n.contains(k)) || n.starts_with("bge")
}

/// 默认对话模型引用(与设置里的文本模型来源一致)
pub fn default_ref(s: &Settings) -> String {
    if s.uses_cloud_llm() {
        format!("cloud:{DEFAULT_PROFILE}")
    } else {
        format!("ollama:{}", s.text_model)
    }
}

/// 列出当前可用于对话的模型
///
/// 返回: 本地已安装文本模型(Ollama 不可用时至少包含设置里的文本模型)+ 已配置密钥的云端模型
pub async fn list_chat_models(core: &Core) -> Vec<ChatModelOption> {
    let s = core.settings();
    let mut out: Vec<ChatModelOption> = Vec::new();

    let mut local: Vec<String> = core
        .ollama()
        .list_models()
        .await
        .map(|v| v.into_iter().map(|m| m.name).collect())
        .unwrap_or_default();
    local.retain(|n| !is_non_chat_model(n, &s) || n == &s.text_model);
    if !s.text_model.is_empty() && !local.iter().any(|n| n == &s.text_model) {
        local.insert(0, s.text_model.clone());
    }
    local.sort();
    local.dedup();
    for n in local {
        out.push(ChatModelOption {
            ref_id: format!("ollama:{n}"),
            provider: PROVIDER_OLLAMA.into(),
            label: n.clone(),
            model: n,
            preset: None,
        });
    }

    // 默认云端配置:填写了地址与模型且有密钥才可选
    if !s.llm_base_url.is_empty() && !s.llm_model.is_empty() && core.key_present(DEFAULT_PROFILE) {
        out.push(ChatModelOption {
            ref_id: format!("cloud:{DEFAULT_PROFILE}"),
            provider: PROVIDER_OPENAI.into(),
            label: s.llm_model.clone(),
            model: s.llm_model.clone(),
            preset: Some(s.llm_preset.clone()),
        });
    }
    for p in &s.llm_profiles {
        if core.key_present(&p.id) {
            out.push(ChatModelOption {
                ref_id: format!("cloud:{}", p.id),
                provider: PROVIDER_OPENAI.into(),
                label: p.name.clone(),
                model: p.model.clone(),
                preset: Some(p.preset.clone()),
            });
        }
    }
    out
}

/// 把模型引用解析成可调用的客户端
///
/// 参数: core 核心;model_ref 引用,None 表示使用设置里的默认模型
/// 返回: 解析结果;引用不合法或云端缺少密钥时返回错误
pub fn resolve(core: &Core, model_ref: Option<&str>) -> Result<ResolvedModel> {
    let s = core.settings();
    let r = match model_ref.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => r.to_string(),
        None => default_ref(&s),
    };
    if let Some(name) = r.strip_prefix("ollama:") {
        if name.is_empty() || name.len() > 100 {
            return Err(AppError::Invalid("模型名不合法".into()));
        }
        return Ok(ResolvedModel {
            client: LlmClient::Ollama(core.ollama()),
            model: name.to_string(),
            provider: PROVIDER_OLLAMA.into(),
            ref_id: r.clone(),
            preset: None,
        });
    }
    if let Some(pid) = r.strip_prefix("cloud:") {
        let (base, model, preset) = if pid == DEFAULT_PROFILE {
            (
                s.llm_base_url.clone(),
                s.llm_model.clone(),
                s.llm_preset.clone(),
            )
        } else {
            let p = s
                .llm_profiles
                .iter()
                .find(|p| p.id == pid)
                .ok_or_else(|| AppError::Invalid("所选云端模型已被删除".into()))?;
            (p.base_url.clone(), p.model.clone(), p.preset.clone())
        };
        let key = core
            .key_for(pid)
            .ok_or_else(|| AppError::Llm("所选云端模型尚未配置 API Key,请到设置里填写".into()))?;
        return Ok(ResolvedModel {
            client: LlmClient::OpenAi(OpenAiClient::new(core.cloud_http(), &base, &key)),
            model,
            provider: PROVIDER_OPENAI.into(),
            ref_id: r.clone(),
            preset: Some(preset),
        });
    }
    Err(AppError::Invalid("未知的对话模型引用".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 过滤向量与视觉模型() {
        let s = Settings::default();
        assert!(is_non_chat_model("bge-m3:latest", &s));
        assert!(is_non_chat_model("nomic-embed-text", &s));
        assert!(is_non_chat_model("minicpm-v:latest", &s));
        assert!(is_non_chat_model("qwen2.5vl:7b-vl", &s));
        assert!(is_non_chat_model("llava:13b", &s));
        assert!(!is_non_chat_model("qwen2.5:7b", &s));
        assert!(!is_non_chat_model("deepseek-r1:8b", &s));
        assert!(!is_non_chat_model("llama3.2:3b", &s));
    }

    #[test]
    fn 默认引用随来源变化() {
        let mut s = Settings::default();
        assert_eq!(default_ref(&s), "ollama:qwen2.5:7b");
        s.llm_provider = PROVIDER_OPENAI.into();
        assert_eq!(default_ref(&s), "cloud:default");
    }
}
