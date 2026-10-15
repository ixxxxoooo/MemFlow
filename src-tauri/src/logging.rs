// @author ixxxxoooo
//! 日志:按天滚动写入文件(保留 7 天),同时输出到终端;并接管 panic 与前端错误。

use std::path::PathBuf;
use std::sync::OnceLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};

/// 日志保留天数
const KEEP_DAYS: usize = 7;

/// 非阻塞写入线程的守卫,必须在进程生命周期内保持存活,否则缓冲日志会丢失
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 日志目录:macOS 为 `~/Library/Logs/MemFlow`,Windows 为 `%LOCALAPPDATA%\\MemFlow\\logs`,
/// 其它平台为 `~/.memflow/logs`
pub fn default_log_dir() -> PathBuf {
    let home = crate::model_fetch::home_dir();
    let home = if home.as_os_str().is_empty() {
        std::env::temp_dir()
    } else {
        home
    };
    if cfg!(target_os = "macos") {
        home.join("Library").join("Logs").join("MemFlow")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("MemFlow")
            .join("logs")
    } else {
        home.join(".memflow").join("logs")
    }
}

/// 当前生效的日志目录(未初始化文件日志时返回默认目录)
pub fn log_dir() -> PathBuf {
    LOG_DIR.get().cloned().unwrap_or_else(default_log_dir)
}

/// 初始化日志(重复调用安全)。文件日志失败时仅降级为终端输出,不影响应用启动。
pub fn init() {
    init_in(default_log_dir());
}

/// 在指定目录初始化日志,便于测试
pub fn init_in(dir: PathBuf) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "memflow_lib=info".into());
    let stdout_layer = tracing_subscriber::fmt::layer();

    let file_layer = std::fs::create_dir_all(&dir)
        .ok()
        .and_then(|_| {
            Builder::new()
                .rotation(Rotation::DAILY)
                .filename_prefix("memflow")
                .filename_suffix("log")
                .max_log_files(KEEP_DAYS)
                .build(&dir)
                .ok()
        })
        .map(|appender| {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let _ = GUARD.set(guard);
            let _ = LOG_DIR.set(dir.clone());
            // 文件里不要 ANSI 颜色码
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer)
        });

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init();

    install_panic_hook();
}

/// 把 panic 写入日志(含位置与线程名),再交给默认钩子
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        tracing::error!(
            thread = thread.name().unwrap_or("<unnamed>"),
            "程序 panic: {info}"
        );
        default(info);
    }));
}

/// 记录来自前端的日志,限制长度避免被滥用
pub fn log_frontend(level: &str, message: &str) {
    let msg: String = message.chars().take(4000).collect();
    match level {
        "error" => tracing::error!(target: "memflow_lib::frontend", "{msg}"),
        "warn" => tracing::warn!(target: "memflow_lib::frontend", "{msg}"),
        _ => tracing::info!(target: "memflow_lib::frontend", "{msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 日志目录在macos下位于library_logs() {
        let d = default_log_dir();
        if cfg!(target_os = "macos") {
            assert!(d.ends_with("Library/Logs/MemFlow"));
        }
    }

    #[test]
    fn 前端日志超长会被截断而不panic() {
        log_frontend("error", &"错".repeat(10_000));
        log_frontend("unknown", "x");
    }
}
