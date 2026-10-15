// @author ygw
//! 本地 ONNX 重排序:对混合检索召回的候选 (问题, 片段) 逐对打分,提升排序精度。
//!
//! 为什么自带模型:Ollama 没有 `/api/rerank` 接口。这里用 `fastembed` 加载量化后的
//! bge-reranker(int8 ONNX),模型文件走镜像下载(断点续传 + sha256 校验),
//! 存放在 `<数据目录>/models/rerank/<模型名>/`。会话懒加载并常驻,推理在阻塞线程中完成。
//!
//! 任何失败(未下载、加载失败、超时)都只会让检索回退到 RRF 排序,不影响问答。

use crate::error::{AppError, Result};
use crate::model_fetch::download_file;
use crate::ollama::PullProgress;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 官方站点(镜像失败时的兜底)
const OFFICIAL_HOST: &str = "https://huggingface.co";
/// 单次重排序的时间预算(超出则放弃,沿用 RRF 排序)
pub const RERANK_BUDGET: Duration = Duration::from_secs(5);
/// 送入模型的片段最大字符数(中文约 1 字 ≈ 0.7 token,512 token 上限内留足余量)
const MAX_DOC_CHARS: usize = 700;
/// 模型最大序列长度
const MAX_SEQ_LEN: usize = 512;
/// 推理批大小
const BATCH_SIZE: usize = 8;

/// 模型文件描述
pub struct RerankFile {
    /// 仓库内路径
    pub remote: &'static str,
    /// 本地文件名
    pub local: &'static str,
    /// 字节数
    pub size: u64,
    /// sha256(小文件无 LFS 哈希时为 None,只校验大小)
    pub sha256: Option<&'static str>,
}

/// 可选的重排序模型
pub struct RerankSpec {
    /// 稳定标识(目录名与设置值)
    pub id: &'static str,
    /// 界面名称
    pub label: &'static str,
    /// HuggingFace 仓库
    pub repo: &'static str,
    /// 需要的文件
    pub files: &'static [RerankFile],
}

/// 内置模型清单
pub static SPECS: &[RerankSpec] = &[
    RerankSpec {
        id: "bge-reranker-base",
        label: "bge-reranker-base(中英,约 280MB,推荐)",
        repo: "Xenova/bge-reranker-base",
        files: &[
            RerankFile {
                remote: "onnx/model_quantized.onnx",
                local: "model.onnx",
                size: 279_301_077,
                sha256: Some("dd98f3e67837d23210a6b7550c08cced4f61845b940ac45be3565840a10f3244"),
            },
            RerankFile {
                remote: "tokenizer.json",
                local: "tokenizer.json",
                size: 17_098_079,
                sha256: Some("48564c5c7d3fa64d85d95e65414a542385f88b0f128fd8d4163fd7a57f2be05c"),
            },
            RerankFile {
                remote: "config.json",
                local: "config.json",
                size: 782,
                sha256: None,
            },
            RerankFile {
                remote: "special_tokens_map.json",
                local: "special_tokens_map.json",
                size: 279,
                sha256: None,
            },
            RerankFile {
                remote: "tokenizer_config.json",
                local: "tokenizer_config.json",
                size: 443,
                sha256: None,
            },
        ],
    },
    RerankSpec {
        id: "bge-reranker-v2-m3",
        label: "bge-reranker-v2-m3(多语言,约 570MB,更准更慢)",
        repo: "onnx-community/bge-reranker-v2-m3-ONNX",
        files: &[
            RerankFile {
                remote: "onnx/model_quantized.onnx",
                local: "model.onnx",
                size: 570_727_094,
                sha256: Some("912fc1215c2dbff6499700534bd8d31253af01573861abbfc43afd1fab6cce5d"),
            },
            RerankFile {
                remote: "tokenizer.json",
                local: "tokenizer.json",
                size: 17_082_900,
                sha256: Some("8bf8afbfd11306bd872018c53bfdf2e160a56f8edbcf49933324404791c148d3"),
            },
            RerankFile {
                remote: "config.json",
                local: "config.json",
                size: 848,
                sha256: None,
            },
            RerankFile {
                remote: "special_tokens_map.json",
                local: "special_tokens_map.json",
                size: 964,
                sha256: None,
            },
            RerankFile {
                remote: "tokenizer_config.json",
                local: "tokenizer_config.json",
                size: 1203,
                sha256: None,
            },
        ],
    },
];

/// 默认模型标识
pub const DEFAULT_MODEL: &str = "bge-reranker-base";

/// 按标识查找模型
pub fn spec_of(id: &str) -> Option<&'static RerankSpec> {
    SPECS.iter().find(|s| s.id == id)
}

/// 重排序模型根目录
pub fn models_dir(root: &Path) -> PathBuf {
    root.join("models").join("rerank")
}

/// 某个模型的目录
pub fn model_dir(root: &Path, id: &str) -> PathBuf {
    models_dir(root).join(id)
}

/// 模型是否已完整下载(文件只会在校验通过后才改名到位,所以只需核对存在与大小)
pub fn is_installed(root: &Path, id: &str) -> bool {
    let Some(spec) = spec_of(id) else {
        return false;
    };
    let dir = model_dir(root, id);
    spec.files.iter().all(|f| {
        std::fs::metadata(dir.join(f.local))
            .map(|m| m.len() == f.size)
            .unwrap_or(false)
    })
}

/// 模型状态(给设置页)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankModelStatus {
    pub id: String,
    pub label: String,
    /// 总下载大小(字节)
    pub size: u64,
    pub installed: bool,
}

/// 列出所有内置模型及安装状态
pub fn list_status(root: &Path) -> Vec<RerankModelStatus> {
    SPECS
        .iter()
        .map(|s| RerankModelStatus {
            id: s.id.into(),
            label: s.label.into(),
            size: s.files.iter().map(|f| f.size).sum(),
            installed: is_installed(root, s.id),
        })
        .collect()
}

/// 下载模型:先走镜像,失败(非取消)再回退官方站点
///
/// 参数: http 无总超时的客户端;mirror 镜像地址;root 数据目录;id 模型标识;
/// cancel 取消标志;on_progress 进度回调
pub async fn download(
    http: &reqwest::Client,
    mirror: &str,
    root: &Path,
    id: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(PullProgress) + Send,
) -> Result<()> {
    let spec = spec_of(id).ok_or_else(|| AppError::Invalid(format!("未知的重排序模型: {id}")))?;
    let dir = model_dir(root, id);
    tokio::fs::create_dir_all(&dir).await?;
    let mirror = mirror.trim().trim_end_matches('/').to_string();
    let mut hosts = vec![mirror.clone()];
    if mirror != OFFICIAL_HOST {
        hosts.push(OFFICIAL_HOST.to_string());
    }
    let total: u64 = spec.files.iter().map(|f| f.size).sum();
    let mut done: u64 = 0;
    for f in spec.files {
        let fin = dir.join(f.local);
        if tokio::fs::metadata(&fin)
            .await
            .map(|m| m.len() == f.size)
            .unwrap_or(false)
        {
            done += f.size;
            continue;
        }
        let mut last: Option<AppError> = None;
        for host in &hosts {
            let url = format!("{host}/{}/resolve/main/{}", spec.repo, f.remote);
            let r = download_file(
                http,
                &url,
                &fin,
                f.size,
                f.sha256,
                &format!("下载 {}", f.local),
                (done, total),
                cancel,
                &mut on_progress,
            )
            .await;
            match r {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(AppError::Cancelled) => return Err(AppError::Cancelled),
                Err(e) => {
                    tracing::warn!("从 {host} 下载 {} 失败: {e}", f.remote);
                    last = Some(e);
                }
            }
        }
        if let Some(e) = last {
            return Err(e);
        }
        done += f.size;
    }
    on_progress(PullProgress {
        status: "success".into(),
        total,
        completed: total,
    });
    Ok(())
}

/// 会话与其对应的模型标识
#[cfg(feature = "rerank")]
type Loaded = Option<(String, fastembed::TextRerank)>;
#[cfg(not(feature = "rerank"))]
type Loaded = Option<(String, ())>;

/// 重排序器:懒加载会话并常驻
#[derive(Clone, Default)]
pub struct Reranker {
    slot: Arc<Mutex<Loaded>>,
}

impl Reranker {
    /// 释放已加载的会话(切换模型或关闭功能时调用)
    pub fn unload(&self) {
        if let Ok(mut g) = self.slot.lock() {
            *g = None;
        }
    }

    /// 是否已加载某模型
    pub fn is_loaded(&self, id: &str) -> bool {
        self.slot
            .lock()
            .map(|g| g.as_ref().map(|(m, _)| m == id).unwrap_or(false))
            .unwrap_or(false)
    }

    /// 预热:提前加载会话,避免首次提问时才付出加载成本
    pub async fn warm(&self, root: &Path, id: &str) -> Result<()> {
        self.score(
            root,
            id,
            "预热",
            vec!["预热".to_string()],
            Duration::from_secs(60),
        )
        .await
        .map(|_| ())
    }

    /// 对候选片段打分
    ///
    /// 参数: root 数据目录;id 模型标识;query 问题;docs 候选文本;budget 时间预算
    /// 返回: 与 docs 等长的相关性分数(sigmoid 后 0~1,越大越相关)
    pub async fn score(
        &self,
        root: &Path,
        id: &str,
        query: &str,
        docs: Vec<String>,
        budget: Duration,
    ) -> Result<Vec<f32>> {
        if docs.is_empty() {
            return Ok(vec![]);
        }
        if !is_installed(root, id) {
            return Err(AppError::ModelMissing(id.to_string()));
        }
        let slot = self.slot.clone();
        let dir = model_dir(root, id);
        let id = id.to_string();
        let query = query.to_string();
        let job = tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                score_blocking(&slot, &dir, &id, &query, &docs)
            }))
            .unwrap_or_else(|_| Err(AppError::Other("重排序发生内部错误".into())))
        });
        match tokio::time::timeout(budget, job).await {
            Err(_) => Err(AppError::Other("重排序超时".into())),
            Ok(Err(e)) => Err(AppError::Other(format!("重排序任务异常: {e}"))),
            Ok(Ok(r)) => r,
        }
    }
}

/// 截断片段文本
fn clip(s: &str) -> String {
    s.chars().take(MAX_DOC_CHARS).collect()
}

/// logit → 0~1 概率
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(feature = "rerank")]
fn score_blocking(
    slot: &Mutex<Loaded>,
    dir: &Path,
    id: &str,
    query: &str,
    docs: &[String],
) -> Result<Vec<f32>> {
    let mut g = slot
        .lock()
        .map_err(|_| AppError::Other("重排序状态异常".into()))?;
    let loaded = matches!(g.as_ref(), Some((m, _)) if m == id);
    if !loaded {
        *g = Some((id.to_string(), load_model(dir)?));
    }
    let (_, model) = g.as_mut().expect("刚加载完成");
    let clipped: Vec<String> = docs.iter().map(|d| clip(d)).collect();
    let res = model
        .rerank(query.to_string(), &clipped, false, Some(BATCH_SIZE))
        .map_err(|e| AppError::Other(format!("重排序推理失败: {e}")))?;
    let mut out = vec![0f32; docs.len()];
    for r in res {
        if let Some(slot) = out.get_mut(r.index) {
            *slot = sigmoid(r.score);
        }
    }
    Ok(out)
}

#[cfg(feature = "rerank")]
fn load_model(dir: &Path) -> Result<fastembed::TextRerank> {
    use fastembed::{
        RerankInitOptionsUserDefined, TextRerank, TokenizerFiles, UserDefinedRerankingModel,
    };
    let read = |name: &str| -> Result<Vec<u8>> {
        std::fs::read(dir.join(name)).map_err(|e| AppError::Other(format!("读取 {name} 失败: {e}")))
    };
    let tok = TokenizerFiles {
        tokenizer_file: read("tokenizer.json")?,
        config_file: read("config.json")?,
        special_tokens_map_file: read("special_tokens_map.json")?,
        tokenizer_config_file: read("tokenizer_config.json")?,
    };
    let model = UserDefinedRerankingModel::new(dir.join("model.onnx"), tok);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let opts = RerankInitOptionsUserDefined::new()
        .with_max_length(MAX_SEQ_LEN)
        .with_intra_threads((cores / 2).clamp(2, 6));
    TextRerank::try_new_from_user_defined(model, opts)
        .map_err(|e| AppError::Other(format!("加载重排序模型失败: {e}")))
}

#[cfg(not(feature = "rerank"))]
fn score_blocking(
    _slot: &Mutex<Loaded>,
    _dir: &Path,
    _id: &str,
    _query: &str,
    _docs: &[String],
) -> Result<Vec<f32>> {
    let _ = (clip(""), sigmoid(0.0), MAX_SEQ_LEN, BATCH_SIZE);
    Err(AppError::Other("当前构建未包含本地重排序".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 模型清单完整且文件名安全() {
        for s in SPECS {
            assert!(spec_of(s.id).is_some());
            assert!(s.files.iter().any(|f| f.local == "model.onnx"));
            assert!(s.files.iter().any(|f| f.local == "tokenizer.json"));
            for f in s.files {
                assert!(!f.local.contains('/') && !f.local.contains(".."));
                if let Some(h) = f.sha256 {
                    assert_eq!(h.len(), 64, "{}", f.remote);
                }
            }
        }
        assert!(spec_of(DEFAULT_MODEL).is_some());
        assert!(spec_of("不存在").is_none());
    }

    #[test]
    fn 未下载时不算已安装() {
        let d = tempfile::tempdir().unwrap();
        assert!(!is_installed(d.path(), DEFAULT_MODEL));
        assert!(list_status(d.path()).iter().all(|m| !m.installed));
    }

    #[test]
    fn 文件齐全且大小一致才算已安装() {
        let d = tempfile::tempdir().unwrap();
        let spec = spec_of(DEFAULT_MODEL).unwrap();
        let dir = model_dir(d.path(), DEFAULT_MODEL);
        std::fs::create_dir_all(&dir).unwrap();
        for f in spec.files {
            // 用稀疏文件模拟大小一致
            let file = std::fs::File::create(dir.join(f.local)).unwrap();
            file.set_len(f.size).unwrap();
        }
        assert!(is_installed(d.path(), DEFAULT_MODEL));
        // 缺一个文件即为未安装
        std::fs::remove_file(dir.join("tokenizer.json")).unwrap();
        assert!(!is_installed(d.path(), DEFAULT_MODEL));
    }

    #[test]
    fn sigmoid单调且有界() {
        assert!(sigmoid(-10.0) < sigmoid(0.0) && sigmoid(0.0) < sigmoid(10.0));
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(100.0) <= 1.0 && sigmoid(-100.0) >= 0.0);
    }

    #[test]
    fn 片段按字符截断() {
        let s = "字".repeat(2000);
        assert_eq!(clip(&s).chars().count(), MAX_DOC_CHARS);
    }

    #[tokio::test]
    async fn 未安装模型时返回缺失错误() {
        let d = tempfile::tempdir().unwrap();
        let r = Reranker::default();
        let e = r
            .score(
                d.path(),
                DEFAULT_MODEL,
                "问",
                vec!["答".into()],
                Duration::from_secs(1),
            )
            .await
            .unwrap_err();
        assert!(matches!(e, AppError::ModelMissing(_)));
    }

    /// 真实模型:需要联网下载约 280MB,手动运行
    /// `cargo test rerank::tests::真实模型 -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn 真实模型排序符合直觉() {
        let root = std::env::temp_dir().join("memflow_rerank_real");
        let http = reqwest::Client::new();
        let cancel = AtomicBool::new(false);
        download(
            &http,
            "https://hf-mirror.com",
            &root,
            DEFAULT_MODEL,
            &cancel,
            |p| {
                if p.completed == p.total {
                    println!("下载完成 {}", p.status)
                }
            },
        )
        .await
        .unwrap();
        let r = Reranker::default();
        let t0 = std::time::Instant::now();
        let docs = vec![
            "今天天气不错,适合去公园散步。".to_string(),
            "差旅费用报销需要在出差结束后七天内提交发票。".to_string(),
            "Rust 的所有权系统保证内存安全。".to_string(),
        ];
        let s = r
            .score(
                &root,
                DEFAULT_MODEL,
                "出差报销的期限是多久?",
                docs,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        println!("scores={s:?} 耗时 {:?}", t0.elapsed());
        assert!(s[1] > s[0] && s[1] > s[2], "{s:?}");
    }
}
