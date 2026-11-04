// 视频压缩工具 Tauri — 入口
// 引擎配方真相 = 旧 Swift 工程 src/Engine.swift（2026-11-03），逐条移植，参数一字不改。
pub mod engine;
mod jobs;
mod commands;

pub use jobs::JobManager;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let manager = JobManager::new();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(manager)
        .on_window_event(|window, event| {
            // 红线：窗口关闭前取消全部 job（q → 等 3s → kill），唔准留 ffmpeg 孤儿进程
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let mgr = window.app_handle().state::<JobManager>();
                mgr.cancel_all();
                mgr.wait_batch(std::time::Duration::from_secs(4));
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_files,
            commands::start_jobs,
            commands::cancel_all,
            commands::verify_lossless,
            commands::media_info,
            commands::probe_info,
            commands::ffmpeg_info,
            commands::preview_frame
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
