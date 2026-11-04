// commands.rs — 前端 invoke 入口（前端永不直接执行进程；全部 ffmpeg 逻辑喺呢度 + engine + jobs）
use crate::engine;
use crate::jobs::{self, ItemRef, JobManager, Msg, StartJobsReq};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

static ID_COUNTER: OnceLock<AtomicU64> = OnceLock::new();
fn next_id() -> u64 {
    ID_COUNTER.get_or_init(|| AtomicU64::new(0)).fetch_add(1, Ordering::SeqCst) + 1
}

// ── 添加文件 ──
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ItemOut {
    pub id: u64,
    pub path: String,
    pub name: String,
    pub kind: String, // video | image
    pub size: i64,
    /// 缩略图 / 预览图缓存文件（asset protocol 范围 = app cache dir）；视频抽帧、图片缩放拷贝
    pub thumb: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddFilesResult {
    pub items: Vec<ItemOut>,
    pub logs: Vec<String>,
}

#[tauri::command]
pub fn add_files(app: AppHandle, paths: Vec<String>) -> AddFilesResult {
    let mut items: Vec<ItemOut> = Vec::new();
    let mut logs: Vec<String> = Vec::new();
    let cache_dir = app
        .path()
        .cache_dir()
        .ok()
        .map(|p| p.join("vct-frames"))
        .unwrap_or_else(std::env::temp_dir);
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        logs.push(format!("缩略图缓存目录创建失败：{e}"));
    }
    let tools = match jobs::resolve(&app) {
        Ok(t) => t,
        Err(e) => {
            logs.push(e);
            return AddFilesResult { items, logs };
        }
    };

    for p in &paths {
        let path = PathBuf::from(p);
        let meta = std::fs::metadata(&path);
        if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
            // 整个图片文件夹 → 列图片（按文件名排序）
            let mut imgs: Vec<PathBuf> = std::fs::read_dir(&path)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .filter(|x| {
                            x.extension()
                                .map(|e| engine::IMAGE_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
                                .unwrap_or(false)
                        })
                        .collect()
                })
                .unwrap_or_default();
            imgs.sort();
            if imgs.is_empty() {
                logs.push(format!(
                    "⚠️ {}：文件夹里没有支持的图片（png/jpg/jpeg/bmp/webp/tiff）",
                    path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                ));
                continue;
            }
            for img in imgs {
                let id = next_id();
                let thumb = make_thumb(&img, &tools, &cache_dir, false);
                items.push(ItemOut {
                    id,
                    path: img.to_string_lossy().into_owned(),
                    name: img.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                    kind: "image".into(),
                    size: std::fs::metadata(&img).map(|m| m.len() as i64).unwrap_or(0),
                    thumb,
                });
            }
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if engine::VIDEO_EXTS.contains(&ext.as_str()) {
            let id = next_id();
            let thumb = make_thumb(&path, &tools, &cache_dir, true);
            items.push(ItemOut {
                id,
                path: path.to_string_lossy().into_owned(),
                name: path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                kind: "video".into(),
                size: std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0),
                thumb,
            });
        } else if engine::IMAGE_EXTS.contains(&ext.as_str()) {
            let id = next_id();
            let thumb = make_thumb(&path, &tools, &cache_dir, false);
            items.push(ItemOut {
                id,
                path: path.to_string_lossy().into_owned(),
                name: path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                kind: "image".into(),
                size: std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0),
                thumb,
            });
        } else {
            logs.push(format!("⚠️ 已忽略：{}（不是支持的视频/图片）", ext));
        }
    }
    AddFilesResult { items, logs }
}

/// 视频：抽 128 长边帧；图片：720 长边缩放拷贝（asset protocol 只许 cache dir）
fn make_thumb(
    file: &std::path::Path,
    tools: &engine::Tools,
    cache_dir: &std::path::Path,
    is_video: bool,
) -> Option<String> {
    let edge = if is_video { 128 } else { 720 };
    let dst = cache_dir.join(format!(
        "{}-{}.jpg",
        file.file_name()?.to_string_lossy(),
        edge
    ));
    if dst.exists() {
        return Some(dst.to_string_lossy().into_owned());
    }
    for ss in ["1", "0"] {
        let args: Vec<String> = vec![
            "-hide_banner".into(), "-loglevel".into(), "error".into(), "-ss".into(),
            ss.into(), "-i".into(), file.to_string_lossy().into_owned(),
            "-frames:v".into(), "1".into(), "-vf".into(),
            format!("scale={edge}:-2"), "-f".into(), "image2".into(), "-y".into(),
            dst.to_string_lossy().into_owned(),
        ];
        let out = Command::new(&tools.ffmpeg).args(&args).output();
        if let Ok(o) = out {
            if o.status.success() && dst.exists() {
                return Some(dst.to_string_lossy().into_owned());
            }
        }
    }
    None
}

// ── 开始 / 取消 ──
#[tauri::command]
pub fn start_jobs(
    app: AppHandle,
    state: State<'_, JobManager>,
    req: StartJobsReq,
    channel: Channel<Msg>,
) -> Result<(), String> {
    jobs::start_batch(app, req, channel, &state)
}

#[tauri::command]
pub fn cancel_all(state: State<'_, JobManager>) -> Result<(), String> {
    state.cancel_all();
    Ok(())
}

// ── 无损验证（framemd5） ──
#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPair {
    pub item_id: u64,
    pub source: String,
    pub output: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub item_id: u64,
    pub identical: bool,
    pub note: String,
}

#[tauri::command]
pub fn verify_lossless(
    app: AppHandle,
    pairs: Vec<VerifyPair>,
) -> Result<Vec<VerifyResult>, String> {
    let tools = jobs::resolve(&app)?;
    let mut out = Vec::new();
    for p in pairs {
        let r = engine::verify_lossless(
            std::path::Path::new(&p.source),
            std::path::Path::new(&p.output),
            &tools,
        );
        match r {
            Ok((identical, note)) => out.push(VerifyResult {
                item_id: p.item_id,
                identical,
                note,
            }),
            Err(e) => out.push(VerifyResult {
                item_id: p.item_id,
                identical: false,
                note: format!("验证失败：{e}"),
            }),
        }
    }
    Ok(out)
}

// ── 媒体信息 / ffmpeg 来源 ──
#[tauri::command]
pub fn media_info(app: AppHandle, path: String) -> Result<String, String> {
    let tools = jobs::resolve(&app)?;
    engine::info_text(std::path::Path::new(&path), &tools)
}

#[tauri::command]
pub fn probe_info(app: AppHandle, path: String) -> Result<engine::Info, String> {
    let tools = jobs::resolve(&app)?;
    engine::probe(std::path::Path::new(&path), &tools)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegInfo {
    pub source_desc: String,
    pub version: String,
}

#[tauri::command]
pub fn ffmpeg_info(app: AppHandle) -> Result<FfmpegInfo, String> {
    let tools = jobs::resolve(&app)?;
    let out = Command::new(&tools.ffmpeg)
        .arg("-version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string())
        .unwrap_or_default();
    Ok(FfmpegInfo {
        source_desc: tools.source_desc,
        version: out,
    })
}

/// 预览大图（720 长边抽帧，入 cache；GIF 产物前端直接 <img> 原生播放）
#[tauri::command]
pub fn preview_frame(app: AppHandle, path: String) -> Result<Option<String>, String> {
    let tools = jobs::resolve(&app)?;
    let cache = app.path().cache_dir().map_err(|e| e.to_string())?;
    let p = engine::frame_path(std::path::Path::new(&path), true, 720, &tools, &cache);
    Ok(p.map(|x| x.to_string_lossy().into_owned()))
}

// 编译期检查：ItemRef 喺 job 结构里保留「按路径直达队列」嘅接口（本期 Finder 右键快捷入口系下期）
#[allow(dead_code)]
fn _future_finder_entry(items: Vec<ItemRef>) -> Vec<ItemRef> {
    items
}
