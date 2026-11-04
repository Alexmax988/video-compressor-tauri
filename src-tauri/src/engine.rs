// engine.rs — 便携视频处理箱 Tauri 版引擎
// 配方真相 = 旧 Swift 工程 src/Engine.swift（2026-11-03 版，ffmpeg 9.0.1 实测）。
// 移植原则：build_args 嘅 11 项 ffmpeg 配方参数逐字对应，一字不改；
// 仅按新派单追加 -progress pipe:1 -nostats（进度解析），并去掉 -nostdin（取消协议写 q 需要 stdin）。
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "webm", "m4v", "ts", "avi"];
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp", "tiff"];

// ── 功能（对应 Swift Feature enum，顺序一致） ──
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    Ffv1,
    #[serde(rename = "x265")]
    X265,
    Gif,
    PngSeq,
    ImgToVideo,
    ToSize,
    Concat,
    Trim,
    Extract,
    Mute,
    Rotate,
}

impl Feature {
    pub fn title(self) -> &'static str {
        match self {
            Feature::Ffv1 => "无损存档 FFV1",
            Feature::X265 => "视觉无损瘦身 x265",
            Feature::Gif => "转 GIF",
            Feature::PngSeq => "导出 PNG 序列",
            Feature::ImgToVideo => "图片合成视频",
            Feature::ToSize => "压到指定大小",
            Feature::Concat => "视频拼接",
            Feature::Trim => "裁剪片段",
            Feature::Extract => "提取音频",
            Feature::Mute => "一键静音",
            Feature::Rotate => "旋转 / 横竖转换",
        }
    }
}

// ── 参数（对应 Swift Params，字符串值保持与配方一致） ──
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Params {
    pub crf_x265: String,
    pub gif_preset: String,
    pub gif_fps: String,
    pub gif_width: String,
    #[serde(default = "def_false")]
    pub png_fps1: bool,
    pub img_fps: String,
    pub img_res: String,
    pub img_cw: String,
    pub img_ch: String,
    pub img_fit: String,
    pub img_crf: String,
    pub target_mb: String,
    #[serde(default = "def_true")]
    pub keep_audio: bool,
    pub trim_start: String,
    pub trim_dur: String,
    #[serde(default = "def_false")]
    pub trim_precise: bool,
    pub audio_format: String,
    pub rotate_pick: String,
}

/// Swift 版嘅默认值（GUI 前端总会传全字段；CLI 冒烟靠呢个 Default）
impl Default for Params {
    fn default() -> Self {
        Self {
            crf_x265: "14".into(),
            gif_preset: "quality".into(),
            gif_fps: "12".into(),
            gif_width: "720".into(),
            png_fps1: false,
            img_fps: "24".into(),
            img_res: "1920x1080".into(),
            img_cw: "1280".into(),
            img_ch: "720".into(),
            img_fit: "pad".into(),
            img_crf: "12".into(),
            target_mb: "10".into(),
            keep_audio: true,
            trim_start: "0".into(),
            trim_dur: "1".into(),
            trim_precise: false,
            audio_format: "mp3".into(),
            rotate_pick: "90".into(),
        }
    }
}

fn def_false() -> bool { false }
fn def_true() -> bool { true }

// ── 工具定位 ──
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    /// 日志用：边个 ffmpeg（sidecar 9.0.2 / 系统 homebrew …）
    pub source_desc: String,
}

/// 探测次序（派单）：sidecar 优先，冇就系统 /opt/homebrew/bin/ffmpeg 兜底，日志注明用咗边个。
pub fn resolve_tools(ffmpeg_path: Option<PathBuf>, ffprobe_path: Option<PathBuf>) -> Tools {
    let side_ffmpeg = ffmpeg_path.unwrap_or_default();
    let side_ffprobe = ffprobe_path.unwrap_or_default();
    if side_ffmpeg.is_file() && side_ffprobe.is_file() {
        return Tools {
            source_desc: format!(
                "内置 sidecar {}（martin-riedl 静态版 9.0.2）",
                side_ffmpeg.display()
            ),
            ffmpeg: side_ffmpeg,
            ffprobe: side_ffprobe,
        };
    }
    for base in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let f = format!("{base}/ffmpeg");
        let p = format!("{base}/ffprobe");
        if Path::new(&f).is_file() && Path::new(&p).is_file() {
            return Tools {
                source_desc: format!("系统 {f}（homebrew 兜底）"),
                ffmpeg: f.into(),
                ffprobe: p.into(),
            };
        }
    }
    Tools {
        source_desc: "未找到".into(),
        ffmpeg: PathBuf::from("ffmpeg"),
        ffprobe: PathBuf::from("ffprobe"),
    }
}

// ── 基础执行（对应 Swift run / runStdout；配方参数不动） ──
pub fn run(tool: &Path, args: &[String]) -> (bool, String) {
    let out = Command::new(tool).args(args).output();
    match out {
        Ok(o) => (o.status.success(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (false, format!("启动失败：{e}")),
    }
}

pub fn run_stdout(tool: &Path, args: &[String]) -> (bool, String, String) {
    let out = Command::new(tool).args(args).output();
    match out {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
        ),
        Err(e) => (false, String::new(), format!("启动失败：{e}")),
    }
}

/// 报错摘要（对应 Swift errSummary：优先挑含 Error/Invalid/does not contain/No such 的行，最多 2 行；否则末 3 行；截 400 字）
pub fn err_summary(stderr: &str, limit: usize) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let interesting: Vec<&str> = lines
        .iter()
        .filter(|l| {
            let l = *l;
            l.contains("Error")
                || l.contains("Invalid")
                || l.contains("does not contain")
                || l.contains("No such")
        })
        .copied()
        .collect();
    let picked: Vec<&str> = if interesting.is_empty() {
        lines.iter().rev().take(3).map(|s| *s).collect()
    } else {
        interesting.iter().rev().take(2).map(|s| *s).collect()
    };
    let mut out: String = if picked.is_empty() {
        "ffmpeg 退出码非 0（无详细报错）".into()
    } else {
        picked.join(" | ")
    };
    if out.chars().count() > limit {
        out = out
            .chars()
            .rev()
            .take(limit)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
    }
    out
}

/// 重名不覆盖，加序号（对应 Swift unique）
pub fn unique(p: &Path) -> PathBuf {
    let mut u = p.to_path_buf();
    let mut i = 1;
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = p.parent().map(PathBuf::from).unwrap_or_default();
    while u.exists() {
        let name = if ext.is_empty() {
            format!("{stem}-{i}")
        } else {
            format!("{stem}-{i}.{ext}")
        };
        u = parent.join(name);
        i += 1;
    }
    u
}

/// 重名目录不覆盖（对应 Swift uniqueDir；创建中间目录）
pub fn unique_dir(p: &Path) -> PathBuf {
    let mut d = p.to_path_buf();
    let mut i = 1;
    let stem = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = p.parent().map(PathBuf::from).unwrap_or_default();
    while d.exists() && d.read_dir().map(|mut r| r.next().is_some()).unwrap_or(false) {
        d = parent.join(format!("{stem}-{i}"));
        i += 1;
    }
    let _ = fs::create_dir_all(&d);
    d
}

/// 临时清单文件（imgToVideo 散落图 / concat / toSize passlogfile），对应 Swift temp 目录方案
pub fn temp_unique(prefix: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("{prefix}-{}-{}", ts, std::process::id()))
}

// ── ffprobe（对应 Swift Info / probe / infoText） ──
#[derive(Default, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub duration: f64,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub video_codec: String,
    pub video_br: i64,
    pub audio_codec: String,
    pub has_audio: bool,
}

pub fn probe_json(file: &Path, tools: &Tools) -> Result<String, String> {
    let args: Vec<String> = vec![
        "-v".into(), "quiet".into(), "-print_format".into(), "json".into(),
        "-show_format".into(), "-show_streams".into(), file.to_string_lossy().into(),
    ];
    let (ok, out, err) = run_stdout(&tools.ffprobe, &args);
    if !ok {
        return Err(format!(
            "ffprobe 读取失败：{}{}",
            file.file_name().map(|s| s.to_string_lossy()).unwrap_or_default(),
            if err.trim().is_empty() { String::new() } else { format!("（{err}）") }
        ));
    }
    Ok(out)
}

pub fn probe(file: &Path, tools: &Tools) -> Result<Info, String> {
    let json = probe_json(file, tools)?;
    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    let mut info = Info::default();
    if let Some(s) = v.get("format").and_then(|f| f.get("duration")).and_then(|d| d.as_str()) {
        info.duration = s.parse().unwrap_or(0.0);
    }
    for s in v.get("streams").and_then(|s| s.as_array()).into_iter().flatten() {
        match s.get("codec_type").and_then(|c| c.as_str()) {
            Some("video") => {
                info.width = s.get("width").and_then(|w| w.as_i64()).unwrap_or(0);
                info.height = s.get("height").and_then(|w| w.as_i64()).unwrap_or(0);
                info.video_codec = s
                    .get("codec_name")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                info.video_br = s
                    .get("bit_rate")
                    .and_then(|b| b.as_str())
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0);
                if let Some(fr) = s.get("r_frame_rate").and_then(|r| r.as_str()) {
                    let parts: Vec<&str> = fr.split('/').collect();
                    if parts.len() == 2 {
                        if let (Ok(num), Ok(den)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                            if den > 0.0 {
                                info.fps = num / den;
                            }
                        }
                    }
                }
            }
            Some("audio") => {
                info.has_audio = true;
                info.audio_codec = s
                    .get("codec_name")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            _ => {}
        }
    }
    Ok(info)
}

/// 人类可读大小（近似 macOS ByteCountFormatter.file：1024 进制，<100 一位小数）
pub fn human_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        let kb = b / KB;
        if kb >= 100.0 { format!("{kb:.0} KB") } else { format!("{kb:.1} KB") }
    } else if b < KB.powi(3) {
        let mb = b / KB.powi(3);
        if mb >= 100.0 { format!("{mb:.0} MB") } else { format!("{mb:.1} MB") }
    } else {
        format!("{:.1} GB", b / KB.powi(4))
    }
}

/// 媒体信息文本（对应 Swift infoText 行格式）
pub fn info_text(file: &Path, tools: &Tools) -> Result<String, String> {
    let size = fs::metadata(file).map(|m| m.len() as i64).unwrap_or(0);
    let json = probe_json(file, tools)?;
    let mut lines: Vec<String> = vec![];
    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    if let Some(dur) = v
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
    {
        lines.push(format!("时长：{dur} 秒"));
    }
    for s in v.get("streams").and_then(|s| s.as_array()).into_iter().flatten() {
        match s.get("codec_type").and_then(|c| c.as_str()) {
            Some("video") => {
                let codec = s.get("codec_name").and_then(|c| c.as_str()).unwrap_or("—");
                let w = s.get("width").and_then(|x| x.as_i64()).unwrap_or(0);
                let h = s.get("height").and_then(|x| x.as_i64()).unwrap_or(0);
                let fr = s
                    .get("r_frame_rate")
                    .and_then(|f| f.as_str())
                    .unwrap_or("—");
                let br = s
                    .get("bit_rate")
                    .and_then(|b| b.as_str())
                    .and_then(|b| b.parse::<i64>().ok())
                    .map(|b| format!("{} kbps", b / 1000))
                    .unwrap_or_else(|| "—".into());
                let pix = s.get("pix_fmt").and_then(|p| p.as_str()).unwrap_or("");
                lines.push(format!(
                    "视频：{codec}  {w}×{h}  {fr} fps  码率 {br}  {pix}"
                ));
            }
            Some("audio") => {
                let codec = s.get("codec_name").and_then(|c| c.as_str()).unwrap_or("—");
                let rate = s
                    .get("sample_rate")
                    .and_then(|r| r.as_str())
                    .unwrap_or("—");
                let ch = s.get("channels").and_then(|c| c.as_i64()).unwrap_or(0);
                lines.push(format!("音频：{codec}  {rate} Hz  {ch} 声道"));
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        lines.push("无法解析媒体信息".into());
    }
    lines.push(format!("大小：{}", human_size(size)));
    Ok(lines.join("\n"))
}

// ── 配方构造（逐字对应 Swift Engine.plan，一字不改） ──
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub cmds: Vec<Vec<String>>,
    pub note: String,
    pub out: PathBuf,
    pub is_dir: bool,
    pub need_frame_count: bool,
    pub probe_output_duration: bool,
}

fn s(p: &str) -> String {
    p.to_string()
}

pub fn build_args(
    feature: Feature,
    source: &Path,
    sources: &[PathBuf],
    images: &[PathBuf],
    p: &Params,
    out_base: Option<&Path>,
    tools: &Tools,
) -> Result<Plan, String> {
    let src_name = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = out_base
        .map(PathBuf::from)
        .unwrap_or_else(|| source.parent().map(PathBuf::from).unwrap_or_default());
    let source = source.to_string_lossy().into_owned();

    match feature {
        Feature::Ffv1 => {
            let out = unique(&dir.join(format!("{src_name}.ffv1.mkv")));
            let mut args = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                s("-map"), s("0:v"), s("-map"), s("0:a?"),
                s("-c:v"), s("ffv1"), s("-level"), s("3"), s("-coder"), s("1"), s("-context"), s("1"),
                s("-g"), s("1"), s("-slices"), s("24"), s("-slicecrc"), s("1"),
                s("-c:a"), s("flac"),
            ];
            args.push(out.to_string_lossy().into_owned());
            Ok(Plan {
                cmds: vec![args],
                note: "FFV1 无损母带（逐帧可验证，alpha 保留）".into(),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: false,
            })
        }
        Feature::X265 => {
            let out = unique(&dir.join(format!("{src_name}.x265.mp4")));
            let mut args = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                s("-c:v"), s("libx265"), s("-crf"), p.crf_x265.clone(), s("-preset"), s("slow"),
                s("-tag:v"), s("hvc1"), s("-c:a"), s("copy"),
            ];
            args.push(out.to_string_lossy().into_owned());
            Ok(Plan {
                cmds: vec![args],
                note: format!("x265 CRF {}（hvc1 标签保苹果可播）", p.crf_x265),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: false,
            })
        }
        Feature::Gif => {
            let out = unique(&dir.join(format!("{src_name}.gif")));
            let scale = if p.gif_width == "orig" {
                String::new()
            } else {
                format!("scale={w}:-1:flags=lanczos,", w = p.gif_width)
            };
            let vf = format!(
                "fps={},{}split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle",
                p.gif_fps, scale
            );
            let mut args = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(), s("-vf"), vf,
            ];
            args.push(out.to_string_lossy().into_owned());
            let preset = if p.gif_preset == "size" { "体积优先" } else { "画质优先" };
            let width_desc = if p.gif_width == "orig" { "原始" } else { &p.gif_width };
            Ok(Plan {
                cmds: vec![args],
                note: format!(
                    "GIF（{preset}：stats_mode=diff + bayer 抖动 diff_mode=rectangle；fps={}，宽={width_desc}）",
                    p.gif_fps
                ),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: false,
            })
        }
        Feature::PngSeq => {
            let fdir = unique_dir(&dir.join(format!("{src_name}_frames")));
            let mut args = vec![s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone()];
            if p.png_fps1 {
                args.push(s("-vf"));
                args.push(s("fps=1"));
            }
            args.push(fdir.join("frame_%06d.png").to_string_lossy().into_owned());
            let desc = if p.png_fps1 { "每秒 1 帧" } else { "全部帧，逐帧无损" };
            Ok(Plan {
                cmds: vec![args],
                note: format!("{desc} → {}/", fdir.file_name().map(|x| x.to_string_lossy()).unwrap_or_default()),
                out: fdir,
                is_dir: true,
                need_frame_count: true,
                probe_output_duration: false,
            })
        }
        Feature::ImgToVideo => {
            let n = images.len();
            if n == 0 {
                return Err("没有图片输入".into());
            }
            let mut args = vec![s("-hide_banner"), s("-nostdin"), s("-y")];
            let f0 = &images[0];
            let same_dir = n == 1
                || images[1..].iter().all(|im| im.parent() == f0.parent());
            let note_input;
            if same_dir {
                let glob = f0
                    .parent()
                    .map(|d| format!("{}/*.{}", d.display(), f0.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()))
                    .ok_or("图片路径缺少目录")?;
                args.extend([
                    s("-framerate"), p.img_fps.clone(),
                    s("-pattern_type"), s("glob"), s("-i"), glob.clone(),
                ]);
                note_input = format!("glob {glob}");
            } else {
                let list_dir = temp_unique("vct-list");
                fs::create_dir_all(&list_dir).map_err(|e| e.to_string())?;
                let dur = 1.0 / (p.img_fps.parse::<f64>().unwrap_or(24.0)).max(1.0);
                let mut lines = String::new();
                for (i, img) in images.iter().enumerate() {
                    lines.push_str(&format!("file '{}'\nduration {dur:.6}\n", img.display()));
                    if i == n - 1 {
                        lines.push_str(&format!("file '{}'\n", img.display()));
                    }
                }
                let list_file = list_dir.join("list.txt");
                fs::write(&list_file, lines).map_err(|e| e.to_string())?;
                args.extend([
                    s("-f"), s("concat"), s("-safe"), s("0"), s("-i"),
                    list_file.to_string_lossy().into_owned(),
                ]);
                note_input = format!("concat 清单（{n} 张散落图片，按文件名排序）");
            }
            let (mut w, mut h) = (p.img_cw.clone(), p.img_ch.clone());
            match p.img_res.as_str() {
                "1920x1080" => { w = "1920".into(); h = "1080".into(); }
                "1080x1920" => { w = "1080".into(); h = "1920".into(); }
                "1080x1080" => { w = "1080".into(); h = "1080".into(); }
                "orig" => {
                    let ip = probe(f0, tools)?;
                    w = ip.width.to_string();
                    h = ip.height.to_string();
                }
                _ => {}
            }
            let vf = if p.img_fit == "pad" {
                format!(
                    "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1"
                )
            } else {
                format!(
                    "scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1"
                )
            };
            let out = unique(&dir.join(format!("图片合成_{n}张.mp4")));
            let fit_desc = if p.img_fit == "pad" { "完整显示+补边" } else { "裁切填满" };
            args.extend([
                s("-vf"), vf, s("-c:v"), s("libx264"), s("-pix_fmt"), s("yuv420p"),
                s("-crf"), p.img_crf.clone(), s("-r"), p.img_fps.clone(),
            ]);
            args.push(out.to_string_lossy().into_owned());
            Ok(Plan {
                cmds: vec![args],
                note: format!(
                    "图片合成：{note_input}；{fit_desc} {w}×{h}；libx264 CRF {}；{} fps",
                    p.img_crf, p.img_fps
                ),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: true,
            })
        }
        Feature::ToSize => {
            let target = p.target_mb.parse::<f64>().unwrap_or(10.0);
            let info = probe(source_path(source.as_str()), tools)?;
            let dur = info.duration.max(0.5);
            let has_audio = p.keep_audio && info.has_audio;
            let v = (target * 8192.0 / dur - if has_audio { 128.0 } else { 0.0 }).max(1.0);
            let v_int = v as i64; // Swift Int(v)：截断
            let out = unique(&dir.join(format!("{src_name}_{}MB.mp4", p.target_mb)));
            let passlog = temp_unique("vct-pass");
            let pass1 = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                s("-c:v"), s("libx264"), s("-b:v"), format!("{v_int}k"), s("-pass"), s("1"),
                s("-passlogfile"), passlog.to_string_lossy().into_owned(),
                s("-an"), s("-f"), s("null"), s("-"),
            ];
            let mut pass2 = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                s("-c:v"), s("libx264"), s("-b:v"), format!("{v_int}k"), s("-pass"), s("2"),
                s("-passlogfile"), passlog.to_string_lossy().into_owned(),
            ];
            if has_audio {
                pass2.extend([s("-c:a"), s("aac"), s("-b:a"), s("128k")]);
            } else {
                pass2.push(s("-an"));
            }
            pass2.push(out.to_string_lossy().into_owned());
            let audio_note = if has_audio { "− 128k 音频" } else { "无音轨" };
            Ok(Plan {
                cmds: vec![pass1, pass2],
                note: format!(
                    "压到 {}MB：两遍 x264，视频码率 ≈{v_int} kbps（目标×8192÷时长 {dur:.1}s {audio_note}），{}",
                    p.target_mb,
                    if has_audio { "音轨保留 128k" } else { "源无音轨" }
                ),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: true,
            })
        }
        Feature::Concat => {
            if sources.len() < 2 {
                return Err("拼接至少需要 2 个视频段".into());
            }
            let first = probe(&sources[0], tools)?;
            for src in &sources[1..] {
                let si = probe(src, tools)?;
                if si.video_codec != first.video_codec
                    || si.width != first.width
                    || si.height != first.height
                    || (si.fps - first.fps).abs() > 0.51
                {
                    return Err(format!(
                        "拼接失败：{} 与 {} 规格不一致（编码/分辨率/帧率）。请先统一规格；「重编码拼接」模式后续提供。",
                        src.file_name().map(|x| x.to_string_lossy()).unwrap_or_default(),
                        sources[0].file_name().map(|x| x.to_string_lossy()).unwrap_or_default()
                    ));
                }
            }
            let list_dir = temp_unique("vct-concat");
            fs::create_dir_all(&list_dir).map_err(|e| e.to_string())?;
            let list_file = list_dir.join("concat.txt");
            let text = sources
                .iter()
                .map(|p| format!("file '{}'", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(&list_file, text).map_err(|e| e.to_string())?;
            let out = unique(&dir.join(format!("合并_{}段.mp4", sources.len())));
            let mut args = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-f"), s("concat"), s("-safe"), s("0"),
                s("-i"), list_file.to_string_lossy().into_owned(), s("-c"), s("copy"),
            ];
            args.push(out.to_string_lossy().into_owned());
            Ok(Plan {
                cmds: vec![args],
                note: format!("拼接 {} 段（同规格，-c copy 不重编码）", sources.len()),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: true,
            })
        }
        Feature::Trim => {
            let out = unique(&dir.join(format!("{src_name}_{}s-{}s.mp4", p.trim_start, p.trim_dur)));
            if p.trim_precise {
                let info = probe(source_path(source.as_str()), tools)?;
                let mut args = vec![
                    s("-hide_banner"), s("-nostdin"), s("-y"), s("-ss"), p.trim_start.clone(),
                    s("-i"), source.clone(), s("-t"), p.trim_dur.clone(),
                    s("-c:v"), s("libx264"), s("-crf"), s("12"),
                ];
                if info.has_audio {
                    args.extend([s("-c:a"), s("copy")]);
                }
                let a_note = if info.has_audio { "音频拷贝" } else { "无音轨" };
                args.push(out.to_string_lossy().into_owned());
                Ok(Plan {
                    cmds: vec![args],
                    note: format!(
                        "裁剪 精确模式（{}s 起 {}s，重编码 CRF 12，{a_note}，分毫不差）",
                        p.trim_start, p.trim_dur
                    ),
                    out,
                    is_dir: false,
                    need_frame_count: false,
                    probe_output_duration: true,
                })
            } else {
                let mut args = vec![
                    s("-hide_banner"), s("-nostdin"), s("-y"), s("-ss"), p.trim_start.clone(),
                    s("-i"), source.clone(), s("-t"), p.trim_dur.clone(), s("-c"), s("copy"),
                ];
                args.push(out.to_string_lossy().into_owned());
                Ok(Plan {
                    cmds: vec![args],
                    note: format!(
                        "裁剪 快速模式（{}s 起 {}s，-c copy；按关键帧对齐，时长约 1 秒内出入）",
                        p.trim_start, p.trim_dur
                    ),
                    out,
                    is_dir: false,
                    need_frame_count: false,
                    probe_output_duration: true,
                })
            }
        }
        Feature::Extract => {
            if p.audio_format == "wav" {
                let out = unique(&dir.join(format!("{src_name}.wav")));
                let mut args = vec![
                    s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                    s("-vn"), s("-c:a"), s("pcm_s16le"),
                ];
                args.push(out.to_string_lossy().into_owned());
                Ok(Plan {
                    cmds: vec![args],
                    note: "提取音频 WAV（无损 pcm_s16le）".into(),
                    out,
                    is_dir: false,
                    need_frame_count: false,
                    probe_output_duration: false,
                })
            } else {
                let out = unique(&dir.join(format!("{src_name}.mp3")));
                let mut args = vec![
                    s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                    s("-vn"), s("-c:a"), s("libmp3lame"), s("-q:a"), s("2"),
                ];
                args.push(out.to_string_lossy().into_owned());
                Ok(Plan {
                    cmds: vec![args],
                    note: "提取音频 MP3（q:a 2）".into(),
                    out,
                    is_dir: false,
                    need_frame_count: false,
                    probe_output_duration: false,
                })
            }
        }
        Feature::Mute => {
            let out = unique(&dir.join(format!("{src_name}-静音.mp4")));
            let mut args = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                s("-an"), s("-c:v"), s("copy"),
            ];
            args.push(out.to_string_lossy().into_owned());
            Ok(Plan {
                cmds: vec![args],
                note: "一键静音（视频流原样拷贝）".into(),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: false,
            })
        }
        Feature::Rotate => {
            let vf = match p.rotate_pick.as_str() {
                "90" => "transpose=1",
                "270" => "transpose=2",
                "180" => "hflip,vflip",
                "landscape" => "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1",
                "portrait" => "scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2,setsar=1",
                _ => "transpose=1",
            }
            .to_string();
            let out = unique(&dir.join(format!("{src_name}-旋转{}.mp4", p.rotate_pick)));
            let info = probe(source_path(source.as_str()), tools)?;
            let mut args = vec![
                s("-hide_banner"), s("-nostdin"), s("-y"), s("-i"), source.clone(),
                s("-vf"), vf.clone(), s("-c:v"), s("libx264"), s("-crf"), s("12"),
            ];
            if p.rotate_pick == "landscape" || p.rotate_pick == "portrait" {
                args.extend([s("-pix_fmt"), s("yuv420p")]);
            }
            if info.has_audio {
                args.extend([s("-c:a"), s("copy")]);
            }
            let a_note = if info.has_audio { "音频拷贝" } else { "无音轨" };
            args.push(out.to_string_lossy().into_owned());
            Ok(Plan {
                cmds: vec![args],
                note: format!("{}（重编码 CRF 12，{a_note}）", p.rotate_pick),
                out,
                is_dir: false,
                need_frame_count: false,
                probe_output_duration: false,
            })
        }
    }
}

fn source_path(src: &str) -> &Path {
    Path::new(src)
}

/// 任务总时长（微秒），对应 Swift jobTotalUs（进度分母）
pub fn job_total_us(
    feature: Feature,
    source: &Path,
    sources: &[PathBuf],
    images: &[PathBuf],
    p: &Params,
    tools: &Tools,
) -> i64 {
    let us: i64 = match feature {
        Feature::ToSize => {
            let d = probe(source, tools).map(|i| i.duration).unwrap_or(0.0);
            (d * 1_000_000.0 * 2.0) as i64 // 两遍编码
        }
        Feature::Concat => {
            let sum: f64 = sources
                .iter()
                .map(|s| probe(s, tools).map(|i| i.duration).unwrap_or(0.0))
                .sum();
            (sum * 1_000_000.0) as i64
        }
        Feature::ImgToVideo => {
            let fps = p.img_fps.parse::<f64>().unwrap_or(24.0).max(1.0);
            ((images.len().max(1) as f64 / fps) * 1_000_000.0) as i64
        }
        _ => {
            let d = probe(source, tools).map(|i| i.duration).unwrap_or(0.0);
            (d * 1_000_000.0) as i64
        }
    };
    us.max(1_000_000)
}

// ── framemd5 无损验证（对应 Swift verifyLossless：-map 0:v -f framemd5 后剔除 # 头逐行比对） ──
pub fn verify_lossless(a: &Path, b: &Path, tools: &Tools) -> Result<(bool, String), String> {
    fn frame_lines(f: &Path, tools: &Tools) -> Result<Vec<String>, String> {
        let args = vec![
            s("-hide_banner"), s("-loglevel"), s("error"), s("-i"), f.to_string_lossy().into_owned(),
            s("-map"), s("0:v"), s("-f"), s("framemd5"), s("-"),
        ];
        let (ok, out, err) = run_stdout(&tools.ffmpeg, &args);
        if !ok {
            return Err(format!(
                "framemd5 读取失败：{}{}",
                f.file_name().map(|x| x.to_string_lossy()).unwrap_or_default(),
                if err.trim().is_empty() { String::new() } else { format!("（{}）", err.lines().next().unwrap_or("")) }
            ));
        }
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_string())
            .collect())
    }
    let ma = frame_lines(a, tools)?;
    let mb = frame_lines(b, tools)?;
    if ma.len() != mb.len() {
        return Ok((
            false,
            format!("帧数不一致（源 {} 帧 vs 产物 {} 帧）", ma.len(), mb.len()),
        ));
    }
    let mut diff = 0;
    let mut first_diff: isize = -1;
    for i in 0..ma.len() {
        if ma[i] != mb[i] {
            diff += 1;
            if first_diff < 0 {
                first_diff = i as isize;
            }
        }
    }
    if diff == 0 {
        Ok((true, format!("逐帧一致（{} 帧全等）", ma.len())))
    } else {
        Ok((false, format!("第 {} 帧起有差异，共 {diff} 帧不同", first_diff + 1)))
    }
}

// ── 抽帧（队列缩略图 / 预览大图；对应 Swift framePath） ──
pub fn frame_path(
    file: &Path,
    is_video: bool,
    long_edge: i64,
    tools: &Tools,
    cache_dir: &Path,
) -> Option<PathBuf> {
    if !is_video {
        return Some(file.to_path_buf());
    }
    let dir = cache_dir.join("vct-frames");
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let name = file
        .file_name()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dst = dir.join(format!("{name}-{long_edge}.jpg"));
    if dst.exists() {
        return Some(dst);
    }
    for ss in ["1", "0"] {
        let args = vec![
            s("-hide_banner"), s("-loglevel"), s("error"), s("-ss"), s(ss), s("-i"),
            file.to_string_lossy().into_owned(), s("-frames:v"), s("1"),
            s("-vf"), format!("scale={long_edge}:-2"), s("-f"), s("image2"), s("-y"),
            dst.to_string_lossy().into_owned(),
        ];
        let (ok, _) = run(&tools.ffmpeg, &args);
        if ok && dst.exists() {
            return Some(dst);
        }
    }
    None
}

/// 目录总大小（对应 Swift dirSize）
pub fn dir_size(dir: &Path) -> i64 {
    fn walk(path: &Path, total: &mut i64) {
        if let Ok(rd) = fs::read_dir(path) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, total);
                } else if let Ok(m) = fs::metadata(&p) {
                    *total += m.len() as i64;
                }
            }
        }
    }
    let mut total: i64 = 0;
    walk(dir, &mut total);
    total
}

/// 抽帧缓存目录（app 缓存目录下；供缩略图 + asset protocol）
pub fn frame_cache_dir(app_cache: &Path) -> PathBuf {
    let d = app_cache.join("vct-frames");
    let _ = fs::create_dir_all(&d);
    d
}
