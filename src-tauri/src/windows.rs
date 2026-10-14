// @author ygw
//! 窗口管理:主工作台、闪念胶囊、托盘、全局快捷键、macOS 毛玻璃。

use crate::error::{AppError, Result};
use std::sync::atomic::{AtomicI64, Ordering};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// 胶囊最近一次显示的时间(毫秒),用于忽略刚显示时的瞬时失焦
static CAPSULE_SHOWN_AT: AtomicI64 = AtomicI64::new(0);
/// 提示窗口的显示代次,用于连续提示时避免被上一次的定时隐藏误关
static TOAST_GEN: AtomicI64 = AtomicI64::new(0);
/// 胶囊固定宽度(逻辑像素)
const CAPSULE_WIDTH: f64 = 640.0;

/// 显示并聚焦主工作台
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 把胶囊定位到鼠标所在屏幕的上方居中
fn position_capsule(app: &AppHandle, w: &tauri::WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (monitor, w.outer_size()) else {
        return;
    };
    let x = m.position().x + (m.size().width as i32 - size.width as i32) / 2;
    let y = m.position().y + (m.size().height as f64 * 0.16) as i32;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// 显示闪念胶囊并通知前端聚焦输入框
pub fn show_capsule(app: &AppHandle) {
    let Some(w) = app.get_webview_window("capsule") else {
        return;
    };
    position_capsule(app, &w);
    CAPSULE_SHOWN_AT.store(crate::db::now_ms(), Ordering::Relaxed);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to("capsule", "capsule-shown", ());
}

/// 隐藏闪念胶囊
pub fn hide_capsule(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("capsule") {
        let _ = w.hide();
    }
}

/// 切换胶囊显示状态
pub fn toggle_capsule(app: &AppHandle) {
    let visible = app
        .get_webview_window("capsule")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if visible {
        hide_capsule(app);
    } else {
        show_capsule(app);
    }
}

/// 调整胶囊高度
pub fn resize_capsule(app: &AppHandle, height: f64) {
    if let Some(w) = app.get_webview_window("capsule") {
        let _ = w.set_size(LogicalSize::new(CAPSULE_WIDTH, height));
    }
}

/// 胶囊失焦处理:忽略刚显示的 400ms 内的失焦,其余情况自动收起
pub fn on_capsule_blur(app: &AppHandle) {
    if crate::db::now_ms() - CAPSULE_SHOWN_AT.load(Ordering::Relaxed) > 400 {
        hide_capsule(app);
    }
}

/// 在屏幕右下角闪现一条轻量提示(不抢焦点、不拦截鼠标),约 1.8 秒后自动消失
///
/// 参数: app 应用句柄;message 提示文案
pub fn show_toast(app: &AppHandle, message: &str) {
    let Some(w) = app.get_webview_window("toast") else {
        return;
    };
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let (Some(m), Ok(size)) = (monitor, w.outer_size()) {
        let scale = m.scale_factor();
        let x = m.position().x + m.size().width as i32 - size.width as i32 - (24.0 * scale) as i32;
        let y =
            m.position().y + m.size().height as i32 - size.height as i32 - (72.0 * scale) as i32;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.set_ignore_cursor_events(true);
    let _ = w.show();
    let _ = app.emit_to("toast", "toast-message", message.to_string());
    let gen = TOAST_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1900)).await;
        if TOAST_GEN.load(Ordering::SeqCst) == gen {
            if let Some(w) = handle.get_webview_window("toast") {
                let _ = w.hide();
            }
        }
    });
}

/// 注册全局快捷键(先注销旧的;失败时尝试恢复旧快捷键)
///
/// 参数: app 应用句柄;new 新快捷键;old 旧快捷键(用于失败回滚)
/// 返回: 成功 Ok;格式非法或被占用时返回 Invalid
pub fn register_shortcut(app: &AppHandle, new: &str, old: Option<&str>) -> Result<()> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if let Err(e) = gs.register(new) {
        if let Some(o) = old {
            let _ = gs.register(o);
        }
        return Err(AppError::Invalid(format!(
            "快捷键「{new}」注册失败(可能格式有误或已被占用): {e}"
        )));
    }
    Ok(())
}

/// 构造全局快捷键插件(按下即切换胶囊)
pub fn shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_capsule(app);
            }
        })
        .build()
}

/// 为窗口应用 macOS 毛玻璃
#[cfg(target_os = "macos")]
fn apply_vibrancy(app: &AppHandle) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
    if let Some(w) = app.get_webview_window("main") {
        let _ = apply_vibrancy(&w, NSVisualEffectMaterial::Sidebar, None, None);
    }
    if let Some(w) = app.get_webview_window("capsule") {
        let _ = apply_vibrancy(&w, NSVisualEffectMaterial::HudWindow, None, Some(18.0));
    }
}

/// 创建菜单栏托盘
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let capsule = MenuItem::with_id(app, "capsule", "闪念胶囊", true, None::<&str>)?;
    let main = MenuItem::with_id(app, "main", "打开工作台", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 MemFlow", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&capsule, &main, &sep, &quit])?;
    let mut builder = TrayIconBuilder::with_id("memflow-tray")
        .menu(&menu)
        .tooltip("MemFlow")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "capsule" => show_capsule(app),
            "main" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")) {
        builder = builder.icon(icon).icon_as_template(true);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// 初始化窗口外观、托盘与快捷键
///
/// 参数: app 应用句柄;shortcut 配置的快捷键
pub fn setup(app: &AppHandle, shortcut: &str) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    apply_vibrancy(app);
    setup_tray(app)?;
    if let Err(e) = register_shortcut(app, shortcut, None) {
        tracing::warn!("{e}");
    }
    Ok(())
}
