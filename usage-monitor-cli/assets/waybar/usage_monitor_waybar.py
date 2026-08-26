"""Waybar module payload for Usage Monitor.

It goes through the shared data helper rather than piping
`usage-monitor-cli widget waybar` straight out, for two reasons:

* **Cache.** A raw CLI call reports whatever happened in that one fetch, so a
  single rate-limited or expired-credential round turned the whole bar into "⚠"
  even though every number was fine a minute earlier. The helper merges the
  result with the last-good cache (the same one the popup uses), so a failing
  provider keeps its last value and the module is only marked `stale`.
* **Pacing.** The bar and the popup are separate processes with separate timers.
  The helper shares one fetched value between them for
  `minFetchIntervalSeconds` and takes an inter-process lock around the fetch, so
  the providers see one caller instead of two.

The output keeps the Waybar contract (`text`, `tooltip`, `class`, `percentage`)
and stays a superset of the CLI payload.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))


def find_binary() -> str | None:
    return (
        os.environ.get("USAGE_MONITOR_BIN")
        or shutil.which("usage-monitor-cli")
        or shutil.which("usage-monitor")
    )


def fallback(tooltip: str = "UsageMonitor unavailable") -> dict:
    return {
        "text": "—",
        "tooltip": tooltip,
        "class": "stale",
        "percentage": 0,
        "has_errors": True,
        "providers": [],
        "updated_at": "",
    }


def waybar_payload(summary: dict) -> dict:
    """Map the helper's summary onto the Waybar JSON contract."""
    providers = summary.get("providers") or []
    has_errors = any(bool(entry.get("error")) for entry in providers)
    stale = any(bool(entry.get("stale")) for entry in providers)
    payload = {
        "text": summary.get("text") or "—",
        "tooltip": summary.get("tooltip") or "Usage Monitor: no provider data",
        "class": summary.get("class") or "stale",
        "percentage": int(float(summary.get("percentage") or 0)),
        "has_errors": has_errors,
        "providers": providers,
        "updated_at": summary.get("updatedAt") or "",
    }
    if stale or summary.get("cached"):
        payload["cached"] = bool(summary.get("cached"))
    return payload


def run() -> str:
    if not find_binary():
        return json.dumps(fallback(
            "usage-monitor-cli not found. Set USAGE_MONITOR_BIN to its full path."
        ))
    try:
        import usage_monitor_waybar_data as data

        return json.dumps(waybar_payload(data.summary_payload()), ensure_ascii=False)
    except (OSError, ValueError, ImportError, subprocess.SubprocessError) as exc:
        return json.dumps(fallback(f"UsageMonitor error: {exc}"))


if __name__ == "__main__":
    print(run())
