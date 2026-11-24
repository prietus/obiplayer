#!/usr/bin/env python3
"""Extract translatable UI strings from crates/obiplayer/src into po/obiplayer.pot.

Strings are the English literals passed to `tr("…")` plus the helpers that
call `tr` internally (icon buttons, info-grid keys, headings…).
"""
import pathlib, re

SRC = pathlib.Path(__file__).resolve().parent.parent / "crates/obiplayer/src"
LIT = r'"((?:[^"\\]|\\.)+)"'
PATTERNS = [
    r'\btr\(\s*' + LIT,
    r'icon_button\(\s*"[^"]*",\s*' + LIT,                     # (icon, tip)
    r'round_button\(\s*"[^"]*",\s*' + LIT,
    r'\b(?:text|copyable|widget)\(\s*' + LIT,                  # InfoGrid keys
    r'\bbutton\(\s*"[^"]*",\s*' + LIT + r',\s*Transport',      # bar buttons
    r'\btbtn\(\s*"[^"]*",\s*' + LIT,
    r'\btoggle\(\s*"[^"]*",\s*' + LIT,
    r'"(Oscilloscope|Spectrum|Circular|Week|Month|Year|All Time|Classic|Warm|Mono|Vinyl|Edition|Year|DR|Min|Max|Codec|Source|Mon|Tue|Wed|Thu|Fri|Sat|Sun|artists|labels|composers|tracks|albums|plays|Search artists…|Search labels…|Search composers…|Title|Artist|Album|Label|Composer|Dur\.)"',
]
found = {}
for f in sorted(SRC.rglob("*.rs")):
    if f.name.startswith("._"):  # macOS AppleDouble leftovers from tar
        continue
    text = f.read_text()
    for p in PATTERNS:
        for m in re.finditer(p, text):
            s = m.group(1)
            if s.strip() and not s.startswith(("{", "/")) and not re.fullmatch(r"[a-z0-9_-]+-symbolic", s):
                found.setdefault(s, f.name)
out = ['msgid ""', 'msgstr ""', '"Content-Type: text/plain; charset=UTF-8\\n"', ""]
for s, src in sorted(found.items(), key=lambda x: x[0].lower()):
    out += [f"#: {src}", f'msgid "{s}"', 'msgstr ""', ""]
(SRC.parent.parent.parent / "po/obiplayer.pot").write_text("\n".join(out))
print(len(found), "strings")
