import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { Channel } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./styles.css";
import {
  addFiles,
  cancelAll,
  ffmpegInfo,
  type Feature,
  FEATURE_TITLES,
  type Msg,
  type Params,
  defaultParams,
  type QueueItem,
  humanSize,
  probeInfo,
  previewFrame,
  startJobs,
  stamp,
  takeLaunchFiles,
  verifyLossless,
  mediaInfo,
  type ProbeInfo,
} from "./lib";

// ── 功能图标（轻量 unicode；左功能栏 232px） ──
const FEAT_ICONS: Record<Feature, string> = {
  ffv1: "📦",
  x265: "🗜",
  gif: "🖼",
  pngSeq: "🗂",
  imgToVideo: "📸",
  toSize: "⬇",
  concat: "🔗",
  trim: "✂",
  extract: "🎵",
  mute: "🔇",
  rotate: "⟳",
};
const CONVERT_FEATS: Feature[] = ["ffv1", "x265", "gif", "pngSeq", "imgToVideo"];
const TOOL_FEATS: Feature[] = ["toSize", "concat", "trim", "extract", "mute", "rotate"];

interface LogLine {
  t: string;
  text: string;
  kind: "log" | "ok" | "err" | "warn";
}

export default function App() {
  const [items, setItems] = useState<QueueItem[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [feature, setFeature] = useState<Feature>("ffv1");
  const [params, setParams] = useState<Params>(defaultParams());
  const [outMode, setOutMode] = useState<"same" | "custom">("same");
  const [outFolder, setOutFolder] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [overall, setOverall] = useState(0);
  const [stage, setStage] = useState("");
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [previewInfo, setPreviewInfo] = useState("");
  const [ffmpegSource, setFfmpegSource] = useState("");
  const [dragHot, setDragHot] = useState(false);
  const [sizeDur, setSizeDur] = useState(0);
  const jobToIds = useRef(new Map<number, number[]>());
  const logBoxRef = useRef<HTMLDivElement>(null);
  const itemsRef = useRef(items);
  itemsRef.current = items;

  const log = useCallback((text: string, kind: LogLine["kind"] = "log") => {
    setLogs((ls) => [...ls.slice(-400), { t: stamp(), text, kind }]);
  }, []);

  // ── 启动：ffmpeg 来源提示（对应旧版两条提示行） ──
  useEffect(() => {
    ffmpegInfo()
      .then((f) => {
        setFfmpegSource(f.sourceDesc);
        log(`提示：ffmpeg 使用 ${f.sourceDesc}（${f.version || "版本未知"}）`);
        log("提示：产物默认放源文件同目录；重名自动加序号；进度来自 ffmpeg -progress 真实解析。");
      })
      .catch((e) => log(`ffmpeg 定位失败：${e}`, "err"));
  }, [log]);

  // ── 日志自动滚底 ──
  useEffect(() => {
    logBoxRef.current?.scrollTo({ top: 999999 });
  }, [logs]);

  // ── 拖放（Tauri 原生 onDragDropEvent 攞绝对路径） ──
  useEffect(() => {
    let un: (() => void) | undefined;
    getCurrentWindow()
      .onDragDropEvent(async (e) => {
        if (e.payload.type === "drop") {
          setDragHot(false);
          await doAddFiles(e.payload.paths);
        } else if (e.payload.type === "enter" || e.payload.type === "over") {
          setDragHot(true);
        } else {
          setDragHot(false);
        }
      })
      .then((fn) => (un = fn))
      .catch(() => {});
    return () => un?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── 添加文件（去重 + 默认选中第一行） ──
  const doAddFiles = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      try {
        const res = await addFiles(paths);
        res.logs.forEach((l) => log(l, l.startsWith("⚠️") ? "warn" : "log"));
        const seen = new Set(itemsRef.current.map((i) => i.path));
        const fresh: QueueItem[] = res.items
          .filter((it) => !seen.has(it.path))
          .map((it) => ({
            ...it,
            state: "pending" as const,
            progress: 0,
          }));
        if (fresh.length > 0) {
          setItems((prev) => {
            const dup = new Set(prev.map((i) => i.path));
            return [...prev, ...fresh.filter((f) => !dup.has(f.path))];
          });
          log(`已添加 ${fresh.length} 个文件`);
          if (selectedId === null) {
            setSelectedId(res.items[0]?.id ?? null);
          }
        }
      } catch (e) {
        log(`添加失败：${e}`, "err");
      }
    },
    [log, selectedId],
  );

  const doAddFilesRef = useRef(doAddFiles);
  doAddFilesRef.current = doAddFiles;

  // ── 启动参数文件入队（Finder Quick Action 入口，第二期） ──
  // 冷启动：Rust setup 把 argv 文件存 PENDING_LAUNCH_FILES → mount 时 takeLaunchFiles 取走；
  // App 已运行：单例插件把二进程 argv 转发为 vct://pending-files 事件 → 直接入队。
  // doAddFiles 已按 path 去重，重复触发唔会出双行。
  useEffect(() => {
    let un: (() => void) | undefined;
    takeLaunchFiles()
      .then((files) => {
        if (files.length) doAddFilesRef.current(files);
      })
      .catch(() => {});
    listen<string[]>("vct://pending-files", (ev) => {
      const files = ev.payload;
      if (files?.length) doAddFilesRef.current(files);
    })
      .then((fn) => (un = fn))
      .catch(() => {});
    return () => un?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── 压到指定大小：码率预估（标注估算） ──
  useEffect(() => {
    if (feature !== "toSize") {
      setSizeDur(0);
      return;
    }
    const firstVideo = items.find((i) => i.kind === "video");
    if (!firstVideo) {
      setSizeDur(0);
      return;
    }
    probeInfo(firstVideo.path)
      .then((p: ProbeInfo) => setSizeDur(p.duration))
      .catch(() => setSizeDur(0));
  }, [feature, items]);

  // ── 检查器：预览 + 媒体信息（随选中行） ──
  useEffect(() => {
    const it = items.find((i) => i.id === selectedId);
    if (!it) {
      setPreviewUrl(null);
      setPreviewInfo("选中队列里的一个文件查看预览；选中的已完成 GIF 产物会动态播放。");
      return;
    }
    let cancelled = false;
    mediaInfo(it.path)
      .then((t) => !cancelled && setPreviewInfo(t))
      .catch((e) => !cancelled && setPreviewInfo(`（读取媒体信息失败）\n${e}`));
    const isGifOut =
      it.state === "done" && it.outPath?.toLowerCase().endsWith(".gif");
    if (it.kind === "image") {
      setPreviewUrl(it.thumb ? convertFileSrc(it.thumb) : null);
    } else if (isGifOut) {
      setPreviewUrl(it.outPath ? convertFileSrc(it.outPath) : null); // <img> 原生播放 GIF
    } else {
      setPreviewUrl(it.thumb ? convertFileSrc(it.thumb) : null);
      previewFrame(it.path)
        .then((p) => !cancelled && p && setPreviewUrl(convertFileSrc(p)))
        .catch(() => {});
    }
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId, items]);

  // ── 开始（Channel 收进度 / 状态） ──
  const start = useCallback(async () => {
    if (busy) return;
    const all = itemsRef.current;
    if (all.length === 0) {
      log("⚠️ 先添加文件再开始", "warn");
      return;
    }
    // 补充规格：自动跳过已完成行（只处理待转/失败行，日志写明跳过数）
    // 范围跟 Rust 侧一致：图片合成功能只处理图片行，其余功能只处理视频行
    const kindOk = (i: { kind: string }) => (feature === "imgToVideo" ? i.kind === "image" : i.kind === "video");
    const active = all.filter((i) => kindOk(i) && (i.state === "pending" || i.state === "failed"));
    const skipped = all.filter(kindOk).length - active.length;
    if (active.length === 0) {
      log("⚠️ 没有可处理的行：全部行已完成/已取消（删除行后重新添加再开始）", "warn");
      return;
    }
    if (skipped > 0) {
      log(`已跳过 ${skipped} 个已完成/已取消行，只处理 ${active.length} 个待转/失败行`, "warn");
    }
    if (outMode === "custom" && !outFolder) {
      log("⚠️ 自选文件夹还没选", "warn");
      return;
    }
    // 参数校验（对应旧 readParams 的校验）
    const fps = Number(params.imgFps);
    if (params.imgFps !== "" && (!Number.isFinite(fps) || fps < 1 || fps > 60)) {
      log("⚠️ 参数错误：帧率必须是 1–60 的数字", "warn");
      return;
    }
    if (params.imgRes === "custom") {
      const w = Number(params.imgCw), h = Number(params.imgCh);
      if (!Number.isInteger(w) || !Number.isInteger(h) || w <= 0 || h <= 0) {
        log("⚠️ 参数错误：自定义宽高必须是正整数", "warn");
        return;
      }
    }
    if (isNaN(Number(params.trimStart)) || isNaN(Number(params.trimDur))) {
      log("⚠️ 参数错误：裁剪起点/时长必须是数字（秒）", "warn");
      return;
    }

    setBusy(true);
    setOverall(0);
    setStage(`准备开始（${active.length} 个待处理行）`);
    const channel = new Channel<Msg>();
    channel.onmessage = (msg) => {
      switch (msg.k) {
        case "log":
          log(msg.text, msg.text.startsWith("⚠️") ? "warn" : msg.text.startsWith("❌") ? "err" : "log");
          break;
        case "jobStarted": {
          jobToIds.current.set(msg.job, msg.itemIds);
          setItems((prev) =>
            prev.map((it) =>
              msg.itemIds.includes(it.id)
                ? { ...it, state: "running" as const, progress: 0, error: undefined }
                : it,
            ),
          );
          setStage(`正在转换 ${msg.job + 1}/${msg.total}：${msg.label}`);
          break;
        }
        case "progress": {
          const ids = jobToIds.current.get(msg.job) ?? [];
          setItems((prev) =>
            prev.map((it) => (ids.includes(it.id) ? { ...it, progress: msg.frac } : it)),
          );
          setOverall(msg.overall);
          break;
        }
        case "jobDone": {
          setItems((prev) =>
            prev.map((it) => {
              if (!msg.itemIds.includes(it.id)) return it;
              if (!msg.ok) {
                return { ...it, state: "failed" as const, error: msg.detail };
              }
              return {
                ...it,
                state: msg.noGain ? "nogain" : "done",
                progress: 1,
                outPath: msg.outPath ?? undefined,
                outSize: msg.outSize ?? undefined,
                frameCount: msg.frameCount ?? undefined,
                duration: msg.duration ?? undefined,
              };
            }),
          );
          if (msg.ok) {
            log(
              `✅ ${msg.label}：完成${msg.frameCount != null ? `，共 ${msg.frameCount} 帧` : ""}${msg.duration != null ? `，时长 ${msg.duration.toFixed(2)}s` : ""}${msg.outSize != null ? `，产物 ${humanSize(msg.outSize)}` : ""}`,
              "ok",
            );
            if (msg.noGain) log(`⚠️ ${msg.label}：产物 ≥ 原文件，无体积增益`, "warn");
          }
          setOverall(1);
          break;
        }
        case "jobCancelled": {
          setItems((prev) =>
            prev.map((it) =>
              msg.itemIds.includes(it.id) ? { ...it, state: "cancelled" as const, progress: 0 } : it,
            ),
          );
          break;
        }
        case "allDone": {
          setBusy(false);
          setStage(`${msg.okCount + msg.failCount + msg.cancelledCount} 完成`);
          break;
        }
      }
    };
    try {
      await startJobs(
        feature,
        params,
        outMode,
        outMode === "custom" ? outFolder : null,
        active.map((i) => ({ id: i.id, path: i.path, kind: i.kind })),
        channel,
      );
    } catch (e) {
      setBusy(false);
      log(`启动失败：${e}`, "err");
    }
  }, [busy, feature, params, outMode, outFolder, log]);

  // ── 取消（Rust 侧 q 优雅收尾协议） ──
  const cancel = useCallback(async () => {
    try {
      await cancelAll();
      log("已请求取消：ffmpeg 收到 q 优雅收尾（3s 未死先 kill），剩余任务跳过。", "warn");
    } catch (e) {
      log(`取消失败：${e}`, "err");
    }
  }, [log]);

  // ── 补充规格：删除单行 / 清除已完成行 ──
  const removeItem = useCallback((id: number) => {
    const cur = itemsRef.current;
    const it = cur.find((i) => i.id === id);
    const idx = cur.findIndex((i) => i.id === id);
    if (!it) return;
    const next = cur.filter((i) => i.id !== id);
    setItems(next);
    if (it.id === selectedId) {
      const fb = next.length > 0 ? next[Math.min(idx, next.length - 1)] : null;
      setSelectedId(fb ? fb.id : null);
    }
    log(`删除行：${it.name}`);
  }, [selectedId, log]);

  const clearCompleted = useCallback(() => {
    const cur = itemsRef.current;
    const keep = cur.filter((i) => i.state !== "done" && i.state !== "nogain");
    const n = cur.length - keep.length;
    if (n === 0) return;
    setItems(keep);
    if (selectedId != null && !keep.some((i) => i.id === selectedId)) {
      setSelectedId(keep.length > 0 ? keep[0].id : null);
    }
    log(`清除 ${n} 个已完成行`);
  }, [selectedId, log]);

  // ── 验证无损（framemd5，一键验证全部行） ──
  const verify = useCallback(async () => {
    // 全部有 FFV1 产物的行都验（不只看第一行；不要求 done，产物在就可验）
    const cands = itemsRef.current.filter((i) => i.outPath?.endsWith(".ffv1.mkv"));
    if (cands.length === 0) {
      log("⚠️ 没有可验证的 FFV1 产物（先把 FFV1 路线跑完成）", "warn");
      return;
    }
    log(`开始 framemd5 验证 ${cands.length} 行…`);
    const res = await verifyLossless(
      cands.map((i) => ({ itemId: i.id, source: i.path, output: i.outPath! })),
    );
    let okN = 0;
    for (const r of res) {
      if (r.identical) okN++;
      log(
        `${r.identical ? "✅" : "❌"} framemd5 验证 ${itemsRef.current.find((i) => i.id === r.itemId)?.name ?? r.itemId}：${r.note}`,
        r.identical ? "ok" : "err",
      );
      setItems((prev) =>
        prev.map((it) =>
          it.id === r.itemId ? { ...it, verified: r.identical, error: r.identical ? undefined : r.note } : it,
        ),
      );
    }
    log(
      `验证完成：${cands.length} 行中 ${okN} 行逐帧一致${okN === cands.length ? "，全部无损" : "，有行不一致（见上方 ❌）"}`,
      okN === cands.length ? "ok" : "warn",
    );
  }, [log]);

  // ── 添加 / 选目录 ──
  const addClicked = useCallback(async () => {
    const r = await open({
      multiple: true,
      canChooseFiles: true,
      canChooseDirectories: true,
      message: "选择视频、图片，或整个图片文件夹",
    });
    if (Array.isArray(r)) doAddFiles(r);
    else if (typeof r === "string") doAddFiles([r]);
  }, [doAddFiles]);

  const browseFolder = useCallback(async () => {
    const r = await open({ directory: true, canChooseFiles: false });
    if (typeof r === "string") setOutFolder(r);
  }, []);

  // ── GIF 滑杆（0=体积优先 → 100=画质优先；线性插值 fps 10–12、宽 640–720，对应旧版 updateGifReadout） ──
  const [gifSliderV, setGifSliderV] = useState(100);
  const applyGifV = (gv: number) => {
    const fps = Math.round(10 + (2 * gv) / 100);
    const width = Math.round((640 + (80 * gv) / 100) / 2) * 2;
    setGifSliderV(gv);
    setParams((p) => ({ ...p, gifPreset: gv >= 50 ? "quality" : "size", gifFps: String(fps), gifWidth: String(width) }));
  };
  const gifFpsReadout = Math.round(10 + (2 * gifSliderV) / 100);
  const gifWidthReadout = Math.round((640 + (80 * gifSliderV) / 100) / 2) * 2;

  // ── toSize 预估（标注估算） ──
  const sizeEst = useMemo(() => {
    const mb = Number(params.targetMb);
    if (!sizeDur) return "（无视频源，码率预估待有文件后显示）";
    const audioK = params.keepAudio ? 128 : 0;
    const v = Math.max(1, (mb * 8192) / sizeDur - audioK);
    return `视频码率预估 ≈ ${Math.floor(v)} kbps（目标×8192÷时长 ${sizeDur.toFixed(1)}s ${audioK ? "− 128k 音频" : "无音轨"}）· 预估产物 ≈ ${mb} MB（估算）`;
  }, [params, sizeDur]);

  const selItem = items.find((i) => i.id === selectedId);

  return (
    <div className="app">
      {/* ── 左：功能栏 232px ── */}
      <aside className="left">
        <div className="group-label">转换</div>
        {CONVERT_FEATS.map((f) => (
          <div key={f} className={`feat-row ${feature === f ? "on" : ""}`} onClick={() => setFeature(f)}>
            <span className="ico">{FEAT_ICONS[f]}</span>
            {FEATURE_TITLES[f]}
          </div>
        ))}
        <div className="group-label">便携工具</div>
        {TOOL_FEATS.map((f) => (
          <div key={f} className={`feat-row ${feature === f ? "on" : ""}`} onClick={() => setFeature(f)}>
            <span className="ico">{FEAT_ICONS[f]}</span>
            {FEATURE_TITLES[f]}
          </div>
        ))}

        {/* 参数面板（随功能切换） */}
        <div className="params">
          <p className="p-title">{FEATURE_TITLES[feature]}</p>
          {feature === "ffv1" && (
            <p className="p-desc">输出：原名.ffv1.mkv（FFV1 无损母带 + FLAC 音频，逐帧可验证）</p>
          )}
          {feature === "x265" && (
            <>
              <div className="param-row">
                <label>CRF 10</label>
                <input
                  type="range"
                  min={10}
                  max={18}
                  value={Number(params.crfX265)}
                  onChange={(e) => setParams((p) => ({ ...p, crfX265: e.target.value }))}
                />
                <label>18</label>
                <span className="readout">{params.crfX265}</span>
              </div>
              <p className="p-desc">视觉无损瘦身：x265（hvc1 标签，音频原样拷贝）→ 原名.x265.mp4</p>
            </>
          )}
          {feature === "gif" && (
            <>
              <div className="param-row">
                <label>体积</label>
                <input type="range" min={0} max={100} value={gifSliderV} onChange={(e) => applyGifV(Number(e.target.value))} />
                <label>画质</label>
                <span className="readout">
                  fps {gifFpsReadout} · 宽 {gifWidthReadout}
                </span>
              </div>
              <p className="p-desc">
                调色板两步 stats_mode=diff + bayer 抖动（配方不变）→ 原名.gif；GIF 256 色上限，色彩丰富画面可能有轻微色带
              </p>
            </>
          )}
          {feature === "pngSeq" && (
            <>
              <div className="param-row">
                <button className={`chip ${!params.pngFps1 ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, pngFps1: false }))}>
                  全部帧（逐帧无损）
                </button>
                <button className={`chip ${params.pngFps1 ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, pngFps1: true }))}>
                  每秒 1 帧
                </button>
              </div>
              <p className="p-desc">输出：原名_frames/（帧数跑完显示）</p>
            </>
          )}
          {feature === "imgToVideo" && (
            <>
              <div className="param-row">
                <label>帧率</label>
                <input
                  type="text"
                  style={{ width: 48 }}
                  value={params.imgFps}
                  onChange={(e) => setParams((p) => ({ ...p, imgFps: e.target.value }))}
                />
                <label>分辨率</label>
                <select value={params.imgRes} onChange={(e) => setParams((p) => ({ ...p, imgRes: e.target.value }))}>
                  <option value="1920x1080">1920×1080</option>
                  <option value="1080x1920">1080×1920</option>
                  <option value="1080x1080">1080×1080</option>
                  <option value="orig">原图尺寸</option>
                  <option value="custom">自定义</option>
                </select>
                {params.imgRes === "custom" && (
                  <>
                    <input type="text" style={{ width: 52 }} value={params.imgCw} onChange={(e) => setParams((p) => ({ ...p, imgCw: e.target.value }))} />
                    <span>×</span>
                    <input type="text" style={{ width: 52 }} value={params.imgCh} onChange={(e) => setParams((p) => ({ ...p, imgCh: e.target.value }))} />
                  </>
                )}
              </div>
              <div className="param-row">
                <button className={`chip ${params.imgFit === "pad" ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, imgFit: "pad" }))}>
                  完整显示+补边
                </button>
                <button className={`chip ${params.imgFit === "crop" ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, imgFit: "crop" }))}>
                  裁切填满
                </button>
                <label>CRF</label>
                <input type="number" min={0} max={28} style={{ width: 52 }} value={params.imgCrf} onChange={(e) => setParams((p) => ({ ...p, imgCrf: e.target.value }))} />
              </div>
              <p className="p-desc">图片按文件名排序合成 MP4（H.264）</p>
            </>
          )}
          {feature === "toSize" && (
            <>
              <div className="param-row">
                <label>1 MB</label>
                <input
                  type="range"
                  min={1}
                  max={200}
                  value={Number(params.targetMb)}
                  onChange={(e) => setParams((p) => ({ ...p, targetMb: e.target.value }))}
                />
                <label>200 MB</label>
                <span className="readout">{params.targetMb} MB</span>
              </div>
              <div className="param-row">
                <label>快选</label>
                {["10", "25", "50", "100"].map((v) => (
                  <button key={v} className={`chip ${params.targetMb === v ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, targetMb: v }))}>
                    {v}
                  </button>
                ))}
              </div>
              <div className="param-row">
                <button className={`chip ${params.keepAudio ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, keepAudio: !p.keepAudio }))}>
                  保留音频（128k）
                </button>
              </div>
              <p className="est">{sizeEst}</p>
            </>
          )}
          {feature === "concat" && (
            <p className="p-desc">按列表顺序拼接全部视频段（同编码/分辨率/帧率，-c copy 不重编码）；规格不一致会明确报错。输出：合并_段数.mp4</p>
          )}
          {feature === "trim" && (
            <>
              <div className="param-row">
                <label>起点（秒）</label>
                <input type="text" style={{ width: 56 }} value={params.trimStart} onChange={(e) => setParams((p) => ({ ...p, trimStart: e.target.value }))} />
                <label>时长（秒）</label>
                <input type="text" style={{ width: 56 }} value={params.trimDur} onChange={(e) => setParams((p) => ({ ...p, trimDur: e.target.value }))} />
              </div>
              <div className="param-row">
                <button className={`chip ${!params.trimPrecise ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, trimPrecise: false }))}>
                  快速（-c copy，关键帧对齐）
                </button>
                <button className={`chip ${params.trimPrecise ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, trimPrecise: true }))}>
                  精确（重编码 CRF 12）
                </button>
              </div>
            </>
          )}
          {feature === "extract" && (
            <div className="param-row">
              <label>从视频提取音轨</label>
              <button className={`chip ${params.audioFormat === "mp3" ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, audioFormat: "mp3" }))}>
                MP3（q:a 2）
              </button>
              <button className={`chip ${params.audioFormat === "wav" ? "on" : ""}`} onClick={() => setParams((p) => ({ ...p, audioFormat: "wav" }))}>
                WAV 无损
              </button>
            </div>
          )}
          {feature === "mute" && (
            <p className="p-desc">一键静音：视频流原样拷贝、去音轨 → 原名-静音.mp4</p>
          )}
          {feature === "rotate" && (
            <div className="param-row">
              <select value={params.rotatePick} onChange={(e) => setParams((p) => ({ ...p, rotatePick: e.target.value }))}>
                <option value="90">顺时针 90°</option>
                <option value="180">180°</option>
                <option value="270">逆时针 270°</option>
                <option value="landscape">横屏 1920×1080（补边）</option>
                <option value="portrait">竖屏 1080×1920（补边）</option>
              </select>
            </div>
          )}
        </div>

        {/* 输出位置 */}
        <div className="outbox">
          <div className="row">
            <input id="out-same" type="radio" checked={outMode === "same"} onChange={() => setOutMode("same")} />
            <label htmlFor="out-same">源文件同目录</label>
          </div>
          <div className="row">
            <input id="out-custom" type="radio" checked={outMode === "custom"} onChange={() => setOutMode("custom")} />
            <label htmlFor="out-custom">自选文件夹</label>
            <button className="btn" style={{ marginLeft: "auto", padding: "2px 8px" }} onClick={browseFolder}>
              选择…
            </button>
          </div>
          {outMode === "custom" && <path>{outFolder ?? "（未选择）"}</path>}
        </div>

        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn primary" style={{ flex: 1, minWidth: 90 }} disabled={busy} onClick={start}>
            开 始
          </button>
          {busy && (
            <button className="btn danger" style={{ flex: 1, minWidth: 90 }} onClick={cancel}>
              取 消
            </button>
          )}
          <button className="btn" onClick={verify}>
            验证无损
          </button>
        </div>
      </aside>

      {/* ── 中：队列 ── */}
      <main className="center">
        <div className="center-head">
          <span className="title">文件队列</span>
          <span className="stage" style={{ color: "var(--text-2)" }}>
            {ffmpegSource}
          </span>
          <button
            className="btn ghost"
            disabled={busy || !items.some((i) => i.state === "done" || i.state === "nogain")}
            onClick={clearCompleted}
            title="移除所有已完成（含无增益）行"
          >
            清除已完成
          </button>
        </div>

        {busy && (
          <div className="progress-card">
            <span className="big">{Math.round(overall * 100)}%</span>
            <div className="bar-wrap">
              <div className="bar">
                <i style={{ width: `${overall * 100}%` }} />
              </div>
              <span className="stage">{stage}</span>
            </div>
          </div>
        )}

        <div className={`drop-hint ${dragHot ? "hot" : ""}`}>
          拖入视频 / 图片文件（可多个，也可选整个图片文件夹）
          <span className="spacer" />
          <button className="btn" onClick={addClicked}>
            添加…
          </button>
        </div>

        <div className="queue">
          {items.length === 0 && <div className="empty">暂无文件 — 拖入视频 / 图片开始</div>}
          {items.map((it) => (
            <Row
              key={it.id}
              it={it}
              sel={it.id === selectedId}
              busy={busy}
              onSel={() => setSelectedId(it.id)}
              onDel={() => removeItem(it.id)}
            />
          ))}
        </div>
      </main>

      {/* ── 右：检查器 320px ── */}
      <aside className="right-panel">
        <h3>预览</h3>
        <div className="preview-box">
          {previewUrl ? (
            <img src={previewUrl} alt="预览" />
          ) : (
            <div className="ph">选中队列里的一个文件查看预览；选中的已完成 GIF 产物会动态播放。</div>
          )}
        </div>
        <h3>媒体信息 {selItem ? `— ${selItem.name}` : ""}</h3>
        <div className="info-box">{previewInfo || "（无选中文件）"}</div>
        <h3>日志 / 报错</h3>
        <div className="log-box" ref={logBoxRef}>
          {logs.map((l, i) => (
            <div key={i} className={`l-${l.kind}`}>
              [{l.t}] {l.text}
            </div>
          ))}
        </div>
      </aside>
    </div>
  );
}

// ── 队列行（缩略图 + 文件名 + 状态胶囊 + 大小 chip + 行底 bar） ──
function Row({ it, sel, onSel, onDel, busy }: { it: QueueItem; sel: boolean; onSel: () => void; onDel: () => void; busy: boolean }) {
  const outSize = it.outSize ?? 0;
  const ratio = it.state === "done" || it.state === "nogain" ? Math.min(1, outSize / Math.max(1, it.size)) : 0;
  const ratioText =
    ratio < 1 ? `${Math.round(ratio * 100)}%` : it.size > 0 ? `${(outSize / it.size).toFixed(1)}×` : "";
  const sub =
    it.state === "pending"
      ? `原 ${humanSize(it.size)}`
      : it.state === "running"
        ? `原 ${humanSize(it.size)} · 转换中`
        : it.state === "done" || it.state === "nogain"
          ? `原 ${humanSize(it.size)} → 产 ${humanSize(outSize)}`
          : it.state === "failed"
            ? `失败：${(it.error ?? "").split("\n").pop() ?? "失败"}`
            : `原 ${humanSize(it.size)}`;
  const cap =
    it.state === "pending"
      ? "待转"
      : it.state === "running"
        ? `转换中 ${Math.round(it.progress * 100)}%`
        : it.state === "done"
          ? it.frameCount != null
            ? `完成 ${it.frameCount} 帧`
            : "完成"
          : it.state === "nogain"
            ? "无增益"
            : it.state === "failed"
              ? "失败"
              : "已取消";
  return (
    <div className={`row ${sel ? "sel" : ""}`} onClick={onSel}>
      {it.thumb ? (
        <img className="thumb" src={convertFileSrc(it.thumb)} alt="" />
      ) : (
        <div className="thumb ph">{it.kind === "video" ? "▮▮" : "▣"}</div>
      )}
      <div className="info">
        <div className="name">{it.name}</div>
        <div className={`sub ${it.state === "failed" ? "err" : ""}`}>{sub}</div>
      </div>
      <div className="right">
        <div className={`capsule ${it.state}`}>{cap}</div>
        {(it.state === "done" || it.state === "nogain") && (
          <>
            <div className="size-chip">
              <span>{humanSize(it.size)}</span>
              <span className="arrow">→</span>
              <span className="out">{humanSize(outSize)}</span>
              {ratioText && <span className="pct">{ratioText}</span>}
            </div>
            <div className="rowbars" style={{ width: 120 }}>
              <i className="orig" style={{ width: "100%" }} />
              <i className="out" style={{ width: `${ratio * 100}%` }} />
            </div>
          </>
        )}
        {it.state === "running" && (
          <div className="bar" style={{ width: 120 }}>
            <i style={{ width: `${it.progress * 100}%` }} />
          </div>
        )}
      </div>
      <button
        className="del"
        title="删除此行"
        aria-label="删除此行"
        disabled={busy}
        onClick={(e) => {
          e.stopPropagation();
          onDel();
        }}
      >
        ×
      </button>
    </div>
  );
}
