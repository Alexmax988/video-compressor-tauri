#!/usr/bin/env bash
# 冒烟：11 项各一次 + FFV1 framemd5 对照旧版 + 取消键实测
# 用法：bash selftest/smoke.sh   （需先 cargo build --bin vct-cli）
set -uo pipefail
cd "$(dirname "$0")/.."
OLD=~/Projects/叻叻/2026-11-03_视频压缩工具/selftest
CLI=./src-tauri/target/release/vct-cli
[ -x "$CLI" ] || CLI=./src-tauri/target/debug/vct-cli
[ -x "$CLI" ] || { echo "❌ 搵唔到 vct-cli，先跑 cargo build --bin vct-cli"; exit 1; }
OUT="$OLD/../selftest-out-tauri"   # 放旧工程外，唔郁旧产物
rm -rf "$OUT"; mkdir -p "$OUT"
pass=0; fail=0

say() { echo; echo "── $1"; }
ok()  { echo "   ✅ $1"; pass=$((pass+1)); }
bad() { echo "   ❌ $1"; fail=$((fail+1)); }

say "0) ffmpeg 来源（应係 sidecar 9.0.2）"
"$CLI" ffv1 --info "$OLD/test-src.mp4" >/dev/null 2>&1

run() { # run <label> <cli...>
  local label="$1"; shift
  say "$label"
  if "$CLI" "$@" --out "$OUT" >"$OUT/last.log" 2>&1; then
    ok "$label"
  else
    bad "$label"; tail -3 "$OUT/last.log"
  fi
}

# 1 FFV1 + framemd5 对照旧版产物
run "1) FFV1" ffv1 "$OLD/test-src.mp4"
if [ -f "$OUT/test-src.ffv1.mkv" ]; then
  if "$CLI" verify "$OLD/test-src.mp4" "$OUT/test-src.ffv1.mkv" 2>&1 | grep -q "逐帧一致"; then
    ok "FFV1 新产物 framemd5 逐帧一致"
  else
    bad "FFV1 framemd5 唔一致"; "$CLI" verify "$OLD/test-src.mp4" "$OUT/test-src.ffv1.mkv"
  fi
  # 对照旧版产物
  if "$CLI" verify "$OUT/test-src.ffv1.mkv" "$OLD/out/test-src.ffv1.mkv" 2>&1 | grep -q "逐帧一致"; then
    ok "FFV1 新旧产物 framemd5 一致"
  else
    bad "FFV1 新旧产物 framemd5 唔一致"
  fi
fi

# 2 x265（±10% 量级对照）
run "2) x265" x265 "$OLD/test-src.mp4"
if [ -f "$OUT/test-src.x265.mp4" ]; then
  a=$(stat -f%z "$OUT/test-src.x265.mp4"); b=$(stat -f%z "$OLD/out/test-src.x265.mp4")
  d=$(( (a > b ? a - b : b - a) * 100 / b ))
  [ "$d" -le 10 ] && ok "x265 大小量级：新 ${a}B vs 旧 ${b}B（差 ${d}%）" || bad "x265 大小差 ${d}%（>10%）"
fi

# 3 GIF（±10%）
run "3) GIF" gif "$OLD/test-src.mp4"
if [ -f "$OUT/test-src.gif" ]; then
  a=$(stat -f%z "$OUT/test-src.gif"); b=$(stat -f%z "$OLD/out/test-src.gif")
  d=$(( (a > b ? a - b : b - a) * 100 / b ))
  [ "$d" -le 10 ] && ok "GIF 大小量级：新 ${a}B vs 旧 ${b}B（差 ${d}%）" || bad "GIF 大小差 ${d}%（>10%）"
fi

# 4 PNG 序列
run "4) PNG 序列" png "$OLD/test-src.mp4"
n=$(ls "$OUT/test-src_frames" 2>/dev/null | grep -c png)
[ "$n" -gt 0 ] && ok "PNG 序列 ${n} 帧" || bad "PNG 序列无输出"

# 5 图片合成
run "5) 图片合成" img "$OLD/out/test-src_frames" --img-fps 24
[ -f "$OUT/图片合成_150张.mp4" ] && ok "图片合成产物存在" || bad "图片合成无产物"

# 6 压到指定大小（±10%）
run "6) 压到 1MB" tosize "$OLD/test-src.mp4" --target 1
if [ -f "$OUT/test-src_1MB.mp4" ]; then
  a=$(stat -f%z "$OUT/test-src_1MB.mp4"); b=$(stat -f%z "$OLD/out/test-src_1MB.mp4")
  d=$(( (a > b ? a - b : b - a) * 100 / b ))
  [ "$d" -le 10 ] && ok "压大小量级：新 ${a}B vs 旧 ${b}B（差 ${d}%）" || bad "压大小差 ${d}%（>10%）"
fi

# 7 拼接
run "7) 拼接" concat "$OLD/test-src.mp4" "$OLD/test-src_5MB.mp4"
[ -f "$OUT/合并_2段.mp4" ] && ok "拼接产物存在" || bad "拼接无产物"

# 8 裁剪（快速 + 精确）
run "8a) 裁剪快速" trim "$OLD/test-src.mp4" --trim 0 1
run "8b) 裁剪精确" trim "$OLD/test-src.mp4" --trim 0 1 --precise
[ -f "$OUT/test-src_0s-1s.mp4" ] && ok "裁剪产物存在" || bad "裁剪无产物"

# 9 提取音频
run "9a) 提取 MP3" extract "$OLD/test-src.mp4"
run "9b) 提取 WAV" extract "$OLD/test-src.mp4" --audio wav
[ -f "$OUT/test-src.mp3" ] && [ -f "$OUT/test-src.wav" ] && ok "提取音频产物存在" || bad "提取音频无产物"

# 10 静音
run "10) 静音" mute "$OLD/test-src.mp4"
[ -f "$OUT/test-src-静音.mp4" ] && ok "静音产物存在" || bad "静音无产物"

# 11 旋转
run "11) 旋转 90" rotate "$OLD/test-src.mp4" --rotate 90
[ -f "$OUT/test-src-旋转90.mp4" ] && ok "旋转产物存在" || bad "旋转无产物"

# ── 取消键实测：x265 转到一半取消（30s 源 x265 实测 ~4.5s 完成，2s 必係轉緊） ──
say "取消键实测（x265 转 30s 源、2s 时 q 取消＝编码中途）"
CANCEL_SRC=./selftest/test-src_30s.mp4
[ -f "$CANCEL_SRC" ] || { bad "取消测试源缺失：$CANCEL_SRC"; CANCEL_SRC="$OLD/test-src_5MB.mp4"; }
OUT_CANCEL="$OUT/test-src_30s.x265.mp4"
rm -f "$OUT_CANCEL"
"$CLI" x265 "$CANCEL_SRC" --out "$OUT" --cancel-after 2 >"$OUT/cancel.log" 2>&1
rc=$?
if [ "$rc" -eq 3 ]; then
  ok "取消退出码 3（已取消）"
else
  bad "取消退出码 ${rc}（应 3）"; tail -5 "$OUT/cancel.log"
fi
if [ -f "$OUT_CANCEL" ]; then
  bad "半成品 ${OUT_CANCEL} 仍留低"
else
  ok "无半成品残留（${OUT_CANCEL##*/} 唔存在）"
fi
sleep 1
orphans=$(pgrep -f "$(basename "$OUT_CANCEL")" | wc -l | tr -d ' ')
[ "$orphans" -eq 0 ] && ok "无 ffmpeg 孤儿（取消後进程全清）" || bad "ffmpeg 孤儿 ${orphans} 个"
echo "   （取消日志尾部）"; tail -3 "$OUT/cancel.log"

echo
echo "════ 冒烟结果：${pass} 项通过，${fail} 项失败 ════"
[ "$fail" -eq 0 ]
