#!/usr/bin/env python3
"""Вливает переводы шаблонов в locales/<lang>.patterns.json.
Ключ пакета — точный шаблон или его однозначное начало (для длинных строк).
    python3 tools/i18n/merge_patterns.py ru tools/i18n/batches/ru_pat_001.json
В значениях: {0} — подстановка; {0:форма1|форма2|форма5} — русский плюрал по числу из {0}.
"""
import json, sys
from pathlib import Path
L = Path(__file__).resolve().parents[2] / "dashboard" / "static" / "locales"
lang, batch = sys.argv[1], Path(sys.argv[2])
tgt = L / f"{lang}.patterns.json"
d = json.loads(tgt.read_text(encoding="utf-8")); b = json.loads(batch.read_text(encoding="utf-8"))
bad = []
for k, v in b.items():
    c = [x for x in d if x == k] or [x for x in d if x.startswith(k)]
    if len(c) == 1: d[c[0]] = v
    else: bad.append((k[:60], len(c)))
tgt.write_text(json.dumps(d, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
print(f"всего: {len(d)}, переведено: {sum(1 for v in d.values() if v)}, проблемные: {bad}")
