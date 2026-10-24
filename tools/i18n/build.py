#!/usr/bin/env python3
"""Собирает static/locales/<lang>.json (+ <lang>.patterns.json) в <lang>.js:
window.RATSPEAK_I18N (точные строки) и window.RATSPEAK_I18N_PATTERNS ([[regex, значение], ...]).
    python3 tools/i18n/build.py ru
"""
import json, re, sys
from pathlib import Path
L = Path(__file__).resolve().parents[2] / "dashboard" / "static" / "locales"
PH = re.compile(r"\{\d+\}")

def to_regex(key: str) -> str:
    out, pos = "^", 0
    for m in PH.finditer(key):
        out += re.escape(key[pos:m.start()]) + "([\\s\\S]*?)"
        pos = m.end()
    return out + re.escape(key[pos:]) + "$"

for lang in sys.argv[1:] or ["ru"]:
    exact = {k: v for k, v in json.loads((L / f"{lang}.json").read_text(encoding="utf-8")).items() if v}
    pats = []
    pf = L / f"{lang}.patterns.json"
    if pf.exists():
        for k, v in json.loads(pf.read_text(encoding="utf-8")).items():
            if not v: continue
            if PH.search(k): pats.append((k, v))
            else: exact.setdefault(re.sub(r"\s+", " ", k), v)
    # длинные (более специфичные) шаблоны проверяются первыми
    pats.sort(key=lambda kv: -len(PH.sub("", kv[0])))
    plist = [[to_regex(k), v] for k, v in pats]
    js = "window.RATSPEAK_I18N = " + json.dumps(exact, ensure_ascii=False, indent=1) + ";\n"
    js += "window.RATSPEAK_I18N_PATTERNS = " + json.dumps(plist, ensure_ascii=False, indent=1) + ";\n"
    (L / f"{lang}.js").write_text(js, encoding="utf-8")
    print(f"{lang}: {len(exact)} строк, {len(plist)} шаблонов")
