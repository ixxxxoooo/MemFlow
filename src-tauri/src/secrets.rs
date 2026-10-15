// @author ygw
//! 密钥存取:macOS 使用系统钥匙串,其他平台仅保留在内存中(不落盘)。
//! API Key 绝不写入 SQLite 的设置 JSON,也不会随备份导出。

#[cfg(target_os = "macos")]
use crate::error::AppError;
use crate::error::Result;

/// 钥匙串服务名
#[cfg(target_os = "macos")]
const SERVICE: &str = "com.memflow.app";
/// 钥匙串账户名
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "llm_api_key";

/// 读取已保存的云端 API Key
///
/// 返回: 存在返回 Some;不存在或读取失败返回 None
#[cfg(target_os = "macos")]
pub fn load_api_key() -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).ok()?;
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

/// 保存云端 API Key;传入空串表示删除
///
/// 参数: key 密钥
/// 返回: 成功返回 Ok
#[cfg(target_os = "macos")]
pub fn save_api_key(key: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| AppError::Other(format!("打开钥匙串失败: {e}")))?;
    if key.trim().is_empty() {
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
pub fn load_api_key() -> Option<String> {
    None
}

/// 非 macOS 平台:不持久化
#[cfg(not(target_os = "macos"))]
pub fn save_api_key(_key: &str) -> Result<()> {
    Ok(())
}
