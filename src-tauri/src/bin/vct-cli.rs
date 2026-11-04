// vct-cli — 冒烟/自测用隐藏 CLI（对应旧 Swift 版 --cli 模式）
// 与 GUI 走同一个 engine.rs 配方（build_args 一字未改）；取消协议：stdin 写 q → 3s → kill → wait → 删半成品。
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use vct_lib::engine;

fn fail(msg: &str) -> ! {
    eprintln!("【CLI】{msg}");
    std::process::exit(1);
}

fn find_tool(name: &str) -> String {
    // 1) exe 同目录嘅 sidecar（target/debug/<name>-triple）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(format!("{name}-aarch64-apple-darwin"));
            if p.is_file() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    // 2) 工程内 binaries/（cwd = src-tauri 或项目根都试）
    if let Ok(cwd) = std::env::current_dir() {
        for rel in [
            "binaries/{n}-aarch64-apple-darwin",
            "src-tauri/binaries/{n}-aarch64-apple-darwin",
        ] {
            let p = cwd.join(rel.replace("{n}", name));
            if p.is_file() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    // 3) 系统 homebrew
    for base in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let p = format!("{base}/{name}");
        if std::path::Path::new(&p).is_file() {
            return p;
        }
    }
    name.to_string()
}

struct Opts {
    out: Option<String>,
    p: engine::Params,
    info: bool,
    verify: bool,
    cancel_after: Option<f64>,
}

fn parse_args() -> (String, Vec<String>, Opts) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts {
        out: None,
        p: engine::Params::default(),
        info: false,
        verify: false,
        cancel_after: None,
    };
    let mut it = args.into_iter().peekable();
    let mut files: Vec<String> = Vec::new();
    let feature = match it.next() {
        Some(f) if !f.starts_with("--") => f,
        _ => fail("缺少 feature（ffv1/x265/gif/png/img/tosize/concat/trim/extract/mute/rotate）"),
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => o.out = it.next(),
            "--crf" => o.p.crf_x265 = it.next().unwrap_or_default(),
            "--gif" => o.p.gif_preset = it.next().unwrap_or_default(),
            "--gif-fps" => o.p.gif_fps = it.next().unwrap_or_default(),
            "--gif-width" => o.p.gif_width = it.next().unwrap_or_default(),
            "--png-fps1" => o.p.png_fps1 = true,
            "--img-fps" => o.p.img_fps = it.next().unwrap_or_default(),
            "--img-res" => o.p.img_res = it.next().unwrap_or_default(),
            "--img-cw" => o.p.img_cw = it.next().unwrap_or_default(),
            "--img-ch" => o.p.img_ch = it.next().unwrap_or_default(),
            "--img-fit" => o.p.img_fit = it.next().unwrap_or_default(),
            "--img-crf" => o.p.img_crf = it.next().unwrap_or_default(),
            "--target" => o.p.target_mb = it.next().unwrap_or_default(),
            "--no-audio" => o.p.keep_audio = false,
            "--trim" => {
                o.p.trim_start = it.next().unwrap_or_default();
                o.p.trim_dur = it.next().unwrap_or_default();
            }
            "--precise" => o.p.trim_precise = true,
            "--audio" => o.p.audio_format = it.next().unwrap_or_default(),
            "--rotate" => o.p.rotate_pick = it.next().unwrap_or_default(),
            "--info" => o.info = true,
            "--verify" => o.verify = true,
            "--cancel-after" => o.cancel_after = it.next().and_then(|s| s.parse().ok()),
            s if s.starts_with("--") => eprintln!("【CLI】忽略未知选项：{s}"),
            f => files.push(f.to_string()),
        }
    }
    (feature, files, o)
}

/// 执行一条 ffmpeg 命令（-progress 解析进度；cancel_at Some 时到期写 q → 3s → kill → wait）
fn run_cmd(
    ffmpeg: &str,
    args: &[String],
    total_us: i64,
    seg_us: i64,
    cancel_at: Option<Instant>,
) -> (bool, String, bool) {
    let mut args2: Vec<String> = args.iter().filter(|a| a.as_str() != "-nostdin").cloned().collect();
    args2.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    let mut child = Command::new(ffmpeg)
        .args(&args2)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| fail(&format!("ffmpeg spawn 失败：{e}")));
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // stderr 后台收集
    let err_rx = {
        let mut err = stderr.unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            let _ = tx.send(s);
        });
        rx
    };

    let mut cancelled = false;
    if let Some(out) = stdout {
        let mut reader = BufReader::new(out);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(us) = line.trim().strip_prefix("out_time_us=") {
                        if total_us > 0 {
                            if let Ok(v) = us.parse::<i64>() {
                                let frac = (seg_us + v.min(seg_us.max(0))) as f64 / total_us as f64;
                                eprintln!("[progress] {:.1}%", (frac * 100.0).min(100.0));
                            }
                        }
                    }
                }
                Err(_) => break,
            }
            if let Some(deadline) = cancel_at {
                if Instant::now() >= deadline {
                    // 取消协议：q → 等 3s → kill → wait
                    if let Some(in_) = stdin.as_mut() {
                        let _ = in_.write_all(b"q\n");
                        let _ = in_.flush();
                    }
                    let d2 = Instant::now() + Duration::from_secs(3);
                    loop {
                        if child.try_wait().ok().flatten().is_some() {
                            break;
                        }
                        if Instant::now() >= d2 {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                    cancelled = true;
                    break;
                }
            }
            if child.try_wait().ok().flatten().is_some() {
                // 进程已退出（读完 stdout）
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
    let err_text = err_rx.recv().unwrap_or_default();
    (code == 0, err_text, cancelled)
}

fn main() {
    let (feature, files, o) = parse_args();
    let ffmpeg = find_tool("ffmpeg");
    let ffprobe = find_tool("ffprobe");
    if let Some(out_dir) = o.out.as_deref() {
        if let Err(e) = std::fs::create_dir_all(out_dir) {
            fail(&format!("创建输出目录失败 {out_dir}：{e}"));
        }
    }
    let ffmpeg_bin = ffmpeg.clone();
    let tools = engine::Tools {
        source_desc: "CLI".into(),
        ffmpeg: ffmpeg.into(),
        ffprobe: ffprobe.into(),
    };

    if o.info {
        for f in &files {
            println!("【媒体信息】{f}");
            println!(
                "{}",
                engine::info_text(std::path::Path::new(f), &tools).unwrap_or_else(|e| e)
            );
            println!();
        }
        return;
    }
    if o.verify {
        if files.len() != 2 {
            fail("--verify 需要两个文件：源 和 FFV1 产物");
        }
        match engine::verify_lossless(
            std::path::Path::new(&files[0]),
            std::path::Path::new(&files[1]),
            &tools,
        ) {
            Ok((identical, note)) => {
                println!("【验证】{note}");
                std::process::exit(if identical { 0 } else { 1 });
            }
            Err(e) => fail(&format!("验证失败：{e}")),
        }
    }

    // verify / info 伪功能（对应旧 CLI 的 verify/info）——用原始字符串判断
    if feature == "verify" {
        if files.len() != 2 {
            fail("--verify 需要两个文件：源 和 FFV1 产物");
        }
        match engine::verify_lossless(
            std::path::Path::new(&files[0]),
            std::path::Path::new(&files[1]),
            &tools,
        ) {
            Ok((identical, note)) => {
                println!("【验证】{note}");
                std::process::exit(if identical { 0 } else { 1 });
            }
            Err(e) => fail(&format!("验证失败：{e}")),
        }
    }
    if feature == "info" {
        for f in &files {
            println!("【媒体信息】{f}");
            println!(
                "{}",
                engine::info_text(std::path::Path::new(f), &tools).unwrap_or_else(|e| e)
            );
            println!();
        }
        return;
    }

    let fmap: Vec<(&str, engine::Feature)> = vec![
        ("ffv1", engine::Feature::Ffv1),
        ("x265", engine::Feature::X265),
        ("gif", engine::Feature::Gif),
        ("png", engine::Feature::PngSeq),
        ("img", engine::Feature::ImgToVideo),
        ("tosize", engine::Feature::ToSize),
        ("concat", engine::Feature::Concat),
        ("trim", engine::Feature::Trim),
        ("extract", engine::Feature::Extract),
        ("mute", engine::Feature::Mute),
        ("rotate", engine::Feature::Rotate),
    ];
    let feature = fmap
        .iter()
        .find(|(s, _)| s == &feature)
        .map(|(_, f)| *f)
        .unwrap_or_else(|| fail(&format!("未知 feature：{feature}")));

    let out_base = o.out.as_ref().map(|s| PathBuf::from(s.as_str()));
    let files_p: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(f)).collect();

    let plan = match feature {
        engine::Feature::ImgToVideo => {
            let imgs = if files_p.len() == 1
                && files_p[0].is_dir()
            {
                let mut v: Vec<PathBuf> = std::fs::read_dir(&files_p[0])
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.path())
                            .filter(|x| {
                                x.extension()
                                    .map(|e| {
                                        engine::IMAGE_EXTS.contains(
                                            &e.to_string_lossy().to_lowercase().as_str(),
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                v.sort();
                v
            } else {
                let mut v = files_p.clone();
                v.sort();
                v
            };
            if imgs.is_empty() {
                fail("img 需要图片文件（或整个图片文件夹）");
            }
            engine::build_args(feature, &imgs[0], &[], &imgs, &o.p, out_base.as_deref(), &tools)
        }
        engine::Feature::Concat => {
            let vids: Vec<PathBuf> = files_p
                .iter()
                .filter(|f| {
                    f.extension()
                        .map(|e| engine::VIDEO_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            if vids.len() < 2 {
                fail("concat 至少需要 2 个视频");
            }
            engine::build_args(feature, &vids[0], &vids, &[], &o.p, out_base.as_deref(), &tools)
        }
        _ => {
            let vids: Vec<PathBuf> = files_p
                .iter()
                .filter(|f| {
                    f.extension()
                        .map(|e| engine::VIDEO_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            if vids.is_empty() {
                fail("该 feature 需要视频文件");
            }
            let res = vids
                .iter()
                .map(|src| {
                    let plan =
                        engine::build_args(feature, src, std::slice::from_ref(src), &[], &o.p, out_base.as_deref(), &tools)?;
                    println!("【CLI】配方：{}", plan.note);
                    let total_us = engine::job_total_us(
                        feature,
                        src,
                        if feature == engine::Feature::Concat { &vids } else { &[] },
                        if feature == engine::Feature::ImgToVideo { &vids } else { &[] },
                        &o.p,
                        &tools,
                    );
                    let n = plan.cmds.len().max(1);
                    let seg = total_us / n as i64;
                    let mut ok_all = true;
                    for (i, cmd) in plan.cmds.iter().enumerate() {
                        let cancel_at = if i == 0 {
                            o.cancel_after
                                .map(|s| Instant::now() + Duration::from_secs_f64(s))
                                .unwrap_or_else(|| Instant::now() + Duration::from_secs(86400 * 30))
                        } else {
                            Instant::now() + Duration::from_secs(86400 * 30)
                        };
                        let (ok, err_text, cancelled) =
                            run_cmd(&ffmpeg_bin, cmd, total_us, seg, Some(cancel_at));
                        if cancelled {
                            println!("【CLI】已取消（q 优雅收尾 / kill 确认死透，半成品删除：{}）", plan.out.display());
                            if plan.is_dir {
                                let _ = std::fs::remove_dir_all(&plan.out);
                            } else {
                                let _ = std::fs::remove_file(&plan.out);
                            }
                            std::process::exit(3);
                        }
                        if !ok {
                            ok_all = false;
                            let e2 = engine::err_summary(&err_text, 400);
                            println!("【CLI】失败 {}：{}", src.display(), e2);
                        }
                    }
                    if ok_all {
                        let out_str = if plan.is_dir {
                            format!("目录 {}/", plan.out.display())
                        } else {
                            format!("{}", plan.out.display())
                        };
                        println!("【CLI】成功 {}：{out_str}", src.display());
                        Ok(())
                    } else {
                        Err("失败".to_string())
                    }
                })
                .collect::<Result<Vec<()>, String>>();
            match res {
                Ok(_) => {}
                Err(e) => fail(&e),
            }
            return;
        }
    };

    match plan {
        Ok(plan) => {
            println!("【CLI】配方：{}", plan.note);
            let cancel_at = o
                .cancel_after
                .map(|s| Instant::now() + Duration::from_secs_f64(s))
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(86400 * 30));
            let total_us = engine::job_total_us(
                feature,
                &files_p[0],
                &[],
                &files_p,
                &o.p,
                &tools,
            );
            let n = plan.cmds.len().max(1);
            let seg = total_us / n as i64;
            for (i, cmd) in plan.cmds.iter().enumerate() {
                let ca = if i == 0 {
                    cancel_at
                } else {
                    Instant::now() + Duration::from_secs(86400 * 30)
                };
                let (ok, err_text, cancelled) = run_cmd(&ffmpeg_bin, cmd, total_us, seg, Some(ca));
                if cancelled {
                    println!(
                        "【CLI】已取消（q 优雅收尾 / kill 确认死透，半成品删除：{}）",
                        plan.out.display()
                    );
                    if plan.is_dir {
                        let _ = std::fs::remove_dir_all(&plan.out);
                    } else {
                        let _ = std::fs::remove_file(&plan.out);
                    }
                    std::process::exit(3);
                }
                if !ok {
                    fail(&format!(
                        "{}：{}",
                        plan.note,
                        engine::err_summary(&err_text, 400)
                    ));
                }
            }
            println!("【CLI】成功 → {}", plan.out.display());
            if let Some(fc) = plan.need_frame_count.then(|| {
                std::fs::read_dir(&plan.out)
                    .map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".png")).count())
                    .unwrap_or(0)
            }) {
                println!("帧数：{fc}");
            }
        }
        Err(e) => fail(&e),
    }
}
