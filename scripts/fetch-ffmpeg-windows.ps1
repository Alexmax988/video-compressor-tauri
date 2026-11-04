# fetch-ffmpeg-windows — Windows 版 ffmpeg/ffprobe sidecar 准备脚本（GitHub Actions windows-latest runner 用）
# 派单：BtbN win64-gpl 静态构建，固定 release tag + 固定资产 + SHA-256 固定校验 + libx265 验证。二进制不入仓。
#
# 固定版本（2026-11-04 定）：
#   release tag  autobuild-2026-10-25-13-00（BtbN 每日构建 release，tag 固定后资产 URL 永久不变）
#   资产         ffmpeg-n9.0.2-36-g6882969ce9-win64-gpl-9.0.zip（ffmpeg 9.0.2 系列，win64 GPL 静态）
#   SHA-256      941603e8e99cf92309a3faaf156479438c41e738d007dbef3e484204614cb156（本机实下载核对）
$ErrorActionPreference = "Stop"

$FF_RELEASE = "autobuild-2026-10-25-13-00"
$FF_ASSET   = "ffmpeg-n9.0.2-36-g6882969ce9-win64-gpl-9.0.zip"
$FF_SHA256  = "941603e8e99cf92309a3faaf156479438c41e738d007dbef3e484204614cb156"
$BASE       = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FF_RELEASE"
$TRIPLE     = "x86_64-pc-windows-msvc"

function Die([string]$msg) { Write-Host "❌ $msg" -ForegroundColor Red; exit 1 }

$root = Split-Path $PSScriptRoot -Parent
$bin  = Join-Path $root "src-tauri\binaries"
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("vct-ff-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $bin, $work | Out-Null

# 1) 下载固定资产
Write-Host "→ 下载 $FF_ASSET（BtbN win64-gpl 静态版，ffmpeg 9.0.2 系列）..."
$url = "$BASE/$FF_ASSET"
$zip = Join-Path $work $FF_ASSET
$ProgressPreference = "SilentlyContinue"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing -TimeoutSec 1800

# 2) SHA-256 固定校验
$actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $FF_SHA256) {
    Die "SHA-256 不匹配：期望 $FF_SHA256，实际 $actual（下载损坏或被换包，停止）"
}
Write-Host "→ SHA-256 校验通过"

# 3) 解压 + 按 tauri sidecar 命名规则放位（bin/ffmpeg.exe → ffmpeg-<triple>.exe）
Write-Host "→ 解壓..."
Expand-Archive -Path $zip -DestinationPath (Join-Path $work "ff") -Force
$ff = Get-ChildItem -Path (Join-Path $work "ff") -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
$fp = Get-ChildItem -Path (Join-Path $work "ff") -Recurse -Filter "ffprobe.exe" | Select-Object -First 1
if (-not $ff -or -not $fp) { Die "解压后搵唔到 ffmpeg.exe / ffprobe.exe（BtbN zip 布局变动？）" }
Copy-Item $ff.FullName "$bin\ffmpeg-$TRIPLE.exe"
Copy-Item $fp.FullName "$bin\ffprobe-$TRIPLE.exe"

# 4) 验证编码器（静态 GPL 版必含 libx264/libx265/ffv1，缺一个即系个包唔啱）
Write-Host "→ 验证编码器..."
$enc = & "$bin\ffmpeg-$TRIPLE.exe" -hide_banner -encoders 2>&1
foreach ($n in @("libx265", "libx264", "ffv1")) {
    if ($enc -notmatch [regex]::Escape($n)) {
        Die "ffmpeg 缺少 $n 编码器（唔系 BtbN 静态 GPL 版？停止）"
    }
}

# 5) 版本确认
& "$bin\ffmpeg-$TRIPLE.exe" -version 2>&1 | Select-Object -First 1
& "$bin\ffprobe-$TRIPLE.exe" -version 2>&1 | Select-Object -First 1

Remove-Item -Recurse -Force $work
Write-Host "✅ 完成：$bin\ffmpeg-$TRIPLE.exe 与 ffprobe-$TRIPLE.exe（BtbN win64-gpl 静态版，含 libx264/libx265/ffv1）"
