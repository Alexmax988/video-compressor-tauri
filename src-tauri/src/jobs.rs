// jobs.rs — job runner（Rust 側執行全部 ffmpeg；前端只 invoke）
// 取消协议（派单写死）：先向 ffmpeg stdin 写 q 等优雅收尾 → 3 秒不结束先 kill → 必须 wait/收 Terminated → 确认死透先删半成品。
// 窗口关闭 / 取消按钮 → cancel_all：取消运行中 job + 跳过剩余 job，唔准留 ffmpeg 孤儿进程
// （shell plugin 本身亦会在 app 退出时 kill 佢 spawn 嘅所有 child，双保险）。
use crate::engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::async_runtime::Receiver;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::process::{CommandEvent, CommandChild};
use tauri_plugin_shell::ShellExt;

// ── 请求 / 消息（与前端约定） ──
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    pub id: u64,
    pub path: String,
    pub kind: String, // "video" | "image"
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartJobsReq {
    pub feature: engine::Feature,
    pub params: engine::Params,
    pub out_mode: String, // "same" | "custom"
    pub out_folder: Option<String>,
    pub items: Vec<ItemRef>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "k", rename_all = "camelCase")]
pub enum Msg {
    Log {
        text: String,
    },
    JobStarted {
        job: usize,
        label: String,
        total: usize,
        item_ids: Vec<u64>,
    },
    Progress {
        job: usize,
        frac: f64,
        overall: f64,
    },
    JobDone {
        job: usize,
        label: String,
        ok: bool,
        detail: String,
        out_path: Option<String>,
        out_size: Option<i64>,
        frame_count: Option<u64>,
        duration: Option<f64>,
        item_ids: Vec<u64>,
        no_gain: bool,
    },
    JobCancelled {
        job: usize,
        label: String,
        item_ids: Vec<u64>,
    },
    AllDone {
        ok_count: usize,
        fail_count: usize,
        cancelled_count: usize,
    },
}

// ── 批次状态（cancel_all 用） ──
pub struct BatchState {
    pub cancel: AtomicBool,
    pub done: AtomicBool,
}

pub struct JobManager {
    inner: Mutex<Option<Arc<BatchState>>>,
}

impl JobManager {
    pub fn new() -> Self {
        JobManager {
            inner: Mutex::new(None),
        }
    }

    /// 取消全部：置位 cancel flag。runner 线程会按协议收尾（q → 3s → kill → 确认死透 → 删半成品），
    /// 剩余排队 job 跳过。调用方可轮询 batch.done 确认全部结束（窗口关闭用）。
    pub fn cancel_all(&self) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(batch) = guard.as_ref() {
                batch.cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    /// 等批次收尾（窗口关闭用；最坏 ~3.5s 的 q/kill 协议窗口）
    pub fn wait_batch(&self, timeout: Duration) {
        let batch = self.inner.lock().ok().and_then(|g| g.clone());
        if let Some(b) = batch {
            let deadline = Instant::now() + timeout;
            while !b.done.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// ── 工具解析（派单：sidecar 优先 → 系统 homebrew 兜底，日志注明用咗边个） ──
#[cfg(target_os = "windows")]
const TRIPLE: &str = "x86_64-pc-windows-msvc";
#[cfg(not(target_os = "windows"))]
const TRIPLE: &str = "aarch64-apple-darwin";

/// Windows 上 sidecar 名带 .exe 后缀（tauri 命名规则）；其他平台无后缀。
#[cfg(target_os = "windows")]
const BIN_SUFFIX: &str = ".exe";
#[cfg(not(target_os = "windows"))]
const BIN_SUFFIX: &str = "";

/// 找 sidecar 二进制：dev（cargo run / tauri dev）tauri-build 会喺 exe 同目录放
/// 无 triple 名（target/debug/ffmpeg）；release bundle 喺 exe / Resources 放
/// 带 triple 名（ffmpeg-aarch64-apple-darwin）。两边都试。
fn find_tool(app: &AppHandle, base: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(format!("{base}-{TRIPLE}{BIN_SUFFIX}")));
            candidates.push(dir.join(format!("{base}{BIN_SUFFIX}")));
            candidates.push(dir.join(base));
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join(format!("{base}-{TRIPLE}{BIN_SUFFIX}")));
        candidates.push(res.join(base));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn resolve(app: &AppHandle) -> Result<engine::Tools, String> {
    if let (Some(ff), Some(pp)) = (find_tool(app, "ffmpeg"), find_tool(app, "ffprobe")) {
        #[cfg(target_os = "windows")]
        let desc = format!(
            "内置 sidecar BtbN 静态版（{}）",
            ff.display()
        );
        #[cfg(not(target_os = "windows"))]
        let desc = format!(
            "内置 sidecar martin-riedl 静态版 9.0.2（{}）",
            ff.display()
        );
        return Ok(engine::Tools {
            source_desc: desc,
            ffmpeg: ff,
            ffprobe: pp,
        });
    }
    for base in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let f = format!("{base}/ffmpeg");
        let p = format!("{base}/ffprobe");
        if Path::new(&f).is_file() && Path::new(&p).is_file() {
            return Ok(engine::Tools {
                source_desc: format!("系统 {f}（homebrew 兜底）"),
                ffmpeg: f.into(),
                ffprobe: p.into(),
            });
        }
    }
    Err("找不到 ffmpeg/ffprobe（试过内置 sidecar 与 /opt/homebrew/bin、/usr/local/bin）".into())
}

/// 生成 job 命令（配方参数 + 进度选项 + 去掉 -nostdin 以便取消写 q）
fn job_args(cmd: &[String]) -> Vec<String> {
    let mut out: Vec<String> = cmd
        .iter()
        .filter(|a| a.as_str() != "-nostdin") // 取消协议需要 stdin 收 q
        .cloned()
        .collect();
    out.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    out
}

/// 用 shell plugin spawn（派单要求 app.shell().sidecar("ffmpeg")；系统兜底走 command(path)）
fn spawn(
    app: &AppHandle,
    tools: &engine::Tools,
    args: &[String],
) -> Result<(Receiver<CommandEvent>, CommandChild), String> {
    // sidecar（喺 exe 同目录，dev/release 都係 tauri-build 摆落去嘅）→ 走 plugin 嘅 sidecar()
    let is_sidecar = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| tools.ffmpeg.starts_with(&d))
        .unwrap_or(false);
    let cmd = if is_sidecar {
        let name = tools
            .ffmpeg
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_else(|| std::ffi::OsString::from("ffmpeg"));
        app.shell()
            .sidecar(name)
            .map_err(|e| format!("sidecar 解析失败：{e}"))?
    } else {
        // 系统兜底：显式路径
        app.shell().command(&tools.ffmpeg)
    };
    let cmd = cmd.args(args);
    let (rx, child) = cmd
        .spawn()
        .map_err(|e| format!("ffmpeg spawn 失败：{e}"))?;
    Ok((rx, child))
}

// ── 跑一个批次（独立线程） ──
pub fn start_batch(
    app: AppHandle,
    req: StartJobsReq,
    channel: Channel<Msg>,
    mgr: &JobManager,
) -> Result<(), String> {
    let tools = resolve(&app)?;
    let batch = Arc::new(BatchState {
        cancel: AtomicBool::new(false),
        done: AtomicBool::new(false),
    });
    {
        let mut guard = mgr.inner.lock().map_err(|_| "状态锁损坏".to_string())?;
        *guard = Some(batch.clone());
    }
    let ch = channel.clone();
    std::thread::spawn(move || {
        run_batch(app, req, tools, &ch, &batch);
        batch.done.store(true, Ordering::SeqCst);
    });
    Ok(())
}

fn run_batch(
    app: AppHandle,
    req: StartJobsReq,
    tools: engine::Tools,
    channel: &Channel<Msg>,
    batch: &Arc<BatchState>,
) {
    let log = |text: String| {
        let _ = channel.send(Msg::Log { text });
    };
    log(format!("提示：ffmpeg 使用 {}", tools.source_desc));

    // 按功能过滤队列（对应旧 AppController.startClicked 的分流逻辑）
    let videos: Vec<ItemRef> = req
        .items
        .iter()
        .filter(|i| i.kind == "video")
        .cloned()
        .collect();
    let images: Vec<ItemRef> = req
        .items
        .iter()
        .filter(|i| i.kind == "image")
        .cloned()
        .collect();

    let out_base: Option<PathBuf> = match req.out_mode.as_str() {
        "custom" => match req.out_folder.clone() {
            Some(f) if !f.is_empty() => Some(PathBuf::from(f)),
            _ => {
                log("⚠️ 自选文件夹还没选".into());
                return;
            }
        },
        _ => None,
    };

    // 构造 jobs
    let mut jobs: Vec<(Vec<ItemRef>, String, engine::Plan)> = Vec::new();
    match req.feature {
        engine::Feature::Concat => {
            if videos.len() < 2 {
                log("⚠️ 拼接至少需要 2 个视频段".into());
                return;
            }
            let srcs: Vec<PathBuf> = videos.iter().map(|v| PathBuf::from(&v.path)).collect();
            match engine::build_args(req.feature, &srcs[0], &srcs, &[], &req.params, out_base.as_deref(), &tools)
            {
                Ok(plan) => jobs.push((videos.clone(), format!("拼接 {} 段", videos.len()), plan)),
                Err(e) => {
                    log(format!("⚠️ {e}"));
                    return;
                }
            }
        }
        engine::Feature::ImgToVideo => {
            if images.is_empty() {
                log("⚠️ 需要图片文件（或整个图片文件夹）".into());
                return;
            }
            let mut sorted = images.clone();
            sorted.sort_by(|a, b| {
                let na = a.path.rsplit('/').next().unwrap_or(&a.path);
                let nb = b.path.rsplit('/').next().unwrap_or(&b.path);
                na.cmp(nb)
            });
            let imgs: Vec<PathBuf> = sorted.iter().map(|v| PathBuf::from(&v.path)).collect();
            match engine::build_args(req.feature, &imgs[0], &[], &imgs, &req.params, out_base.as_deref(), &tools)
            {
                Ok(plan) => jobs.push((sorted.clone(), format!("图片合成 {} 张", sorted.len()), plan)),
                Err(e) => {
                    log(format!("⚠️ {e}"));
                    return;
                }
            }
        }
        _ => {
            if videos.is_empty() {
                log("⚠️ 该功能需要视频文件".into());
                return;
            }
            for v in &videos {
                let p = PathBuf::from(&v.path);
                match engine::build_args(req.feature, &p, std::slice::from_ref(&p), &[], &req.params, out_base.as_deref(), &tools)
                {
                    Ok(plan) => jobs.push((vec![v.clone()], v.path.clone(), plan)),
                    Err(e) => log(format!("⚠️ {}: {e}", v.path)),
                }
            }
            if jobs.is_empty() {
                log("⚠️ 没有可执行的任务".into());
                return;
            }
        }
    }

    let total = jobs.len();
    let mut done = 0usize;
    let mut ok_n = 0usize;
    let mut fail_n = 0usize;
    let mut cancelled_n = 0usize;

    for (i, (items, label, plan)) in jobs.iter().enumerate() {
        let item_ids: Vec<u64> = items.iter().map(|x| x.id).collect();
        if batch.cancel.load(Ordering::SeqCst) {
            cancelled_n += 1;
            let _ = channel.send(Msg::JobCancelled {
                job: i,
                label: label.clone(),
                item_ids,
            });
            continue;
        }
        let _ = channel.send(Msg::JobStarted {
            job: i,
            label: label.clone(),
            total,
            item_ids: item_ids.clone(),
        });
        log(format!("正在转换 {i}/{total}：{label}（{note}）", note = plan.note));

        let total_us = {
            let srcs: Vec<PathBuf> = if req.feature == engine::Feature::Concat {
                items.iter().map(|x| PathBuf::from(&x.path)).collect()
            } else {
                Vec::new()
            };
            let imgs: Vec<PathBuf> = if req.feature == engine::Feature::ImgToVideo {
                items.iter().map(|x| PathBuf::from(&x.path)).collect()
            } else {
                Vec::new()
            };
            engine::job_total_us(req.feature, Path::new(&items[0].path), &srcs, &imgs, &req.params, &tools)
        };
        let in_size: i64 = items.iter().map(|it| file_size(Path::new(&it.path))).sum();

        let result = run_one_job(&app, &tools, plan, total_us, i, channel, batch);

        match result {
            JobResult::Done {
                ok,
                detail,
                out_size,
                frame_count,
                duration,
            } => {
                let no_gain = if ok {
                    // 新增①：产物唔好大过原文件（FFV1 无损路线除外）
                    req.feature != engine::Feature::Ffv1
                        && out_size.unwrap_or(0) >= in_size
                        && in_size > 0
                } else {
                    false
                };
                if ok {
                    ok_n += 1;
                    if no_gain {
                        log(format!(
                            "⚠️ {label}：产物 {} ≥ 原始 {}，本次转换无体积增益（非 FFV1 无损路线；建议更低 CRF 或换 x265 路线再压）",
                            engine::human_size(out_size.unwrap_or(0)),
                            engine::human_size(in_size)
                        ));
                    }
                } else {
                    fail_n += 1;
                    log(format!("❌ {label}：{}", detail.lines().next().unwrap_or("失败")));
                }
                done += 1;
                let _ = channel.send(Msg::JobDone {
                    job: i,
                    label: label.clone(),
                    ok,
                    detail,
                    out_path: if ok {
                        Some(plan.out.to_string_lossy().to_string())
                    } else {
                        None
                    },
                    out_size,
                    frame_count,
                    duration,
                    item_ids,
                    no_gain,
                });
                let _ = channel.send(Msg::Progress {
                    job: i,
                    frac: 1.0,
                    overall: done as f64 / total as f64,
                });
            }
            JobResult::Cancelled => {
                cancelled_n += 1;
                let _ = channel.send(Msg::JobCancelled {
                    job: i,
                    label: label.clone(),
                    item_ids,
                });
                break; // 取消后剩余 job 全部跳过
            }
        }
        if batch.cancel.load(Ordering::SeqCst) {
            break;
        }
    }

    let msg = if fail_n == 0 && cancelled_n == 0 {
        format!("全部完成：{ok_n} 项成功。")
    } else if cancelled_n > 0 {
        format!(
            "已取消：{ok_n} 项成功，{fail_n} 项失败，{cancelled_n} 项取消（半成品已删除，无 ffmpeg 残留）。"
        )
    } else {
        format!("完成：{ok_n} 项成功，{fail_n} 项失败。失败项的 ffmpeg 报错摘要已显示在日志与列表中。")
    };
    log(msg);
    let _ = channel.send(Msg::AllDone {
        ok_count: ok_n,
        fail_count: fail_n,
        cancelled_count: cancelled_n,
    });
}

fn file_size(p: &Path) -> i64 {
    std::fs::metadata(p).map(|m| m.len() as i64).unwrap_or(0)
}

enum JobResult {
    Done {
        ok: bool,
        detail: String,
        out_size: Option<i64>,
        frame_count: Option<u64>,
        duration: Option<f64>,
    },
    Cancelled,
}

/// 单个 job：顺序跑 plan 内每条命令（toSize 两遍），进度分段（两遍任务 0–50 / 50–100）
fn run_one_job(
    app: &AppHandle,
    tools: &engine::Tools,
    plan: &engine::Plan,
    total_us: i64,
    job_idx: usize,
    channel: &Channel<Msg>,
    batch: &Arc<BatchState>,
) -> JobResult {
    let n = plan.cmds.len().max(1);
    let seg_us = if total_us > 0 { total_us / n as i64 } else { 0 };

    for c in 0..n {
        let args = job_args(&plan.cmds[c]);

        let (mut rx, mut child) = match spawn(app, tools, &args) {
            Ok(p) => p,
            Err(e) => {
                return JobResult::Done {
                    ok: false,
                    detail: format!("{}\n执行失败：{e}", plan.note),
                    out_size: None,
                    frame_count: None,
                    duration: None,
                };
            }
        };

        let err_tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let mut exit_code: Option<i32> = None;
        let mut stderr_err = false;

        loop {
            // 排空所有待处理事件（stdout 进度 / stderr 收集 / 结束信号），唔漏 Terminated
            loop {
                match rx.try_recv() {
                    Ok(CommandEvent::Stdout(b)) => {
                        if let Some(line) = String::from_utf8_lossy(&b).trim().strip_prefix("out_time_us=") {
                            if total_us > 0 {
                                if let Ok(us_i) = line.parse::<i64>() {
                                    let frac = ((c as i64) * seg_us + us_i.min(seg_us.max(0))) as f64
                                        / total_us as f64;
                                    let frac = frac.clamp(0.0, 1.0);
                                    let _ = channel.send(Msg::Progress {
                                        job: job_idx,
                                        frac,
                                        overall: ((c as f64 + frac) / n as f64).min(1.0),
                                    });
                                }
                            }
                        }
                    }
                    Ok(CommandEvent::Stderr(b)) => {
                        let mut g = err_tail.lock().ok();
                        if let Some(g) = g.as_mut() {
                            g.extend_from_slice(&b);
                            if g.len() > 8192 {
                                let drain = g.len() - 8192;
                                g.drain(..drain);
                            }
                        }
                    }
                    Ok(CommandEvent::Error(e)) => {
                        stderr_err = true;
                        let mut g = err_tail.lock().ok();
                        if let Some(g) = g.as_mut() {
                            g.extend_from_slice(format!("事件错误：{e}\n").as_bytes());
                        }
                    }
                    Ok(CommandEvent::Terminated(p)) => {
                        exit_code = Some(
                            p.code.unwrap_or(if p.signal.is_some() { -15 } else { -1 }),
                        );
                    }
                    Ok(_) => {}
                    Err(_) => break, // 暂无消息（plugin 保证进程结束时一定有 Terminated 事件，循环继续即可）
                }
                if exit_code.is_some() {
                    break;
                }
            }

            if exit_code.is_some() {
                break;
            }
            if batch.cancel.load(Ordering::SeqCst) {
                // ── 取消协议：stdin 写 q → 等 3s 优雅收尾 → 不结束先 kill → 必须收 Terminated ──
                let _ = child.write(b"q\n");
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut exited = false;
                loop {
                    // 收 Termination 事件（优雅退出）
                    match rx.try_recv() {
                        Ok(CommandEvent::Terminated(_)) => {
                            exited = true;
                            break;
                        }
                        _ => {}
                    }
                    if exited || Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                if !exited {
                    // 3s 未死：SIGKILL（kill 内部会 wait 收 Terminated，确认死透）
                    if let Err(e) = child.kill() {
                        let _ = e; // kill 本身失败（进程已死）唔重要
                    }
                }
                delete_partial(plan);
                return JobResult::Cancelled;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let code = exit_code.unwrap_or(-1);
        let err_text = {
            let g = err_tail.lock().ok();
            g.map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default()
        };
        if code != 0 || stderr_err {
            let cmd_text = args.join(" ");
            let err = engine::err_summary(&err_text, 400);
            return JobResult::Done {
                ok: false,
                detail: format!("{}\n{}\n报错：{err}", plan.note, cmd_text),
                out_size: None,
                frame_count: None,
                duration: None,
            };
        }
    }

    // 全部命令成功 → 后处理（对应 Swift execute 尾部）
    let mut frame_count: Option<u64> = None;
    if plan.need_frame_count {
        let cnt = std::fs::read_dir(&plan.out)
            .map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".png")).count())
            .unwrap_or(0);
        frame_count = Some(cnt as u64);
    }
    let mut duration: Option<f64> = None;
    if plan.probe_output_duration {
        duration = engine::probe(&plan.out, tools).ok().map(|i| i.duration);
    }
    let out_size = if plan.is_dir {
        Some(engine::dir_size(&plan.out))
    } else {
        Some(file_size(&plan.out))
    };
    JobResult::Done {
        ok: true,
        detail: plan.note.clone(),
        out_size,
        frame_count,
        duration,
    }
}

/// 删除半成品（取消后必删，留低就係损坏文件）
fn delete_partial(plan: &engine::Plan) {
    if plan.is_dir {
        let _ = std::fs::remove_dir_all(&plan.out);
    } else {
        let _ = std::fs::remove_file(&plan.out);
    }
}
