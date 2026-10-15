// @author ixxxxoooo
//! 密钥存取:使用系统凭据存储(macOS 钥匙串 / Windows 凭据管理器 / Linux Secret Service)。
//! API Key 绝不写入 SQLite 的设置 JSON,也不会随备份导出。

use crate::error::{AppError, Result};

/// 凭据服务名:与 bundle 标识保持一致(用 GitHub 用户名命名)
const SERVICE: &str = "com.ixxxxoooo.memflow";
/// 旧版本使用的服务名,读取时自动迁移到新服务名(仅 macOS 有历史数据)
#[cfg(target_os = "macos")]
const LEGACY_SERVICE: &str = "com.memflow.app";
/// 默认云端模型使用的档案标识(沿用历史账户名,保证升级后无需重新填写密钥)
pub const DEFAULT_PROFILE: &str = "default";

/// 读取钥匙串的结果:区分"确实没有"与"有但读不出来",避免把读取失败当成未保存
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyLoad {
    /// 读到了密钥
    Found(String),
    /// 钥匙串中确实没有这条密钥
    Missing,
    /// 读取失败(用户拒绝授权、钥匙串被锁定等),附带可展示的原因
    Failed(String),
}

/// 钥匙串账户名:默认档案沿用 `llm_api_key`,其余档案为 `llm_api_key:<id>`
pub fn account_of(profile: &str) -> String {
    if profile.is_empty() || profile == DEFAULT_PROFILE {
        "llm_api_key".to_string()
    } else {
        format!("llm_api_key:{profile}")
    }
}

/// 读取已保存的云端 API Key(macOS 会触发钥匙串授权弹窗,只应在真正调用云端时使用)
///
/// 参数: profile 档案标识(default 为默认云端模型)
/// 返回: Found / Missing / Failed。读取失败**不等于**未保存,调用方不应缓存失败结果;
/// macOS 上旧服务名下的密钥会迁移到新服务名
pub fn load_api_key(profile: &str) -> KeyLoad {
    let account = account_of(profile);
    let found = read_entry(SERVICE, &account);
    #[cfg(target_os = "macos")]
    {
        if found != KeyLoad::Missing {
            return found;
        }
        // 兼容旧版本:从旧服务名读出后写入新服务名并删除旧条目,之后不再触碰旧条目
        let legacy = match read_entry(LEGACY_SERVICE, &account) {
            KeyLoad::Found(v) => v,
            other => return other,
        };
        if let Ok(e) = keyring::Entry::new(SERVICE, &account) {
            if e.set_password(&legacy).is_ok() {
                if let Ok(old) = keyring::Entry::new(LEGACY_SERVICE, &account) {
                    let _ = old.delete_credential();
                }
            }
        }
        KeyLoad::Found(legacy)
    }
    #[cfg(not(target_os = "macos"))]
    found
}

fn read_entry(service: &str, account: &str) -> KeyLoad {
    let entry = match keyring::Entry::new(service, account) {
        Ok(e) => e,
        Err(e) => return KeyLoad::Failed(describe_error(&e)),
    };
    match entry.get_password() {
        Ok(p) if !p.trim().is_empty() => KeyLoad::Found(p),
        Ok(_) => KeyLoad::Missing,
        Err(keyring::Error::NoEntry) => KeyLoad::Missing,
        Err(e) => {
            tracing::warn!("读取系统凭据存储失败: {e}");
            KeyLoad::Failed(describe_error(&e))
        }
    }
}

/// 把凭据存储错误翻译成用户能看懂、并告诉他怎么做的中文说明
fn describe_error(e: &keyring::Error) -> String {
    let raw = e.to_string();
    if raw.contains("user name or passphrase")
        || raw.contains("denied")
        || raw.contains("User interaction")
    {
        "凭据存储授权被拒绝。请重试并在弹窗中点「始终允许」,或重新输入 API Key 保存".to_string()
    } else if cfg!(target_os = "linux") {
        format!("无法访问系统密钥环({raw})。请确认已安装并解锁 GNOME Keyring / KWallet 等 Secret Service 服务,或重新输入 API Key 保存")
    } else {
        format!("暂时无法读取系统凭据存储({raw})。请确认已解锁后重试,或重新输入 API Key 保存")
    }
}

/// 判断密钥是否已保存。
///
/// macOS 通过系统自带的 `security find-generic-password`(不带 -w/-g)只查条目是否存在,
/// **不读取密钥内容,因此不会弹出钥匙串授权窗口**;Windows / Linux 读取凭据不会弹窗,直接读取判断。
#[cfg(target_os = "macos")]
pub fn key_exists(profile: &str) -> bool {
    let account = account_of(profile);
    [SERVICE, LEGACY_SERVICE].iter().any(|svc| {
        std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", svc, "-a", &account])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// 判断密钥是否已保存(见 macOS 版本说明)
#[cfg(not(target_os = "macos"))]
pub fn key_exists(profile: &str) -> bool {
    matches!(read_entry(SERVICE, &account_of(profile)), KeyLoad::Found(_))
}

/// 保存云端 API Key;传入空串表示删除
///
/// 参数: profile 档案标识;key 密钥
/// 返回: 成功返回 Ok
pub fn save_api_key(profile: &str, key: &str) -> Result<()> {
    let account = account_of(profile);
    let entry = keyring::Entry::new(SERVICE, &account)
        .map_err(|e| AppError::Other(format!("打开系统凭据存储失败: {e}")))?;
    if key.trim().is_empty() {
        // 同时清理旧服务名下可能残留的条目
        #[cfg(target_os = "macos")]
        if let Ok(old) = keyring::Entry::new(LEGACY_SERVICE, &account) {
            let _ = old.delete_credential();
        }
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Other(format!("清除系统凭据失败: {e}"))),
        };
    }
    entry
        .set_password(key.trim())
        .map_err(|e| AppError::Other(format!("写入系统凭据存储失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 读取结果能区分缺失与失败() {
        assert_ne!(KeyLoad::Missing, KeyLoad::Failed("x".into()));
        assert_eq!(KeyLoad::Found("k".into()), KeyLoad::Found("k".into()));
    }

    #[test]
    fn 默认档案沿用历史账户名() {
        assert_eq!(account_of("default"), "llm_api_key");
        assert_eq!(account_of(""), "llm_api_key");
        assert_eq!(account_of("p1"), "llm_api_key:p1");
    }
}
