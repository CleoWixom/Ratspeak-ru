#!/usr/bin/env python3
"""Собирает static/locales/<lang>.json в <lang>.js (window.RATSPEAK_I18N), пропуская пустые значения.
    python3 tools/i18n/build.py ru
"""
import json, sys
from pathlib import Path
L = Path(__file__).resolve().parents[2] / "dashboard" / "static" / "locales"
for lang in sys.argv[1:] or ["ru"]:
    d = {k: v for k, v in json.loads((L / f"{lang}.json").read_text(encoding="utf-8")).items() if v}
    (L / f"{lang}.js").write_text("window.RATSPEAK_I18N = " + json.dumps(d, ensure_ascii=False, indent=1) + ";\n", encoding="utf-8")
    print(f"{lang}: {len(d)} строк")
