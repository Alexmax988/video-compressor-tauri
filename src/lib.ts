import { invoke } from "@tauri-apps/api/core";
import type { Channel } from "@tauri-apps/api/core";

// ── 类型（与 Rust 侧一致） ──
export type Feature =
  | "ffv1" | "x265" | "gif" | "pngSeq" | "imgToVideo"
  | "toSize" | "concat" | "trim" | "extract" | "mute" | "rotate";

export const FEATURE_TITLES: Record<Feature, string> = {
  ffv1: "无损存档 FFV1",
  x265: "视觉无损瘦身 x265",
  gif: "转 GIF",
  pngSeq: "导出 PNG 序列",
  imgToVideo: "图片合成视频",
  toSize: "压到指定大小",
  concat: "视频拼接",
  trim: "裁剪片段",
  extract: "提取音频",
  mute: "一键静音",
  rotate: "旋转 / 横竖转换",
};

export interface Params {
  crfX265: string;
  gifPreset: string;
  gifFps: string;
  gifWidth: string;
  pngFps1: boolean;
  imgFps: string;
  imgRes: string;
  imgCw: string;
  imgCh: string;
  imgFit: string;
  imgCrf: string;
  targetMb: string;
  keepAudio: boolean;
  trimStart: string;
  trimDur: string;
  trimPrecise: boolean;
  audioFormat: string;
  rotatePick: string;
}

export function defaultParams(): Params {
  return {
    crfX265: "14",
    gifPreset: "quality",
    gifFps: "12",
    gifWidth: "720",
    pngFps1: false,
    imgFps: "24",
    imgRes: "1920x1080",
    imgCw: "1280",
    imgCh: "720",
    imgFit: "pad",
    imgCrf: "12",
    targetMb: "10",
    keepAudio: true,
    trimStart: "0",
    trimDur: "1",
    trimPrecise: false,
    audioFormat: "mp3",
    rotatePick: "90",
  };
}

export interface ItemOut {
  id: number;
  path: string;
  name: string;
  kind: "video" | "image";
  size: number;
  thumb: string | null;
}

export interface AddFilesResult {
  items: ItemOut[];
  logs: string[];
}

// 队列项（前端状态）
export interface QueueItem extends ItemOut {
  state: "pending" | "running" | "done" | "nogain" | "failed" | "cancelled";
  progress: number;
  outPath?: string;
  outSize?: number;
  frameCount?: number;
  duration?: number;
  error?: string;
  verified?: boolean;
}

export type Msg =
  | { k: "log"; text: string }
  | { k: "jobStarted"; job: number; label: string; total: number; itemIds: number[] }
  | { k: "progress"; job: number; frac: number; overall: number }
  | {
      k: "jobDone";
      job: number;
      label: string;
      ok: boolean;
      detail: string;
      outPath: string | null;
      outSize: number | null;
      frameCount: number | null;
      duration: number | null;
      itemIds: number[];
      noGain: boolean;
    }
  | { k: "jobCancelled"; job: number; label: string; itemIds: number[] }
  | { k: "allDone"; okCount: number; failCount: number; cancelledCount: number };

// ── 命令 ──
export async function addFiles(paths: string[]): Promise<AddFilesResult> {
  return invoke<AddFilesResult>("add_files", { paths });
}

/// 冷启动 argv 文件路径（Finder Quick Action / 命令行启动）；Rust setup 写 pending，取一次即清空。
export async function takeLaunchFiles(): Promise<string[]> {
  return invoke<string[]>("take_launch_files");
}

export async function startJobs(
  feature: Feature,
  params: Params,
  outMode: "same" | "custom",
  outFolder: string | null,
  items: { id: number; path: string; kind: string }[],
  channel: Channel<Msg>,
): Promise<void> {
  await invoke("start_jobs", { feature, params, outMode, outFolder, items, channel });
}

export async function cancelAll(): Promise<void> {
  await invoke("cancel_all");
}

export interface VerifyPair {
  itemId: number;
  source: string;
  output: string;
}
export interface VerifyResult {
  itemId: number;
  identical: boolean;
  note: string;
}
export async function verifyLossless(pairs: VerifyPair[]): Promise<VerifyResult[]> {
  return invoke<VerifyResult[]>("verify_lossless", { pairs });
}

export async function mediaInfo(path: string): Promise<string> {
  return invoke<string>("media_info", { path });
}

export interface ProbeInfo {
  duration: number;
  width: number;
  height: number;
  fps: number;
  hasAudio: boolean;
  videoCodec: string;
}
export async function probeInfo(path: string): Promise<ProbeInfo> {
  return invoke<ProbeInfo>("probe_info", { path });
}

export interface FfmpegInfo {
  sourceDesc: string;
  version: string;
}
export async function ffmpegInfo(): Promise<FfmpegInfo> {
  return invoke<FfmpegInfo>("ffmpeg_info");
}

export async function previewFrame(path: string): Promise<string | null> {
  return invoke<string | null>("preview_frame", { path });
}

// ── 工具 ──
export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return kb >= 100 ? `${Math.round(kb)} KB` : `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return mb >= 100 ? `${Math.round(mb)} MB` : `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

export function stamp(): string {
  const d = new Date();
  const p = (n: number, l = 2) => String(n).padStart(l, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}
