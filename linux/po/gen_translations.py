#!/usr/bin/env python3
"""Maintainer helper: regenerate po/translations.py for the current msgids.

zh-Hans comes from the macOS app's catalog (apple/macos/packaging/
Workbench.en.json, Chinese source → English) so both apps use the same
wording; strings only the GNOME app has are in MANUAL. zh-Hant keeps the
existing translation, else is converted from zh-Hans with OpenCC (s2twp)
and the product terms used throughout the catalog. Run from linux/ after
`python3 po/extract.py`; needs `pip install opencc-python-reimplemented`.
"""
import json, re, sys, pathlib
sys.dont_write_bytecode = True
sys.path.insert(0, "po")
from translations import T
from opencc import OpenCC

MANUAL = {
    "Apple Live Photos → ": "苹果实况 → ",
    "Complete workflow in five steps": "完整操作流程，共五步",
    "Finish the earlier steps to open {step}": "完成前面的步骤后进入{step}",
    "View {step}, keeping the current selection and settings": "查看{step}，保留当前选择和设置",
    "Add folders of photos; subfolders are included.": "添加照片文件夹；包含子文件夹。",
    "If a motion container or HDR resource cannot be fully verified, it is not published or split into a still; see Export Preview for the reason.": "动态容器或 HDR 资源无法完整验证时不发布，也不会拆成静态图；请在导出预览查看原因。",
    "Verify and Reuse checks sources, scope, settings, and output files, and continues without reprocessing when they match.": "校验并复用会核对来源、范围、设置和输出文件，匹配时继续且不重新处理。",
    "Processed {done} / {total}": "已处理 {done} / {total}",
    "Converted {converted} · Unchanged {copied} · Existing {resumed} · Skipped {skipped} · Failed {failed}": "已转换 {converted} · 原样 {copied} · 已有 {resumed} · 跳过 {skipped} · 失败 {failed}",
    "Unchanged export: new {copied} · reused {resumed} · skipped {skipped} · failed {failed}": "原样导出：新增 {copied} · 复用 {resumed} · 跳过 {skipped} · 失败 {failed}",
    "File generation and unchanged exports are recorded separately; the report in the output folder lists every photo.": "文件生成与原样输出分别记录；输出文件夹中的报告列出每张照片。",
    "File generation and unchanged exports are recorded separately. Click a photo for its actual result and reason.": "文件生成与原样输出分别记录；点击照片查看实际结果与原因。",
    "Result": "结果",
    "High-resolution cover selected: candidate {index}": "已选择高清候选封面：第 {index} 个",
    "Show or hide the photo inspector (Ctrl+Alt+P)": "展开或收起照片检查器（Ctrl+Alt+P）",
    "Show or Hide the Photo Inspector": "展开或收起照片检查器",
    "Inspect burst groups: {groups} groups · {frames} frames": "检查连拍组：{groups} 组 · {frames} 帧",
    "Click a photo to inspect it. Use the check mark or Ctrl-click to include it. Filtering and preview do not change the processing scope": "点击照片查看特性与处理计划；用勾选标记或 Ctrl 点击纳入本轮。筛选与预览不改变处理范围",
    "Changing the cover does not affect the motion video. New covers are JPEG and do not preserve the original cover's HDR.": "更换封面不影响动态视频。新封面为 JPEG，不保留原封面的 HDR。",
    "Next Step": "下一步",
    "Workflow": "操作步骤",
    "Static HDR": "静态 HDR",
    "Existing Output": "已有输出时",
    "Write Report Files": "写入报告文件",
    "When a Result Exists": "已有输出时",
    "Save the manifest and JSON report next to the results.": "在结果旁保存清单和 JSON 报告。",
    "Local JPEG/HEIC motion photos, supported HDR, and bursts. Ordinary stills can be processed for dates only or exported unchanged in the next step.": "本地 JPEG / HEIC 实况、受支持的 HDR 与连拍；普通静态照片可在下一步仅处理时间或原样导出。",
    "Import complete Apple Live Photos (image and adjacent MOV). Static HDR follows the supported processing path.": "导入完整 Apple 实况照片（图片及同位置 MOV）；静态 HDR 按实际已支持能力处理。",
    "Verified Google Motion Photo JPEG → HONOR only, from local files. HEIC is unsupported.": "仅支持已验证的 Google Motion Photo JPEG → 荣耀；从本地文件导入，不支持 HEIC。",
    "Recognized HONOR Motion Photo JPEG → Google only, from local files. HEIC is unsupported.": "仅支持已识别的荣耀实况 JPEG → Google；从本地文件导入，不支持 HEIC。",
    "Add Source Folder": "添加来源文件夹",
    "Choose an output folder outside the source folders.": "请选择来源文件夹之外的输出文件夹。",
    "Results are in “{folder}”.": "结果已保存到“{folder}”。",
    "Cover {seconds} s": "封面 {seconds} 秒",
    "Choose Output Folder": "选择输出文件夹",
    "Remove Folder": "移除文件夹",
    "Watch for Changes": "监测变化",
    "No Matching Photos": "没有匹配的照片",
    "Try another filter or search.": "请尝试其他筛选或搜索。",
    "OK": "好",
}
TW_TERMS = [("實況", "原況"), ("安卓", "Android"), ("蘋果", "Apple"), ("圖庫", "圖庫")]

wb = json.load(open("../apple/macos/packaging/Workbench.en.json", encoding="utf-8"))["strings"]
rev = {}
for zh, en in wb.items():
    rev.setdefault(en, zh)

def from_wb(msgid):
    names = re.findall(r"\{([a-z]+)\}", msgid)
    key = msgid
    for i, n in enumerate(names):
        key = key.replace("{" + n + "}", "{%d}" % i, 1)
    zh = rev.get(key)
    if zh is None:
        return None
    for i, n in enumerate(names):
        zh = zh.replace("{%d}" % i, "{" + n + "}")
    return zh

pot = pathlib.Path("po/momentstays.pot").read_text(encoding="utf-8")
ids = re.findall(r'^msgid "(.*)"$', pot, re.M)[1:]
cc = OpenCC("s2twp")
out, missing = {}, []
for i in ids:
    plain = i.replace('\\"', '"')
    zh = MANUAL.get(plain) or from_wb(plain)
    old = T.get(i)
    if zh is None and old:
        out[i] = old
        continue
    if zh is None:
        missing.append(i)
        continue
    if old and old[0] == zh:
        out[i] = old
        continue
    tw = cc.convert(zh)
    for a, b in TW_TERMS:
        tw = tw.replace(a, b)
    out[i] = (zh, tw)
if missing:
    print("missing:", *missing, sep="\n  ")
    sys.exit(1)
esc = lambda s: s.replace("\\", "\\\\").replace('"', '\\"')
lines = ["# zh-Hans (zh_CN) and zh-Hant (zh_TW) translations, generated into *.po by",
         "# po/update.py. zh-Hans follows the macOS app's wording (see po/gen_translations.py).",
         "T = {"]
for k, (a, b) in out.items():
    lines.append(f'"{k}":\n ("{esc(a)}", "{esc(b)}"),')
lines.append("}")
pathlib.Path("po/translations.py").write_text("\n".join(lines) + "\n", encoding="utf-8")
print(len(out), "translations")
