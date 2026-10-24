#!/usr/bin/env python3
"""Извлекает динамические строки (конкатенация 'a ' + x + ' b' и `шаблоны ${x}`) как шаблоны с {0},{1}...
в locales/<lang>.patterns.json. Новые шаблоны получают пустое значение, переведённые не трогаются.
    python3 tools/i18n/extract_patterns.py [--lang ru]
"""
import argparse, json, re
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
JS = ROOT / "dashboard" / "static" / "js"
LOC = ROOT / "dashboard" / "static" / "locales"
SKIP = {"emoji_data.js", "legal_documents.js"}
START = re.compile(r"(?:\.(?:textContent|innerText|placeholder|title)\s*=|\b(?:showToast|toast|alert|confirm|rsConfirm|showConfirm)\(|\b(?:message|title|label|text|description|hint|error|confirmText)\s*:|setAttribute\(\s*['\"](?:title|aria-label|placeholder)['\"]\s*,)\s*")
LIT = re.compile(r"""'((?:[^'\\\n]|\\.)*)'|"((?:[^"\\\n]|\\.)*)\"""")

def unesc(s):
    s = s.replace("\\'", "'").replace('\\"', '"').replace("\\n", " ")
    return re.sub(r"\\u([0-9a-fA-F]{4})", lambda m: chr(int(m.group(1), 16)), s)

def take_expr(src, i):
    """Читает выражение от i до верхнеуровневого ; , ) } — возвращает текст."""
    depth, j, q = 0, i, None
    while j < len(src):
        c = src[j]
        if q:
            if c == "\\": j += 2; continue
            if c == q: q = None
        elif c in "'\"`": q = c
        elif c in "([{": depth += 1
        elif c in ")]}":
            if depth == 0: break
            depth -= 1
        elif c in ";," and depth == 0: break
        j += 1
    return src[i:j]

def split_plus(expr):
    parts, depth, q, cur, j = [], 0, None, "", 0
    while j < len(expr):
        c = expr[j]
        if q:
            cur += c
            if c == "\\": cur += expr[j+1]; j += 2; continue
            if c == q: q = None
        elif c in "'\"`": q = c; cur += c
        elif c in "([{": depth += 1; cur += c
        elif c in ")]}": depth -= 1; cur += c
        elif c == "+" and depth == 0: parts.append(cur.strip()); cur = ""
        else: cur += c
        j += 1
    parts.append(cur.strip())
    return parts

def human(s):
    t = re.sub(r"\{\d+\}", "", s)
    return len(re.findall(r"[A-Za-z]{3,}", t)) >= 1 and not re.search(r"[<>=;]|^https?:", s)

def pattern_from(expr):
    expr = expr.strip()
    if expr.startswith("`") and expr.endswith("`") and "${" in expr:
        n = [0]
        def sub(m): n[0] += 1; return "{%d}" % (n[0]-1)
        body = re.sub(r"\$\{[^}]*\}", sub, expr[1:-1])
        return re.sub(r"\s+", " ", body).strip() if "<" not in body else None
    parts = split_plus(expr)
    if len(parts) < 2: return None
    out, n, lit = [], 0, 0
    for p in parts:
        m = re.fullmatch(r"'((?:[^'\\\n]|\\.)*)'|\"((?:[^\"\\\n]|\\.)*)\"", p, re.S)
        if m: out.append(unesc(m.group(1) if m.group(1) is not None else m.group(2))); lit += 1
        else: out.append("{%d}" % n); n += 1
    if lit == 0 or (n == 0 and lit < 2): return None
    return re.sub(r"\s+", " ", "".join(out)).strip()

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--lang", default="ru"); a = ap.parse_args()
    found = {}
    for f in sorted(JS.glob("*.js")):
        if f.name in SKIP: continue
        src = f.read_text(encoding="utf-8")
        for m in START.finditer(src):
            p = pattern_from(take_expr(src, m.end()))
            if p and human(p) and not re.fullmatch(r"(\{\d+\}\s*)+", p): found[p] = f.name
    tgt = LOC / f"{a.lang}.patterns.json"
    old = json.loads(tgt.read_text(encoding="utf-8")) if tgt.exists() else {}
    new = {k: old.get(k, "") for k in sorted(found, key=str.lower)}
    new.update({k: v for k, v in old.items() if k not in found and v})  # ручные дополнения сохраняются
    tgt.write_text(json.dumps(new, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"шаблонов: {len(new)}, переведено: {sum(1 for v in new.values() if v)}")

if __name__ == "__main__": main()
