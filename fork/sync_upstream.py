#!/usr/bin/env python3
"""Merge the official Ratspeak into this fork without hand-resolving conflicts.

  fork/sync_upstream.py --dry-run --run-tests   try it in a throw-away worktree first
  fork/sync_upstream.py --run-tests             merge upstream/main into the current branch
  fork/sync_upstream.py --ref v1.0.40           merge a tag or any other upstream ref

What it does:
  1. adds the `upstream` remote (ratspeak/Ratspeak) if needed and fetches it;
  2. merges the ref;
  3. for every conflicted file that carries a fork hook (see fork/hooks.json) it takes the
     upstream version and re-applies the hooks (fork/apply_hooks.py), which is what a person
     would do by hand;
  4. checks that every hook is present, optionally runs the dashboard tests, lists new strings
     that still need a Russian translation, and commits the merge.

Conflicts in any other file stop the sync and are listed: that means the fork edited a file it
should not (move that change into a fork-only file and add a hook) or upstream renamed one.
"""
from __future__ import annotations

import argparse
import glob
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
UPSTREAM_URL = "https://github.com/ratspeak/Ratspeak.git"


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(["git", *args], cwd=repo, capture_output=True, text=True)
    if check and result.returncode != 0:
        raise SystemExit(f"git {' '.join(args)} failed:\n{result.stderr or result.stdout}")
    return result


def hook_files(repo: Path) -> set[str]:
    config = json.loads((repo / "fork" / "hooks.json").read_text(encoding="utf-8"))
    files: set[str] = set()
    for hook in config["hooks"]:
        if "files" in hook:
            files.update(str(Path(p).relative_to(repo)) for p in glob.glob(str(repo / hook["files"])))
        if "file" in hook:
            files.add(hook["file"])
    return files


def conflicted(repo: Path) -> list[str]:
    out = git(repo, "diff", "--name-only", "--diff-filter=U").stdout
    return [line for line in out.splitlines() if line]


def apply_hooks(repo: Path, check: bool = False) -> subprocess.CompletedProcess:
    cmd = [sys.executable, str(repo / "fork" / "apply_hooks.py"), "--root", str(repo)]
    if check:
        cmd.append("--check")
    return subprocess.run(cmd, capture_output=True, text=True)


def run_tests(repo: Path) -> list[str]:
    failed = []
    for test in sorted(glob.glob(str(repo / "scripts" / "dashboard" / "test_*.js"))):
        if subprocess.run(["node", test], cwd=repo, capture_output=True).returncode != 0:
            failed.append(str(Path(test).relative_to(repo)))
    return failed


def new_translation_keys(repo: Path) -> list[str]:
    extract = repo / "tools" / "i18n" / "extract.py"
    if not extract.exists():
        return []
    before = set(json.loads((repo / "dashboard/static/locales/ru.json").read_text(encoding="utf-8")))
    subprocess.run([sys.executable, str(extract), "--lang", "ru"], cwd=repo, capture_output=True)
    after = json.loads((repo / "dashboard/static/locales/ru.json").read_text(encoding="utf-8"))
    return [key for key, value in after.items() if key not in before and not value]


def sync(repo: Path, ref: str, tests: bool, commit: bool) -> int:
    git(repo, "remote", "get-url", "upstream", check=False)
    if git(repo, "remote", "get-url", "upstream", check=False).returncode != 0:
        git(repo, "remote", "add", "upstream", UPSTREAM_URL)
    print("fetching upstream ...")
    git(repo, "fetch", "--tags", "upstream")
    target = ref if ref.startswith("upstream/") or git(repo, "rev-parse", "--verify", "-q", ref, check=False).returncode == 0 else f"upstream/{ref}"
    behind = git(repo, "rev-list", "--count", f"HEAD..{target}").stdout.strip()
    print(f"{target} is {behind} commit(s) ahead of this branch")
    if behind == "0":
        print("already up to date")
        return 0

    merge = git(repo, "merge", "--no-commit", "--no-ff", target, check=False)
    problems = conflicted(repo)
    managed = hook_files(repo)
    unresolved = [f for f in problems if f not in managed]
    if unresolved:
        print("\nconflicts outside the hook files; resolve by hand (merge left in progress):", file=sys.stderr)
        for name in unresolved:
            print(f"  {name}", file=sys.stderr)
        return 2
    for name in problems:
        theirs = git(repo, "checkout", "--theirs", "--", name, check=False)
        if theirs.returncode != 0:  # upstream deleted the file
            git(repo, "rm", "-f", "-q", "--", name, check=False)
        else:
            git(repo, "add", "--", name)
    if problems:
        print(f"resolved {len(problems)} conflicted hook file(s) by taking upstream and re-applying the hooks:")
        for name in problems:
            print(f"  {name}")

    applied = apply_hooks(repo)
    sys.stdout.write(applied.stdout)
    if applied.returncode != 0:
        sys.stderr.write(applied.stderr)
        print("\nsome hooks no longer apply; fix the named anchors in fork/hooks.json (merge left in progress)", file=sys.stderr)
        return 3
    git(repo, "add", "-A", "--", *sorted(managed))
    verified = apply_hooks(repo, check=True)
    if verified.returncode != 0:
        sys.stderr.write(verified.stderr)
        return 3

    keys = new_translation_keys(repo)
    if keys:
        git(repo, "add", "--", "dashboard/static/locales")
        print(f"\n{len(keys)} new interface string(s) need a Russian translation (tools/i18n/README.md):")
        for key in keys[:20]:
            print(f"  {key[:100]}")
        if len(keys) > 20:
            print(f"  ... and {len(keys) - 20} more")

    if tests:
        failed = run_tests(repo)
        if failed:
            print("\ndashboard tests failing after the merge:", file=sys.stderr)
            for name in failed:
                print(f"  {name}", file=sys.stderr)
            return 4
        print("\ndashboard tests: all passed")

    if commit:
        config = git(repo, "config", "user.name", check=False).stdout.strip()
        ident = [] if config else ["-c", "user.name=ratspeak-ru-sync", "-c", "user.email=noreply@example.invalid"]
        git(repo, *ident, "commit", "-q", "-m", f"Merge upstream {target} into Ratspeak-ru")
        print(f"\nmerged {target}: {git(repo, 'rev-parse', '--short', 'HEAD').stdout.strip()}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--ref", default="upstream/main", help="upstream branch, tag or commit (default: upstream/main)")
    parser.add_argument("--dry-run", action="store_true", help="merge in a temporary worktree and discard it")
    parser.add_argument("--run-tests", action="store_true", help="run scripts/dashboard/test_*.js after merging")
    parser.add_argument("--no-commit", action="store_true", help="leave the merged result staged")
    args = parser.parse_args(argv)

    if args.dry_run:
        work = Path(tempfile.mkdtemp(prefix="ratspeak-ru-sync-"))
        try:
            git(ROOT, "worktree", "add", "-q", "--detach", str(work), "HEAD")
            # use the checkout's own tooling, including uncommitted edits to it
            shutil.copytree(HERE, work / "fork", dirs_exist_ok=True)
            code = sync(work, args.ref, args.run_tests, commit=False)
            print("\n(dry run: nothing was changed in your checkout)")
            return code
        finally:
            git(ROOT, "worktree", "remove", "--force", str(work), check=False)
            shutil.rmtree(work, ignore_errors=True)

    if git(ROOT, "status", "--porcelain").stdout.strip():
        print("working tree is not clean; commit or stash first", file=sys.stderr)
        return 1
    return sync(ROOT, args.ref, args.run_tests, commit=not args.no_commit)


if __name__ == "__main__":
    sys.exit(main())
