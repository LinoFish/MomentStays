#!/usr/bin/env python3
"""Render source manuals using the existing GitHub Pages design.

Requires Python-Markdown (documentation tooling only). For example:
  python3 -m pip install --target .build/doc-tools 'Markdown>=3.7,<4'
  PYTHONPATH=.build/doc-tools python3 scripts/sync_website_guides.py \
      --output-dir .build/website-guides

Generated files can be applied to gh-pages without merging application code.
"""
import argparse
from datetime import date
from html import escape
from pathlib import Path
import re
import subprocess
from urllib.parse import urljoin
import xml.etree.ElementTree as ET

import markdown
from markdown.extensions.toc import slugify_unicode

ROOT = Path(__file__).resolve().parent.parent
BASE = "https://linofish.github.io/MomentStays/"
SOURCE = "https://github.com/LinoFish/MomentStays/blob/main/docs/"


def read_ref(ref, path):
    return subprocess.check_output(["git", "show", f"{ref}:{path}"], cwd=ROOT, text=True)


def render_manual(name):
    text = (ROOT / "docs" / name).read_text()
    body = markdown.markdown(text, extensions=["tables", "fenced_code", "toc", "sane_lists"],
        extension_configs={"toc": {"slugify": slugify_unicode}})

    def link(match):
        href = match.group(1)
        if href.startswith(("#", "https:", "http:", "mailto:")):
            return match.group(0)
        target = {"USER_GUIDE.md": "guide.html", "USER_GUIDE.zh-CN.md": "guide-zh.html"}.get(href)
        return 'href="' + escape(target or urljoin(SOURCE, href), quote=True) + '"'

    return re.sub(r'href="([^"]+)"', link, body)


def page(template, chinese):
    filename = "guide-zh.html" if chinese else "guide.html"
    other = "guide.html" if chinese else "guide-zh.html"
    name = "USER_GUIDE.zh-CN.md" if chinese else "USER_GUIDE.md"
    title = "MomentStays 使用手册" if chinese else "MomentStays User Guide"
    prefix, rest = template.split("<article>", 1)
    _, suffix = rest.rsplit("</article>", 1)
    prefix = re.sub(r'<html[^>]*>', '<html lang="zh-CN" class="lang-zh">' if chinese else '<html lang="en">', prefix, count=1)
    prefix = re.sub(r'<title>.*?</title>', f"<title>{title}</title>", prefix, count=1, flags=re.S)
    # Explicit URLs determine language, including links opened from the app.
    prefix = re.sub(r'<body>\s*<script>.*?</script>', '<body>', prefix, count=1, flags=re.S)
    switch = f'<a class="langbtn" href="{other}" lang="{"en" if chinese else "zh-CN"}">{"EN" if chinese else "中文"}</a>'
    prefix = re.sub(r'<(?:button|a) class="langbtn"[^>]*>.*?</(?:button|a)>', switch, prefix, count=1, flags=re.S)
    prefix = re.sub(r'<link rel="(?:canonical|alternate)"[^>]*>\s*', '', prefix)
    alternate = (f'<link rel="canonical" href="{BASE}{filename}">\n'
        f'<link rel="alternate" hreflang="en" href="{BASE}guide.html">\n'
        f'<link rel="alternate" hreflang="zh-CN" href="{BASE}guide-zh.html">\n')
    prefix = prefix.replace("</head>", alternate + "</head>")
    return prefix + f"<article>\n<!-- Generated from docs/{name} -->\n" + render_manual(name) + "\n</article>" + suffix


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--website-ref", default="origin/gh-pages")
    args = parser.parse_args()
    output = args.output_dir
    output.mkdir(parents=True, exist_ok=True)
    template = read_ref(args.website_ref, "guide.html")
    for chinese, filename in [(False, "guide.html"), (True, "guide-zh.html")]:
        (output / filename).write_text(page(template, chinese))
    for filename in ["index.html", "zh/index.html"]:
        text = read_ref(args.website_ref, filename)
        text = re.sub(r'(class="[^"]*\bzh\b[^"]*" href="(?:\.\./)?)guide\.html', r'\1guide-zh.html', text)
        if filename.startswith("zh/"):
            text = text.replace('"softwareHelp": "' + BASE + 'guide.html"', '"softwareHelp": "' + BASE + 'guide-zh.html"')
        path = output / filename
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    namespace = "http://www.sitemaps.org/schemas/sitemap/0.9"
    ET.register_namespace("", namespace)
    sitemap = ET.fromstring(read_ref(args.website_ref, "sitemap.xml"))
    for filename in ["guide.html", "guide-zh.html"]:
        entry = next((e for e in sitemap if e.findtext(f"{{{namespace}}}loc") == BASE + filename), None)
        if entry is None:
            entry = ET.SubElement(sitemap, f"{{{namespace}}}url")
            ET.SubElement(entry, f"{{{namespace}}}loc").text = BASE + filename
        modified = entry.find(f"{{{namespace}}}lastmod")
        if modified is None:
            modified = ET.SubElement(entry, f"{{{namespace}}}lastmod")
        modified.text = date.today().isoformat()
    ET.indent(sitemap, space="  ")
    (output / "sitemap.xml").write_bytes(ET.tostring(sitemap, encoding="utf-8", xml_declaration=True))
    print(f"Generated English/Chinese manuals, navigation and sitemap in {output}")


if __name__ == "__main__":
    main()
