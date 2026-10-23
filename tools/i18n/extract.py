#!/usr/bin/env python3
"""Извлекает видимые пользователю строки из dashboard (HTML + JS) в locales/en.json.

Использование:
    python3 tools/i18n/extract.py            # создать/обновить en.json и слить в ru.json
    python3 tools/i18n/extract.py --lang de  # заготовка для другого языка

Новые ключи добавляются в <lang>.json с пустым значением (""), уже переведённые не трогаются.
Ключ = исходная английская строка (точное совпадение), поэтому код приложения менять не нужно.
"""
import argparse, json, re, sys
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DASH = ROOT / "dashboard"
LOCALES = DASH / "static" / "locales"
SKIP_JS = {"vendor", "emoji_data.js", "legal_documents.js"}
ATTRS = ("placeholder", "title", "aria-label", "data-tooltip", "alt")

def looks_human(s: str) -> bool:
    s = s.strip()
    if len(s) < 2 or not re.search(r"[A-Za-z]{2,}", s):
        return False
    if re.search(r"[{}<>=;]|\|\||&&|\($|^\+ |^https?:|^[./#]|\w\.\w+\(|^[a-z]+(-[a-z0-9]+)+$|^[a-z_]+$|^[A-Z_]+$", s):
        return False
    return bool(re.match(r"[A-Z0-9(\"'“]", s)) or " " in s

class HtmlCollector(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.out, self.skip = [], 0
    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style", "svg", "code", "pre"):
            self.skip += 1
        for k, v in attrs:
            if k in ATTRS and v and looks_human(v):
                self.out.append(v.strip())
    def handle_endtag(self, tag):
        if tag in ("script", "style", "svg", "code", "pre") and self.skip:
            self.skip -= 1
    def handle_data(self, data):
        if not self.skip:
            t = re.sub(r"\s+", " ", data).strip()
            if looks_human(t):
                self.out.append(t)

STR = r"""(?:'((?:[^'\\\n]|\\.)*)'|"((?:[^"\\\n]|\\.)*)"|`((?:[^`\\$]|\\.)*)`)"""
JS_CTX = [
    re.compile(r"\.(?:textContent|innerText|placeholder|title|ariaLabel)\s*=\s*" + STR),
    re.compile(r"\.innerHTML\s*=\s*" + STR),
    re.compile(r"\b(?:showToast|toast|alert|confirm|showConfirm|showAlert)\(\s*" + STR),
    re.compile(r"setAttribute\(\s*['\"](?:title|aria-label|placeholder|data-tooltip)['\"]\s*,\s*" + STR),
    re.compile(r"\b(?:message|title|label|text|description|confirmText|cancelText|hint|error)\s*:\s*" + STR),
]

def js_strings(src: str):
    for rx in JS_CTX:
        for m in rx.finditer(src):
            s = next(g for g in m.groups() if g is not None)
            s = s.replace("\\'", "'").replace('\\"', '"').replace("\\n", " ")
            s = re.sub(r"\\u([0-9a-fA-F]{4})", lambda m: chr(int(m.group(1), 16)), s)
            s = re.sub(r"\s+", " ", s).strip()
            if "<" in s:  # HTML-фрагмент: берём только текстовые куски
                for t in re.split(r"<[^>]*>", s):
                    t = t.strip()
                    if looks_human(t):
                        yield t
            elif looks_human(s):
                yield s.strip()

def collect():
    found = {}
    def add(s, where):
        found.setdefault(s, set()).add(where)
    for f in DASH.glob("*.html"):
        c = HtmlCollector(); c.feed(f.read_text(encoding="utf-8"))
        for s in c.out: add(s, f.name)
    for f in sorted((DASH / "static" / "js").glob("*.js")):
        if f.name in SKIP_JS: continue
        for s in js_strings(f.read_text(encoding="utf-8")): add(s, f.name)
    loading = ROOT / "src-tauri" / "loading" / "index.html"
    if loading.exists():
        c = HtmlCollector(); c.feed(loading.read_text(encoding="utf-8"))
        for s in c.out: add(s, "loading/index.html")
    return found

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--lang", default="ru")
    a = ap.parse_args()
    LOCALES.mkdir(parents=True, exist_ok=True)
    found = collect()
    keys = sorted(found, key=str.lower)
    (LOCALES / "en.json").write_text(json.dumps({k: k for k in keys}, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    tgt = LOCALES / f"{a.lang}.json"
    old = json.loads(tgt.read_text(encoding="utf-8")) if tgt.exists() else {}
    new = {k: old.get(k, "") for k in keys}
    stale = [k for k in old if k not in found]
    tgt.write_text(json.dumps(new, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    done = sum(1 for v in new.values() if v)
    print(f"ключей: {len(keys)}, переведено: {done}, устаревших в {a.lang}.json: {len(stale)}")

if __name__ == "__main__":
    sys.exit(main())
