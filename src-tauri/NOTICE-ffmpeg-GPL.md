# 关于 ffmpeg sidecar 的许可证说明

本应用通过 **独立可执行文件**（sidecar）方式调用 ffmpeg / ffprobe，**未将 ffmpeg 静态链入应用本体**：

- 二进制：`Contents/MacOS/ffmpeg-aarch64-apple-darwin`、`Contents/MacOS/ffprobe-aarch64-apple-darwin`
- 来源：[martin-riedl.de FFmpeg Build Server](https://ffmpeg.martin-riedl.de)
  固定版本 **9.0.2 release**（2026-09-20 构建），macOS arm64 静态构建
  - ffmpeg.zip SHA-256: `c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924`
  - ffprobe.zip SHA-256: `fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6`
  - 校验脚本：`scripts/fetch-ffmpeg`（固定版本 + SHA-256 + 验证 libx265 编码器存在）

## 许可证

该构建启用了 **GPL 许可证的组件**（libx264、libx265），因此该 ffmpeg 二进制本身以 **GPL v2（或更高版本）** 授权。
按 GPL 要求：

1. 本应用随包提供 GPL 许可证全文：见 `LICENSE-GPLv2.txt`。
2. **源码获取方式**：
   - ffmpeg 构建配置与脚本（开源）：https://git.martin-riedl.de/ffmpeg/build-script
   - ffmpeg 上游源码：https://ffmpeg.org（版本 9.0.2，对应 2026-09 发布）
   - libx264：http://www.videolan.org/developers/x264.html
   - libx265：https://github.com/videolan/x265
3. ffmpeg 与主程序保持**独立可执行文件**（sidecar），本应用仅以子进程 + 命令行参数方式调用它，
   不向 ffmpeg 传递 GPL 代码，主程序本身（Tauri/React）保持 MIT 授权。
