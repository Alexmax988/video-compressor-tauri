// 视频压缩工具 Tauri — macOS 主入口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vct_lib::run()
}
