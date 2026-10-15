// @author ygw
//! 密钥存取:macOS 使用系统钥匙串,其他平台仅保留在内存中(不落盘)。
//! API Key 绝不写入 SQLite 的设置 JSON,也不会随备份导出。

#[cfg(target_os = "macos")]
use crate::error::AppError;
use crate::error::Result;

/// 钥匙串服务名:与 bundle 标识保持一致(用 GitHub 用户名命名)
#[cfg(target_os = "macos")]
const SERVICE: &str = "com.ixxxxoooo.memflow";
/// 旧版本使用的服务名,读取时自动迁移到新服务名
#[cfg(target_os = "macos")]
const LEGACY_SERVICE: &str = "com.memflow.app";
/// 默认云端模型使用的档案标识(沿用历史账户名,保证升级后无需重新填写密钥)
pub const DEFAULT_PROFILE: &str = "default";

/// 钥匙串账户名:默认档案沿用 `llm_api_key`,其余档案为 `llm_api_key:<id>`
pub fn account_of(profile: &str) -> String {
    if profile.is_empty() || profile == DEFAULT_PROFILE {
        "llm_api_key".to_string()
    } else {
        format!("llm_api_key:{profile}")
    }
}

/// 读取已保存的云端 API Key(会触发钥匙串授权弹窗,只应在真正调用云端时使用)
///
/// 参数: profile 档案标识(default 为默认云端模型)
/// 返回: 存在返回 Some;不存在或读取失败返回 None。旧服务名下的密钥会迁移到新服务名
#[cfg(target_os = "macos")]
pub fn load_api_key(profile: &str) -> Option<String> {
    let account = account_of(profile);
    if let Some(p) = read_entry(SERVICE, &account) {
        return Some(p);
    }
    // 兼容旧版本:从旧服务名读出后写入新服务名并删除旧条目,之后不再触碰旧条目
    let legacy = read_entry(LEGACY_SERVICE, &account)?;
    if let Ok(e) = keyring::Entry::new(SERVICE, &account) {
        if e.set_password(&legacy).is_ok() {
            if let Ok(old) = keyring::Entry::new(LEGACY_SERVICE, &account) {
                let _ = old.delete_credential();
            }
        }
    }
    Some(legacy)
}

#[cfg(target_os = "macos")]
fn read_entry(service: &str, account: &str) -> Option<String> {
    let entry = keyring::Entry::new(service, account).ok()?;
    match entry.get_password() {
        Ok(p) if !p.trim().is_empty() => Some(p),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            tracing::warn!("读取钥匙串失败: {e}");
            None
        }
    }
}

/// 判断密钥是否已保存,**不读取密钥内容,因此不会弹出钥匙串授权窗口**。
/// 通过系统自带的 `security find-generic-password`(不带 -w/-g)只查条目是否存在。
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

/// 保存云端 API Key;传入空串表示删除
///
/// 参数: profile 档案标识;key 密钥
/// 返回: 成功返回 Ok
#[cfg(target_os = "macos")]
pub fn save_api_key(profile: &str, key: &str) -> Result<()> {
    let account = account_of(profile);
    let entry = keyring::Entry::new(SERVICE, &account)
        .map_err(|e| AppError::Other(format!("打开钥匙串失败: {e}")))?;
    if key.trim().is_empty() {
        // 同时清理旧服务名下可能残留的条目
        if let Ok(old) = keyring::Entry::new(LEGACY_SERVICE, &account) {
            let _ = old.delete_credential();
        }
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Other(format!("清除钥匙串失败: {e}"))),
        };
    }
    entry
        .set_password(key.trim())
        .map_err(|e| AppError::Other(format!("写入钥匙串失败: {e}")))
}

/// 非 macOS 平台:不持久化
#[cfg(not(target_os = "macos"))]
pub fn load_api_key(_profile: &str) -> Option<String> {
    None
}

/// 非 macOS 平台:没有持久化,恒为 false
#[cfg(not(target_os = "macos"))]
pub fn key_exists(_profile: &str) -> bool {
    false
}

/// 非 macOS 平台:不持久化
#[cfg(not(target_os = "macos"))]
pub fn save_api_key(_profile: &str, _key: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认档案沿用历史账户名() {
        assert_eq!(account_of("default"), "llm_api_key");
        assert_eq!(account_of(""), "llm_api_key");
        assert_eq!(account_of("p1"), "llm_api_key:p1");
    }
}
