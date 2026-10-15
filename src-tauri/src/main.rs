// @author ygw
// 防止 Windows 发布版弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 被主进程拉起做 OCR 时,只跑识别循环,不启动界面
    if memflow_lib::ocr_worker::maybe_run_worker() {
        return;
    }
    memflow_lib::run()
}
