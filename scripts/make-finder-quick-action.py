#!/usr/bin/env python3
"""make-finder-quick-action — 生成并安装 Finder Quick Action（右键视频/图片 → 用视频压缩工具打开）

装到 ~/Library/Services/用视频压缩工具打开.workflow（亦可以传第一条参数指定输出位置）。
bundle 结构照 Apple 自家 /System/Library/Services/*.workflow：
  Contents/Info.plist      NSServices 注册（runWorkflowAsService，限 Finder 上下文，送 public.movie/public.image）
  Contents/Resources/document.wflow  Automator Run Shell Script 动作（input as arguments）

App 侧（第二期已接好）：
  冷启动 → Rust setup 读 argv → take_launch_files → 前端入队
  App 已运行 → 直接 exec 起二进程，single-instance 插件转发 argv → vct://pending-files 事件 → 前端入队
"""
import os
import plistlib
import shutil
import sys
import uuid

NAME = "用视频压缩工具打开"
BIN = "/Applications/视频压缩工具.app/Contents/MacOS/video-compressor-tauri"

SHELL = """#!/bin/sh
# 用视频压缩工具打开选中的视频/图片文件（Finder Quick Action）
BIN="{bin}"
[ -x "$BIN" ] || exit 0
# 直接 exec 可执行文件：
#   App 未运行 → 呢个进程就係主实例，冷启动 argv 入队（take_launch_files）
#   App 已运行 → single-instance 插件把 argv 转发到运行中实例，呢个进程即刻退出
"$BIN" "$@" >/dev/null 2>&1 &
disown
exit 0
""".format(bin=BIN)


def main() -> None:
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
        "~/Library/Services/" + NAME + ".workflow"
    )
    if not os.path.exists(BIN):
        print(f"⚠️  搵唔到 {BIN}（先要装好 /Applications/视频压缩工具.app 先好使呢个 Quick Action）")
    c = os.path.join(out, "Contents")
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(os.path.join(c, "Resources"))

    info = {
        "CFBundleDevelopmentRegion": "zh_CN",
        "CFBundleIdentifier": "com.lele.vct.quickaction.open",
        "CFBundleName": NAME,
        "CFBundleShortVersionString": "1.0",
        "NSServices": [
            {
                "NSMenuItem": {"default": NAME},
                "NSMessage": "runWorkflowAsService",
                "NSRequiredContext": {"NSApplicationIdentifier": "com.apple.finder"},
                "NSSendFileTypes": ["public.movie", "public.image"],
            }
        ],
    }
    with open(os.path.join(c, "Info.plist"), "wb") as f:
        plistlib.dump(info, f)

    ver = {
        "BuildVersion": "1",
        "CFBundleShortVersionString": "1.0",
        "ProjectName": NAME,
        "SourceVersion": "1",
    }
    with open(os.path.join(c, "version.plist"), "wb") as f:
        plistlib.dump(ver, f)

    doc = {
        "actions": [
            {
                "isViewVisible": True,
                "action": {
                    "ActionBundlePath": "/System/Library/Automator/Run Shell Script.action",
                    "ActionName": "Run Shell Script",
                    "ActionParameters": {
                        "COMMAND_STRING": SHELL,
                        "inputMethod": 1,  # 1 = 输入「as arguments」（0 = stdin）
                        "shell": "/bin/sh",
                        "WORKDIR": "~/",
                        "CHARSETENC": "UTF8",
                    },
                    "AMAccepts": {
                        "Container": "List",
                        "Optional": True,
                        "Types": ["com.apple.cocoa.string"],
                    },
                    "AMProvides": {
                        "Container": "List",
                        "Types": ["com.apple.Automator.nothing"],
                    },
                    "AMActionVersion": "2.0.3",
                    "BundleIdentifier": "com.apple.RunShellScript",
                    "CFBundleVersion": "2.0.3",
                    "Class Name": "RunShellScriptAction",
                    "InputUUID": uuid.uuid4().hex,
                    "OutputUUID": uuid.uuid4().hex,
                    "UUID": uuid.uuid4().hex,
                    "ShowWhenRun": False,
                    "CanShowWhenRun": False,
                    "CanShowSelectedItemsWhenRun": False,
                    "location": "0.000000:0.000000",
                },
            }
        ],
        "AMApplicationBuild": "307",
        "AMApplicationVersion": "2.2",
        "AMDocumentVersion": "2",
        "connectors": {},
        "state": {},
        "workflowMetaData": {
            "serviceApplicationBundleID": "com.apple.finder",
            "serviceApplicationPath": "/System/Library/CoreServices/Finder.app",
            "serviceInputTypeIdentifier": "com.apple.Automator.fileSystemObject.all",
            "serviceOutputTypeIdentifier": "com.apple.Automator.nothing",
            "serviceProcessesInput": 1,
            "workflowTypeIdentifier": "com.apple.Automator.servicesMenu",
        },
    }
    with open(os.path.join(c, "Resources", "document.wflow"), "wb") as f:
        plistlib.dump(doc, f)

    print(f"✅ 已安装：{out}")
    print(f"用法：Finder 选中视频/图片文件 → 右键 → 快速操作/服务 → 「{NAME}」")
    print("如果右键菜单里没显示，去「系统设置 → 键盘 → 键盘快捷键 → 服务」确认「{n}」已勾选".format(n=NAME))


if __name__ == "__main__":
    main()
