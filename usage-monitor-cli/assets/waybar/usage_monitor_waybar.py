from __future__ import annotations

import json
import os
import shutil
import subprocess


def find_binary() -> str | None:
    return (
        os.environ.get("USAGE_MONITOR_BIN")
        or shutil.which("usage-monitor-cli")
        or shutil.which("usage-monitor")
    )


def fallback() -> dict:
    return {
        "text": "—",
        "tooltip": "UsageMonitor unavailable",
        "class": "stale",
        "percentage": 0,
        "has_errors": True,
        "providers": [],
        "updated_at": "",
    }


def run() -> str:
    binary = find_binary()
    if not binary:
        return json.dumps(fallback())
    try:
        proc = subprocess.run(
            [binary, "widget", "waybar"],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return json.dumps(fallback())
    if proc.returncode != 0:
        return json.dumps(fallback())
    line = proc.stdout.strip().splitlines()[0] if proc.stdout.strip() else ""
    try:
        json.loads(line)
    except json.JSONDecodeError:
        return json.dumps(fallback())
    return line
