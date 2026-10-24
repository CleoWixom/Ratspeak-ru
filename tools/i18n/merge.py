#!/usr/bin/env python3
"""Сливает пакет переводов (JSON-объект {"English": "Русский"}) в locales/<lang>.json.
    python3 tools/i18n/merge.py ru tools/i18n/batches/ru_001.json
Ключи, которых нет в <lang>.json, сообщаются и пропускаются.
"""
import json, sys
from pathlib import Path
L = Path(__file__).resolve().parents[2] / "dashboard" / "static" / "locales"
lang, batch = sys.argv[1], Path(sys.argv[2])
tgt = L / f"{lang}.json"
d = json.loads(tgt.read_text(encoding="utf-8"))
b = json.loads(batch.read_text(encoding="utf-8"))
def resolve(k):
    if k in d: return k
    c = [x for x in d if x.startswith(k)]
    return c[0] if len(c) == 1 else None
miss = [k for k in b if resolve(k) is None]
for k, v in b.items():
    r = resolve(k)
    if r is not None: d[r] = v
tgt.write_text(json.dumps(d, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
print(f"влито: {len(b)-len(miss)}, не найдено ключей: {len(miss)} {miss[:5]}; всего переведено: {sum(1 for v in d.values() if v)}/{len(d)}")
