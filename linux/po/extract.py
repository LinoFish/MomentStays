#!/usr/bin/env python3
"""Extract gettext message IDs from linux/src into po/momentstays.pot.

Equivalent to xgettext for this crate's call forms: gettext("..."),
ngettext("...", "...", n) and N_("...") with plain string literals.
Run from linux/: python3 po/extract.py
"""
import pathlib, re

SRC = pathlib.Path("src")
LIT = r'"((?:[^"\\]|\\.)*)"'
SINGLE = re.compile(r'\b(?:gettext|N_)\(\s*' + LIT, re.S)
PLURAL = re.compile(r'\bngettext\(\s*' + LIT + r'\s*,\s*' + LIT, re.S)

def main():
    entries = {}
    order = []
    for path in sorted(SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        # Skip unit tests: their strings are not shown in the UI.
        text = text.split("#[cfg(test)]\nmod tests")[0]
        for rx, plural in ((SINGLE, False), (PLURAL, True)):
            for m in rx.finditer(text):
                line = text.count("\n", 0, m.start()) + 1
                key = m.groups() if plural else (m.group(1),)
                if key not in entries:
                    entries[key] = []
                    order.append((path.as_posix(), line, key))
                entries[key].append(f"{path.as_posix()}:{line}")
    order.sort()
    out = ['msgid ""', 'msgstr ""', '"Project-Id-Version: momentstays\\n"',
           '"Content-Type: text/plain; charset=UTF-8\\n"', '"Content-Transfer-Encoding: 8bit\\n"', ""]
    for _, _, key in order:
        out.append("#: " + " ".join(entries[key]))
        if "{" in key[0]:
            out.append("#, python-brace-format")
        out.append(f'msgid "{key[0]}"')
        if len(key) == 2:
            out.append(f'msgid_plural "{key[1]}"')
            out += ['msgstr[0] ""', 'msgstr[1] ""']
        else:
            out.append('msgstr ""')
        out.append("")
    pathlib.Path("po/momentstays.pot").write_text("\n".join(out), encoding="utf-8")
    print(len(order), "messages")

if __name__ == "__main__":
    main()
