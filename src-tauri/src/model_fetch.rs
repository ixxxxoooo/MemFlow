// @author ygw
//! 直连下载 Ollama 模型:绕过 Ollama 服务端的重定向安全检查。
//!
//! 背景:代理工具(如 Quantumult X)开启 fake-ip 后,`ollama pull` 会因为 CDN 域名被解析到
//! `198.18.x.x` 而报 `redirect target not allowed`。本模块在应用进程内直接按 Docker Registry
//! 协议下载 manifest 与各 blob(断点续传 + sha256 校验),再写入 Ollama 的模型目录,
//! Ollama 无需重启即可识别。
//!
//! 与 `scripts/pull_model_manual.sh` 的逻辑保持一致,但不依赖脚本文件与 python。

use crate::error::{AppError, Result};
use crate::ollama::PullProgress;
use futures_util::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

/// 单个 blob 最多重试次数(每次都会断点续传)
const MAX_ATTEMPTS: u32 = 12;
/// 读取数据流的停滞超时
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
/// 进度回调的最小间隔
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Ollama 在本地模型目录中固定使用的仓库主机目录名(与下载来自哪个镜像无关)
const LOCAL_REGISTRY_DIR: &str = "registry.ollama.ai";

/// 模型引用(命名空间/名称:标签)
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRef {
    /// 命名空间,官方模型为 library
    pub namespace: String,
    /// 模型名
    pub repo: String,
    /// 标签
    pub tag: String,
}

/// manifest 中的一层(含 config)
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// 摘要,形如 sha256:xxxx
    pub digest: String,
    /// 字节数
    pub size: u64,
}

/// 路径片段是否安全(防止目录穿越)
fn safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// 解析模型名
///
/// 参数: name 如 `bge-m3`、`qwen2.5:7b`、`someone/model:tag`
/// 返回: 模型引用;不支持的格式(如 hf.co 前缀)返回 Invalid
pub fn parse_model_ref(name: &str) -> Result<ModelRef> {
    let name = name.trim();
    let (path, tag) = match name.rsplit_once(':') {
        Some((p, t)) if !t.contains('/') => (p, t),
        _ => (name, "latest"),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let (ns, repo) = match parts.as_slice() {
        [repo] => ("library", *repo),
        [ns, repo] => (*ns, *repo),
        _ => {
            return Err(AppError::Invalid(
                "直连下载只支持 Ollama 官方仓库的模型,如 qwen2.5:7b".into(),
            ))
        }
    };
    if ns.contains('.') && ns != "library" {
        // 形如 hf.co/xxx 的第三方仓库
        return Err(AppError::Invalid(
            "直连下载只支持 Ollama 官方仓库的模型,如 qwen2.5:7b".into(),
        ));
    }
    if !(safe_segment(ns) && safe_segment(repo) && safe_segment(tag)) {
        return Err(AppError::Invalid("模型名包含非法字符".into()));
    }
    Ok(ModelRef {
        namespace: ns.to_string(),
        repo: repo.to_string(),
        tag: tag.to_string(),
    })
}

/// 摘要是否合法(sha256:64 位小写十六进制)
pub fn valid_digest(d: &str) -> bool {
    d.strip_prefix("sha256:")
        .map(|h| h.len() == 64 && h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')))
        .unwrap_or(false)
}

/// 解析 manifest,取出 config 与全部 layer
///
/// 参数: raw manifest 原文
/// 返回: 需要下载的层列表
pub fn parse_manifest(raw: &str) -> Result<Vec<Layer>> {
    let v: Value = serde_json::from_str(raw)
        .map_err(|e| AppError::Parse(format!("manifest 不是合法 JSON: {e}")))?;
    let mut out = Vec::new();
    let mut push = |item: &Value| -> Result<()> {
        let digest = item
            .get("digest")
            .and_then(|d| d.as_str())
            .ok_or_else(|| AppError::Parse("manifest 缺少 digest".into()))?;
        if !valid_digest(digest) {
            return Err(AppError::Parse(format!("manifest 含非法 digest: {digest}")));
        }
        let size = item.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
        out.push(Layer {
            digest: digest.to_string(),
            size,
        });
        Ok(())
    };
    if let Some(c) = v.get("config") {
        push(c)?;
    }
    let layers = v
        .get("layers")
        .and_then(|l| l.as_array())
        .ok_or_else(|| AppError::Parse("manifest 缺少 layers".into()))?;
    for l in layers {
        push(l)?;
    }
    Ok(out)
}

/// Ollama 默认模型目录(优先读取 OLLAMA_MODELS)
///
/// 返回: 模型根目录
pub fn default_models_root() -> PathBuf {
    if let Ok(p) = std::env::var("OLLAMA_MODELS") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".ollama").join("models")
}

/// 计算文件 sha256(阻塞,需在 spawn_blocking 中调用)
fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// 进度节流器:避免每个数据块都推送给前端
struct Throttle {
    last: Instant,
}

impl Throttle {
    fn new() -> Self {
        Throttle {
            last: Instant::now() - PROGRESS_INTERVAL,
        }
    }
    fn ready(&mut self) -> bool {
        if self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            true
        } else {
            false
        }
    }
}

/// 下载单个 Ollama blob:断点续传、停滞重试、sha256 校验
///
/// 参数: http 客户端;url 下载地址;fin 最终路径;layer 层信息;span (之前已完成字节, 总字节);
/// cancel 取消标志;progress 进度回调
async fn download_blob(
    http: &reqwest::Client,
    url: &str,
    fin: &Path,
    layer: &Layer,
    span: (u64, u64),
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(PullProgress) + Send),
) -> Result<()> {
    let hex = layer.digest.trim_start_matches("sha256:").to_string();
    let label = format!("下载 {}", &hex[..12]);
    download_file(
        http,
        url,
        fin,
        layer.size,
        Some(&hex),
        &label,
        span,
        cancel,
        progress,
    )
    .await
}

/// 下载单个文件:断点续传、停滞重试、可选 sha256 校验,通过校验后才改名到位
///
/// 参数: http 客户端;url 下载地址;fin 最终路径;size 期望字节数(0 表示未知);
/// sha256 期望摘要(None 则只校验大小);label 进度文案;span (之前已完成字节, 总字节);
/// cancel 取消标志;progress 进度回调
#[allow(clippy::too_many_arguments)]
pub async fn download_file(
    http: &reqwest::Client,
    url: &str,
    fin: &Path,
    size: u64,
    sha256: Option<&str>,
    label: &str,
    span: (u64, u64),
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(PullProgress) + Send),
) -> Result<()> {
    let layer = Layer {
        digest: String::new(),
        size,
    };
    let label = label.to_string();
    // span = (之前已完成字节数, 总字节数)
    let (done_before, total) = span;
    let part = PathBuf::from(format!("{}.part", fin.display()));
    let mut throttle = Throttle::new();
    let mut last_err = String::new();

    for attempt in 0..MAX_ATTEMPTS {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let mut have = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if layer.size > 0 && have > layer.size {
            let _ = tokio::fs::remove_file(&part).await;
            have = 0;
        }
        if layer.size == 0 || have < layer.size {
            let mut req = http.get(url);
            if have > 0 {
                req = req.header("Range", format!("bytes={have}-"));
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    last_err = e.to_string();
                    continue;
                }
            };
            let status = resp.status().as_u16();
            match status {
                200 | 206 => {}
                416 => {
                    // 本地已有内容超出或等于远端:丢弃后重来
                    let _ = tokio::fs::remove_file(&part).await;
                    last_err = "服务端返回 416,已清理半成品重试".into();
                    continue;
                }
                404 => {
                    return Err(AppError::Other(
                        "模型文件不存在(404),请检查模型名与镜像源".into(),
                    ))
                }
                s => {
                    last_err = format!("HTTP {s}");
                    continue;
                }
            }
            // 服务端忽略了 Range(返回 200)时必须从头写
            let append = status == 206 && have > 0;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&part)
                .await?;
            if !append {
                have = 0;
            }
            let mut written = have;
            let mut stream = resp.bytes_stream();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    let _ = file.flush().await;
                    return Err(AppError::Cancelled);
                }
                match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
                    Err(_) => {
                        last_err = "下载停滞超过 30 秒".into();
                        break;
                    }
                    Ok(None) => break,
                    Ok(Some(Err(e))) => {
                        last_err = e.to_string();
                        break;
                    }
                    Ok(Some(Ok(chunk))) => {
                        file.write_all(&chunk).await?;
                        written += chunk.len() as u64;
                        if throttle.ready() {
                            progress(PullProgress {
                                status: label.clone(),
                                total,
                                completed: done_before + written,
                            });
                        }
                    }
                }
            }
            file.flush().await?;
        }
        let have = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if layer.size == 0 || have == layer.size {
            break;
        }
        if attempt + 1 == MAX_ATTEMPTS {
            return Err(AppError::Other(format!(
                "下载未完成({have}/{}),{last_err}",
                layer.size
            )));
        }
    }

    let Some(hex) = sha256.map(str::to_string) else {
        tokio::fs::rename(&part, fin).await?;
        return Ok(());
    };
    progress(PullProgress {
        status: "校验中".into(),
        total,
        completed: done_before + layer.size,
    });
    let part2 = part.clone();
    let got = tokio::task::spawn_blocking(move || sha256_file(&part2))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    if got != hex {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(AppError::Other(format!(
            "sha256 校验失败(期望 {hex},实际 {got}),已清理,请重试"
        )));
    }
    tokio::fs::rename(&part, fin).await?;
    Ok(())
}

/// 直连下载一个模型
///
/// 参数: http 无总超时的客户端;registry 仓库地址(可为镜像);models_root Ollama 模型目录;
/// name 模型名;cancel 取消标志;on_progress 进度回调
/// 返回: 成功返回 Ok
pub async fn pull_direct(
    http: &reqwest::Client,
    registry: &str,
    models_root: &Path,
    name: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(PullProgress) + Send,
) -> Result<()> {
    let mref = parse_model_ref(name)?;
    let base = format!(
        "{}/v2/{}/{}",
        registry.trim().trim_end_matches('/'),
        mref.namespace,
        mref.repo
    );

    on_progress(PullProgress {
        status: "获取模型清单".into(),
        total: 0,
        completed: 0,
    });
    let resp = http
        .get(format!("{base}/manifests/{}", mref.tag))
        .header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json",
        )
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::Other(format!("获取模型清单失败: {e}")))?;
    let status = resp.status().as_u16();
    if status == 404 {
        return Err(AppError::Other(format!("模型 {name} 不存在(404)")));
    }
    if !(200..300).contains(&status) {
        return Err(AppError::Other(format!("获取模型清单失败: HTTP {status}")));
    }
    let manifest = resp
        .text()
        .await
        .map_err(|e| AppError::Other(format!("读取模型清单失败: {e}")))?;
    let layers = parse_manifest(&manifest)?;
    let total: u64 = layers.iter().map(|l| l.size).sum();

    let blobs = models_root.join("blobs");
    tokio::fs::create_dir_all(&blobs).await?;
    let mut done: u64 = 0;
    for layer in &layers {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let fin = blobs.join(layer.digest.replace(':', "-"));
        // 已存在且大小一致:该文件只会在校验通过后才被重命名到位,直接跳过
        if let Ok(m) = tokio::fs::metadata(&fin).await {
            if m.len() == layer.size {
                done += layer.size;
                on_progress(PullProgress {
                    status: "已存在,跳过".into(),
                    total,
                    completed: done,
                });
                continue;
            }
        }
        let url = format!("{base}/blobs/{}", layer.digest);
        download_blob(
            http,
            &url,
            &fin,
            layer,
            (done, total),
            cancel,
            &mut on_progress,
        )
        .await?;
        done += layer.size;
    }

    // 所有 blob 就绪后才写 manifest,保证 Ollama 看到的模型总是完整的
    let mf_dir = models_root
        .join("manifests")
        .join(LOCAL_REGISTRY_DIR)
        .join(&mref.namespace)
        .join(&mref.repo);
    tokio::fs::create_dir_all(&mf_dir).await?;
    let tmp = mf_dir.join(format!("{}.tmp", mref.tag));
    tokio::fs::write(&tmp, manifest.as_bytes()).await?;
    tokio::fs::rename(&tmp, mf_dir.join(&mref.tag)).await?;
    on_progress(PullProgress {
        status: "success".into(),
        total,
        completed: total,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析模型名() {
        let r = parse_model_ref("bge-m3").unwrap();
        assert_eq!(
            (r.namespace.as_str(), r.repo.as_str(), r.tag.as_str()),
            ("library", "bge-m3", "latest")
        );
        let r = parse_model_ref("qwen2.5:7b").unwrap();
        assert_eq!((r.repo.as_str(), r.tag.as_str()), ("qwen2.5", "7b"));
        let r = parse_model_ref("someone/model:q4").unwrap();
        assert_eq!(r.namespace, "someone");
    }

    #[test]
    fn 拒绝不支持或危险的模型名() {
        assert!(parse_model_ref("hf.co/user/model:Q4").is_err());
        assert!(parse_model_ref("../etc:passwd").is_err());
        assert!(parse_model_ref("a/b/c").is_err());
        assert!(parse_model_ref("").is_err());
        assert!(parse_model_ref("m:..").is_err());
    }

    #[test]
    fn 校验摘要格式() {
        let ok = format!("sha256:{}", "a".repeat(64));
        assert!(valid_digest(&ok));
        assert!(!valid_digest("sha256:xyz"));
        assert!(!valid_digest(&format!("md5:{}", "a".repeat(64))));
        assert!(!valid_digest(&format!("sha256:{}", "A".repeat(64))));
    }

    #[test]
    fn 解析manifest() {
        let d1 = format!("sha256:{}", "1".repeat(64));
        let d2 = format!("sha256:{}", "2".repeat(64));
        let raw = format!(
            r#"{{"config":{{"digest":"{d1}","size":10}},"layers":[{{"digest":"{d2}","size":100}}]}}"#
        );
        let l = parse_manifest(&raw).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[1].size, 100);
        assert!(parse_manifest("{").is_err());
        assert!(parse_manifest(r#"{"layers":[{"digest":"bad"}]}"#).is_err());
    }
}
