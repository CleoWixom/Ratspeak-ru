#!/usr/bin/env python3
"""Apply or verify the Ratspeak-ru hooks: the only edits the fork makes inside upstream files.

  fork/apply_hooks.py            apply every hook that is missing (safe to re-run)
  fork/apply_hooks.py --check    only verify; exit 1 if a hook is missing or cannot be applied
  fork/apply_hooks.py --root DIR work on another checkout (used by sync_upstream.py --dry-run)

Hooks are described in fork/hooks.json. Each one is located by a short text anchor, never by
line number, so unrelated upstream changes do not disturb it. If upstream rewrites the text a
hook is anchored to, the hook fails loudly and says which one, instead of silently dropping a
feature.
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
QUERY = re.compile(r"\?v=[^\"'<>\s]+")


class HookError(Exception):
    pass


def read(path: Path) -> tuple[str, str]:
    data = path.read_bytes().decode("utf-8")
    eol = "\r\n" if "\r\n" in data else "\n"
    return data.replace("\r\n", "\n"), eol


def write(path: Path, text: str, eol: str) -> None:
    path.write_bytes(text.replace("\n", eol).encode("utf-8"))


def hook_lines(hook: dict) -> list[str]:
    if "lines_file" in hook:
        text = (HERE / hook["lines_file"]).read_text(encoding="utf-8")
        return text.rstrip("\n").split("\n")
    return list(hook["lines"])


def find_anchor(hook: dict, lines: list[str]) -> int:
    hits = [i for i, line in enumerate(lines) if hook["anchor"] in line]
    if len(hits) != 1:
        raise HookError(
            f"{hook['id']}: anchor {hook['anchor']!r} matches {len(hits)} lines in {hook['file']} "
            "(expected exactly 1); upstream changed this spot, update the anchor in fork/hooks.json"
        )
    return hits[0]


def apply_insert(hook: dict, text: str, check: bool) -> tuple[str, bool]:
    if hook["marker"] in text:
        return text, False
    if check:
        raise HookError(f"{hook['id']}: missing in {hook['file']}")
    lines = text.split("\n")
    index = find_anchor(hook, lines)
    query = QUERY.search(lines[index])
    block = [line.replace("@@Q@@", query.group(0) if query else "") for line in hook_lines(hook)]
    offset = int(hook.get("offset", 0))
    at = index + 1 + offset if hook["type"] == "insert_after" else index
    lines[at:at] = block
    return "\n".join(lines), True


def apply_replace_block(hook: dict, text: str, check: bool) -> tuple[str, bool]:
    if hook["marker"] in text:
        return text, False
    if check:
        raise HookError(f"{hook['id']}: missing in {hook['file']}")
    old, new = "\n".join(hook["old"]), "\n".join(hook["new"])
    if text.count(old) != 1:
        raise HookError(
            f"{hook['id']}: the upstream text this hook replaces was not found exactly once in "
            f"{hook['file']}; upstream changed it, review fork/hooks.json"
        )
    return text.replace(old, new, 1), True


def apply_regex(hook: dict, root: Path, check: bool) -> list[Path]:
    changed = []
    pattern = re.compile(hook["pattern"])
    paths = sorted(Path(p) for p in glob.glob(str(root / hook["files"])))
    if not paths:
        raise HookError(f"{hook['id']}: no files match {hook['files']}")
    for path in paths:
        text, eol = read(path)
        if not pattern.search(text):
            continue
        if check:
            raise HookError(f"{hook['id']}: {path.relative_to(root)} still contains the upstream text")
        write(path, pattern.sub(hook["replacement"], text), eol)
        changed.append(path)
    return changed


def apply_api_meta(hook: dict, root: Path, check: bool) -> list[Path]:
    snapshot = (root / hook["snapshot"]).read_bytes()
    count = len(snapshot.decode("utf-8").rstrip("\n").split("\n"))
    digest = hashlib.sha256(snapshot).hexdigest()
    path = root / hook["file"]
    text, eol = read(path)
    start = text.find(f'"name": "{hook["package"]}"')
    if start < 0:
        raise HookError(f"{hook['id']}: package {hook['package']} not found in {hook['file']}")
    nxt = text.find('"name":', start + 1)
    end = len(text) if nxt < 0 else nxt
    chunk = text[start:end]
    new_chunk = re.sub(r'("itemCount":\s*)\d+', rf"\g<1>{count}", chunk, count=1)
    new_chunk = re.sub(r'("sha256":\s*")[0-9a-f]{64}', rf"\g<1>{digest}", new_chunk, count=1)
    if new_chunk == chunk:
        return []
    if check:
        raise HookError(f"{hook['id']}: item count/hash of {hook['package']} do not match its snapshot")
    write(path, text[:start] + new_chunk + text[end:], eol)
    return [path]


def run(root: Path, check: bool) -> tuple[list[str], list[str]]:
    config = json.loads((HERE / "hooks.json").read_text(encoding="utf-8"))
    changed: list[str] = []
    errors: list[str] = []
    for hook in config["hooks"]:
        try:
            kind = hook["type"]
            if kind in ("insert_after", "insert_before", "replace_block"):
                path = root / hook["file"]
                if not path.exists():
                    raise HookError(f"{hook['id']}: {hook['file']} does not exist")
                text, eol = read(path)
                step = apply_replace_block if kind == "replace_block" else apply_insert
                new_text, did = step(hook, text, check)
                if did:
                    write(path, new_text, eol)
                    changed.append(hook["file"])
            elif kind == "regex_replace":
                changed += [str(p.relative_to(root)) for p in apply_regex(hook, root, check)]
            elif kind == "api_meta":
                changed += [str(p.relative_to(root)) for p in apply_api_meta(hook, root, check)]
            else:
                raise HookError(f"{hook['id']}: unknown hook type {kind}")
        except HookError as error:
            errors.append(str(error))
    return changed, errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="verify only, change nothing")
    parser.add_argument("--root", default=str(HERE.parent), help="repository to work on")
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    changed, errors = run(root, args.check)
    for error in errors:
        print(f"fork hooks: {error}", file=sys.stderr)
    if errors:
        return 1
    if args.check:
        print("fork hooks: all present")
    else:
        files = sorted(set(changed))
        print(f"fork hooks: applied to {len(files)} file(s)" + (": " + ", ".join(files) if files else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
