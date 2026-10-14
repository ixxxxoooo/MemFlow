// @author ygw
//! MemFlow:本地 AI 第二大脑(Tauri 2 + Rust 后端)。

pub mod chat;
pub mod chunker;
pub mod commands;
pub mod core;
pub mod db;
pub mod error;
pub mod extract;
pub mod ingest;
pub mod ollama;
pub mod pipeline;
pub mod prompts;
pub mod queue;
pub mod repo;
pub mod retrieval;
pub mod settings;
pub mod tokenizer;
pub mod vector;
pub mod windows;

use crate::core::{AppPaths, Core, EventSink};
use crate::db::Db;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Emitter, Manager, WindowEvent};

/// 把核心事件转发给所有前端窗口
struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
    }
}

/// 初始化日志(重复调用安全)
fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memflow_lib=info".into()),
        )
        .try_init();
}

/// 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let app = tauri::Builder::default()
        // 重复启动时唤起已有实例的主窗口
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(windows::shortcut_plugin())
        .setup(|app| {
            let root = app.path().data_dir()?.join("MemFlow");
            let paths = AppPaths::new(root)?;
            let db = Db::open(&paths.db_path, Some(&paths.backups))?;
            let core = Core::new(paths, db, Arc::new(TauriSink(app.handle().clone())))?;
            let shortcut = core.settings().shortcut;
            app.manage(core.clone());
            tauri::async_runtime::spawn(queue::run_worker(core));
            windows::setup(app.handle(), &shortcut)?;
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            // 关闭主窗口只是隐藏,应用继续在菜单栏常驻
            ("main", WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                let _ = window.hide();
            }
            ("capsule", WindowEvent::Focused(false)) => {
                windows::on_capsule_blur(window.app_handle())
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::ollama_start,
            commands::pull_model,
            commands::cancel_pull,
            commands::get_settings,
            commands::update_settings,
            commands::capture_submit,
            commands::stage_file,
            commands::list_items,
            commands::get_item,
            commands::list_tags,
            commands::update_item_content,
            commands::delete_items,
            commands::reindex,
            commands::reindex_ai,
            commands::retry_failed,
            commands::cancel_item,
            commands::chat_ask,
            commands::chat_cancel,
            commands::list_sessions,
            commands::get_session_messages,
            commands::delete_session,
            commands::fix_mermaid,
            commands::write_export,
            commands::export_backup,
            commands::show_main,
            commands::show_capsule,
            commands::hide_capsule,
            commands::resize_capsule,
            commands::notify_toast,
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败");

    app.run(|app, event| {
        // 点击 Dock 图标时重新显示主窗口
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            windows::show_main(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
