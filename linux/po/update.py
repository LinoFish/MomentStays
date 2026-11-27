#!/usr/bin/env python3
"""Regenerate po/momentstays.pot and po/{zh_CN,zh_TW}.po (run from linux/)."""
import pathlib, re, subprocess, sys
sys.dont_write_bytecode = True
sys.path.insert(0, "po")
subprocess.run([sys.executable, "po/extract.py"], check=True)
from translations import T

HEAD = {
    "zh_CN": ("Chinese (Simplified)", "zh_CN"),
    "zh_TW": ("Chinese (Traditional)", "zh_TW"),
}
pot = pathlib.Path("po/momentstays.pot").read_text(encoding="utf-8")
blocks = pot.strip().split("\n\n")
missing = []
for col, lang in enumerate(["zh_CN", "zh_TW"]):
    out = ['msgid ""', 'msgstr ""', '"Project-Id-Version: momentstays\\n"',
           '"PO-Revision-Date: 2026-11-28 00:00+0800\\n"',
           '"Last-Translator: Lino\\n"', f'"Language-Team: {HEAD[lang][0]}\\n"',
           f'"Language: {lang}\\n"', '"MIME-Version: 1.0\\n"',
           '"Content-Type: text/plain; charset=UTF-8\\n"', '"Content-Transfer-Encoding: 8bit\\n"',
           '"Plural-Forms: nplurals=1; plural=0;\\n"', ""]
    for block in blocks[1:]:
        lines = block.split("\n")
        ids = re.findall(r'^msgid(?:_plural)? "(.*)"$', block, re.M)
        key = tuple(ids) if len(ids) == 2 else ids[0]
        tr = T.get(key)
        if tr is None:
            missing.append(key)
            tr = ("", "")
        keep = [l for l in lines if not l.startswith("msgstr")]
        if len(ids) == 2:
            keep.append(f'msgstr[0] "{tr[col]}"')
        else:
            keep.append(f'msgstr "{tr[col]}"')
        out.append("\n".join(keep))
        out.append("")
    pathlib.Path(f"po/{lang}.po").write_text("\n".join(out), encoding="utf-8")
if missing:
    print("untranslated:", *sorted(set(map(str, missing))), sep="\n  ")
    sys.exit(1)
print("ok")
