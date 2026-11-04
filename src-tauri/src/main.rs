// 视频压缩工具 Tauri — macOS / Windows 主入口
// 单例逻辑全部喺 vct_lib::run() 嘅 tauri-plugin-single-instance（二期）：
// 第二个实例（例如 Finder Quick Action 喺 App 已运行时直接 exec）会把自己嘅 argv 交给
// 运行中实例嘅 init closure（发 vct://pending-files 事件入队），然后自动退出。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vct_lib::run()
}
