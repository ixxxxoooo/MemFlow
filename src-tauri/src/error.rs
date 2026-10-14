// @author ygw
//! 统一错误类型:区分"可无限重试的临时不可用"与"需要计次重试的业务错误"。

use serde::{Serialize, Serializer};

/// 应用统一错误
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 数据库错误
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),
    /// 文件系统错误
    #[error("文件错误: {0}")]
    Io(#[from] std::io::Error),
    /// Ollama 未运行 / 连接失败 / 超时(临时性,可无限重试)
    #[error("Ollama 不可用: {0}")]
    OllamaUnavailable(String),
    /// 所需模型尚未下载(临时性,用户拉取后即可恢复)
    #[error("模型未安装: {0}")]
    ModelMissing(String),
    /// Ollama 返回了业务错误
    #[error("Ollama 返回错误: {0}")]
    Ollama(String),
    /// 模型输出或文件内容解析失败
    #[error("解析失败: {0}")]
    Parse(String),
    /// 参数不合法
    #[error("参数无效: {0}")]
    Invalid(String),
    /// 任务被用户取消
    #[error("已取消")]
    Cancelled,
    /// 其他错误
    #[error("{0}")]
    Other(String),
}

/// 统一结果类型
pub type Result<T> = std::result::Result<T, AppError>;

impl AppError {
    /// 判断是否属于"临时不可用"(Ollama 没开 / 模型没装),此类错误重试不消耗次数
    ///
    /// 返回: true 表示应延迟后重试且不计入失败次数
    pub fn is_unavailable(&self) -> bool {
        matches!(
            self,
            AppError::OllamaUnavailable(_) | AppError::ModelMissing(_)
        )
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Parse(e.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
