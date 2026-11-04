# 视频压缩工具（Tauri 版）

便携视频处理箱：macOS 桌面应用（Tauri 2 + React 19 + Rust），内置 ffmpeg / ffprobe sidecar，开箱即用、无需安装任何视频工具。

## 下载与安装

安装包统一发布在 [Releases](https://github.com/Alexmax988/video-compressor-tauri/releases)（tag 触发 GitHub Actions 自动构建，dmg / exe 挂 draft Release 后发布）。

- **macOS（Apple Silicon）**：下载 `*.dmg`，拖进「应用程序」即可。
- **Windows（x64）**：下载 `*-setup.exe`（NSIS 安装器），双击安装。

**关于签名（如实说明，本期未做代码签名 / 公证）：**

- macOS 首次打开会提示「无法验证开发者」——这是未签名应用的正常现象：右键点 App → **打开** → 再点「打开」即可，之后正常双击打开；
- Windows 首次运行安装器会有 **SmartScreen 提示**（「已保护你的电脑」）——点「更多信息」→「仍要运行」即可。

两个提示都只出现一次，不是病毒警告，只是没有开发者签名。

## Finder 右键快速打开（macOS）

不想每次开 App 再拖文件？装一个 Quick Action：

```bash
# 前置：已把 /Applications/视频压缩工具.app 装好（任意版本）
python3 scripts/make-finder-quick-action.py
```

之后在 Finder 选中视频/图片文件 → 右键 → **快速操作**（或「服务」）→「**用视频压缩工具打开**」。
文件直接进队列：App 没开就冷启动带文件入队，已开着就单例转发入队（不会开第二个窗口）。
如果右键菜单里没显示，去「系统设置 → 键盘 → 键盘快捷键 → 服务」里确认「用视频压缩工具打开」已勾选。

## 功能清单（11 项配方）

| # | 功能 | 说明 |
|---|------|------|
| 1 | 无损存档 FFV1 | 逐帧可验证（framemd5），保留 alpha 通道 |
| 2 | 视觉无损瘦身 x265 | CRF 可调，`hvc1` 标签保证苹果生态可播 |
| 3 | 转 GIF | 画质优先 / 体积优先两档，`stats_mode=diff` + bayer 抖动 |
| 4 | 导出 PNG 序列 | 逐帧导出为图片目录 |
| 5 | 图片合成视频 | 图片序列 → 视频（帧率可调） |
| 6 | 压到指定大小 | 自动二分 CRF，命中目标体积 |
| 7 | 视频拼接 | 多段同规格视频无损合并 |
| 8 | 裁剪片段 | 按起止时间截取 |
| 9 | 提取音频 | 抽出音轨 |
| 10 | 一键静音 | 去音轨 |
| 11 | 旋转 / 横竖转换 | 90/180/270 度 |

**队列能力**：批量任务队列，支持拖放多个文件；每行可单独删除（转换中行禁用）；「清除已完成」只清完成行；「开始」自动跳过已完成行并记录「跳过已完成 ×N」；「无损验证」一键对全部行做 framemd5 逐帧比对。

## 构建方法

前置：Node.js ≥ 18、Rust（cargo）。

### macOS arm64

```bash
# 1. 安装前端依赖
npm install

# 2. 下载 ffmpeg / ffprobe sidecar（固定 9.0.2 构建 + SHA-256 校验，不入 git）
bash scripts/fetch-ffmpeg

# 3. 构建（自动先跑前端 vite build，再编译 Rust release，产出 .dmg + .app）
npm run tauri build
# 产物：src-tauri/target/release/bundle/dmg/视频压缩工具_*.dmg
```

### Windows x64

```powershell
# 1. 安装前端依赖
npm install

# 2. 下载 ffmpeg / ffprobe sidecar（BtbN win64-gpl 固定构建 + SHA-256 校验，不入 git）
powershell -File scripts/fetch-ffmpeg-windows.ps1

# 3. 构建（NSIS 安装器；NSIS 由 Tauri 自动处理，无需手装）
npm run tauri build
# 产物：src-tauri/target/release/bundle/nsis/视频压缩工具_1.0.0_x64_*.exe
```

开发模式：`npm run tauri dev`。

自测：`bash selftest/smoke.sh`（28 项断言，需先 `cargo build --features vct-cli --bin vct-cli`）。

云端打包：打 `v*` tag（或手动 dispatch）触发 `.github/workflows/release.yml`，双平台 runner 各自下载固定版 ffmpeg 后构建，产物自动挂 draft Release（本期未签名）。

## ffmpeg 许可证声明（GPL）

本应用以 **独立可执行文件（sidecar）** 方式调用 ffmpeg / ffprobe，**未将 ffmpeg 静态链入主程序**：

- sidecar 为 martin-riedl 静态构建（9.0.2），启用 libx264 / libx265 等 GPL 组件，该二进制本身以 **GPL v2（或更高版本）** 授权；
- Windows 版 sidecar 为 BtbN win64-gpl 静态构建（n9.0.2 系列，固定 release tag + SHA-256 校验，见 `scripts/fetch-ffmpeg-windows.ps1`），授权同为 **GPL v2+**，构建源 https://github.com/BtbN/FFmpeg-Builds；
- 随包提供 GPL 全文：`src-tauri/LICENSE-GPLv2.txt`；
- **源码获取方式**：ffmpeg 上游源码 https://ffmpeg.org（9.0.2）；macOS sidecar 构建源 https://git.martin-riedl.de/ffmpeg/build-script，Windows sidecar 构建源 https://github.com/BtbN/FFmpeg-Builds；libx264 / libx265 源码见 x264/x265 官方仓库；
- 主程序（Tauri / React / Rust 代码）保持 **MIT** 授权，详见 `LICENSE`。

完整说明见 [`src-tauri/NOTICE-ffmpeg-GPL.md`](src-tauri/NOTICE-ffmpeg-GPL.md)。

## 项目结构

```
├── src/                  # React 前端（队列 UI、日志、验证面板）
├── src-tauri/
│   ├── src/              # Rust 后端：引擎配方（engine.rs）、任务队列（jobs.rs）、命令（commands.rs）
│   ├── binaries/         # ffmpeg / ffprobe sidecar（gitignore，由 fetch-ffmpeg 下载）
│   ├── NOTICE-ffmpeg-GPL.md   # ffmpeg GPL 说明
│   └── LICENSE-GPLv2.txt      # GPL 全文
├── .github/workflows/release.yml  # 双平台打包（mac dmg + windows nsis）→ draft Release
├── scripts/
│   ├── fetch-ffmpeg             # macOS sidecar：固定版本下载 + SHA-256 校验 + 编码器验证
│   ├── fetch-ffmpeg-windows.ps1 # Windows sidecar：BtbN 固定构建 + SHA-256 校验 + 编码器验证
│   └── make-finder-quick-action.py  # 生成/安装 Finder Quick Action
└── selftest/smoke.sh     # 冒烟自测（28 断言）
```

## License

本项目代码：[MIT](LICENSE)。ffmpeg sidecar：GPL v2+（见上节声明）。
