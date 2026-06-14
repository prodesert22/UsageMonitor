#!/usr/bin/env python3
"""Local quality gate for Usage Monitor.

Runs the same checks used in development. Tools that are not installed (e.g.
`ruff`, `qmllint`) are skipped with a notice rather than failing the run.

Usage:
    python scripts/check_quality.py
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class Result:
    def __init__(self) -> None:
        self.failed: list[str] = []
        self.skipped: list[str] = []


def run(result: Result, name: str, argv: list[str], *, tool: str | None = None) -> None:
    tool = tool or argv[0]
    if shutil.which(tool) is None:
        print(f"⊘ skip  {name} ({tool} not installed)")
        result.skipped.append(name)
        return
    print(f"▶ {name}: {' '.join(argv)}")
    proc = subprocess.run(argv, cwd=ROOT)
    if proc.returncode != 0:
        result.failed.append(name)
        print(f"✘ fail  {name}")
    else:
        print(f"✔ ok    {name}")


def qmllint(result: Result) -> None:
    tool = shutil.which("qmllint") or shutil.which("qmllint-qt6")
    if tool is None:
        print("⊘ skip  qmllint (not installed)")
        result.skipped.append("qmllint")
        return
    files = sorted((ROOT / "widgets/kde/package/contents").rglob("*.qml"))
    print(f"▶ qmllint: {len(files)} QML files")
    bad = [f for f in files if subprocess.run([tool, str(f)]).returncode != 0]
    if bad:
        result.failed.append("qmllint")
        print(f"✘ fail  qmllint ({len(bad)} files)")
    else:
        print("✔ ok    qmllint")


def main() -> int:
    result = Result()
    run(result, "rustfmt (cli)", ["cargo", "fmt", "-p", "usage-monitor-cli", "--check"], tool="cargo")
    run(result, "clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], tool="cargo")
    run(result, "cargo test", ["cargo", "test", "--workspace", "--quiet"], tool="cargo")
    run(result, "ruff", ["ruff", "check", "widgets", "scripts"])
    run(
        result,
        "widget tests",
        [sys.executable, "-m", "unittest", "discover", "-s", "widgets", "-p", "test_*.py"],
        tool=sys.executable,
    )
    qmllint(result)

    print("\n--- summary ---")
    if result.skipped:
        print("skipped:", ", ".join(result.skipped))
    if result.failed:
        print("FAILED:", ", ".join(result.failed))
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
