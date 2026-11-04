// 视频压缩工具 Tauri — 入口
// 引擎配方真相 = 旧 Swift 工程 src/Engine.swift（2026-11-03），逐条移植，参数一字不改。
pub mod engine;
mod jobs;
mod commands;

pub use jobs::JobManager;

use tauri::Manager;
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let manager = JobManager::new();
    tauri::Builder::default()
        // 单例放最前（插件注册顺序 = 执行顺序：第二个实例要先喺其他 setup 之前被检测）：
        // Quick Action 直接 exec 起嘅二进程会把手上 argv 交给呢度，然后自动退出。
        .plugin(tauri_plugin_single_instance::init(|app_handle, argv, _cwd| {
            let files = commands::files_from_args(&argv);
            if !files.is_empty() {
                let _ = app_handle.emit("vct://pending-files", &files);
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.set_focus();
                }
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(manager)
        .setup(|_app| {
            // 冷启动：读 argv 中真实文件路径 → pending，前端 mount 后 take_launch_files 取走入队
            let argv: Vec<String> = std::env::args().collect();
            let files = commands::files_from_args(&argv);
            if !files.is_empty() {
                eprintln!("[vct] 冷启动 argv 文件入 pending: {files:?}");
                *commands::pending().lock().unwrap() = files;
            }
            Ok(())
        })
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
            commands::preview_frame,
            commands::take_launch_files
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
