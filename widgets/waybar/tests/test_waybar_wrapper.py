"""Tests for the Waybar bar module.

The module used to pipe `usage-monitor-cli widget waybar` straight through, so a
single failed fetch (an expired token, a rate-limited endpoint) replaced the
percentage with "⚠". It now goes through the shared data helper, which merges
the fetch with the last-good cache and paces the calls; these tests pin that
behaviour down.

Every test points XDG_CACHE_HOME/XDG_CONFIG_HOME at a tempdir — the helper reads
real state otherwise.
"""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# The waybar helper lives in the CLI asset tree (single source of truth,
# embedded by the installer); load it directly from there.
_ASSET_DIR = Path(__file__).resolve().parents[3] / "usage-monitor-cli" / "assets" / "waybar"


def _load(name: str):
    spec = importlib.util.spec_from_file_location(name, _ASSET_DIR / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


data = _load("usage_monitor_waybar_data")
waybar = _load("usage_monitor_waybar")


def _env(td: str) -> dict[str, str]:
    return {
        "XDG_CONFIG_HOME": f"{td}/config",
        "XDG_CACHE_HOME": f"{td}/cache",
        "HOME": td,
        "XDG_DATA_DIRS": td,
        "USAGE_MONITOR_BIN": "/bin/usage-monitor-cli",
        "USAGE_MONITOR_DARK": "1",
    }


def _cli_payload(percent: int) -> dict:
    return {"providers": [{
        "provider_id": "codex",
        "display_name": "Codex",
        "max_percentage": percent,
        "windows": [{"id": "primary", "percentage": percent, "resets_at": "in 2h"}],
    }]}


class WaybarWrapperTests(unittest.TestCase):
    def test_fallback_when_binary_missing(self):
        with mock.patch.dict(os.environ, {}, clear=True), \
             mock.patch.object(waybar.shutil, "which", return_value=None):
            self.assertIsNone(waybar.find_binary())
            payload = json.loads(waybar.run())
        self.assertEqual(payload["class"], "stale")
        self.assertIn("USAGE_MONITOR_BIN", payload["tooltip"])

    def test_fallback_payload_shape(self):
        payload = waybar.fallback()
        self.assertEqual(
            set(payload),
            {"text", "tooltip", "class", "percentage", "has_errors", "providers", "updated_at"},
        )

    def test_run_reports_the_fetched_percentage(self):
        proc = mock.Mock(returncode=0, stdout=json.dumps(_cli_payload(42)), stderr="")
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, _env(td), clear=True), \
             mock.patch.object(data.subprocess, "run", return_value=proc):
            payload = json.loads(waybar.run())
        self.assertEqual(payload["text"], "42%")
        self.assertEqual(payload["class"], "ok")
        self.assertEqual(payload["percentage"], 42)
        self.assertFalse(payload["has_errors"])

    def test_failed_fetch_keeps_the_last_good_value_instead_of_the_warning_glyph(self):
        ok = mock.Mock(returncode=0, stdout=json.dumps(_cli_payload(44)), stderr="")
        broken = mock.Mock(returncode=1, stdout="", stderr="Rate limited by 'claude'")
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, _env(td), clear=True):
            with mock.patch.object(data.subprocess, "run", return_value=ok):
                json.loads(waybar.run())
            data.set_state_key("minFetchIntervalSeconds", "0")  # no pacing, fetch again
            with mock.patch.object(data.subprocess, "run", return_value=broken):
                payload = json.loads(waybar.run())
        self.assertEqual(payload["text"], "44%")
        # Cached values keep their usage class; below the warning threshold that
        # is "stale", which is what the CSS greys out.
        self.assertEqual(payload["class"], "stale")
        self.assertTrue(payload["has_errors"])

    def test_second_run_inside_the_interval_does_not_call_the_cli(self):
        proc = mock.Mock(returncode=0, stdout=json.dumps(_cli_payload(12)), stderr="")
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, _env(td), clear=True), \
             mock.patch.object(data.subprocess, "run", return_value=proc) as run:
            json.loads(waybar.run())
            calls_after_first = run.call_count
            payload = json.loads(waybar.run())
        self.assertEqual(run.call_count, calls_after_first)
        self.assertTrue(payload["cached"])
        self.assertEqual(payload["text"], "12%")

    def test_invalid_cli_json_degrades_to_an_error_entry(self):
        proc = mock.Mock(returncode=0, stdout="not json\n", stderr="")
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, _env(td), clear=True), \
             mock.patch.object(data.subprocess, "run", return_value=proc):
            payload = json.loads(waybar.run())
        self.assertEqual(payload["class"], "stale")
        self.assertTrue(payload["has_errors"])

    def test_cli_timeout_degrades_to_an_error_entry(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, _env(td), clear=True), \
             mock.patch.object(data.subprocess, "run", side_effect=subprocess.TimeoutExpired("cmd", 1)):
            payload = json.loads(waybar.run())
        self.assertEqual(payload["class"], "stale")

    def test_waybar_payload_keeps_the_module_contract(self):
        summary = {
            "text": "50%", "tooltip": "tip", "class": "ok", "percentage": 50.4,
            "providers": [{"provider": "codex", "stale": True}], "updatedAt": "now", "cached": True,
        }
        payload = waybar.waybar_payload(summary)
        self.assertEqual(payload["percentage"], 50)  # Waybar wants an integer
        self.assertEqual(payload["text"], "50%")
        self.assertEqual(payload["updated_at"], "now")
        self.assertTrue(payload["cached"])
        self.assertFalse(payload["has_errors"])

    def test_executable_runs_from_non_repo_cwd(self):
        script = _ASSET_DIR / "usage-monitor-waybar"
        with tempfile.TemporaryDirectory() as td:
            proc = subprocess.run(
                [sys.executable, str(script)],
                cwd=td,
                env={**_env(td), "USAGE_MONITOR_BIN": "/bin/true"},
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        payload = json.loads(proc.stdout)
        # /bin/true prints nothing: no providers, no cache, so the module says so.
        self.assertEqual(payload["class"], "stale")


if __name__ == "__main__":
    unittest.main()
