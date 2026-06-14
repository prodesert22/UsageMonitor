import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import widgets.waybar.usage_monitor_waybar as waybar


class WaybarWrapperTests(unittest.TestCase):
    def test_fallback_when_binary_missing(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            with mock.patch.object(waybar.shutil, "which", return_value=None):
                self.assertIsNone(waybar.find_binary())

    def test_fallback_payload_shape(self):
        payload = waybar.fallback()
        self.assertEqual(set(payload), {"text", "tooltip", "class", "percentage", "has_errors", "providers", "updated_at"})

    def test_run_returns_cli_single_line_json(self):
        with mock.patch.dict(os.environ, {"USAGE_MONITOR_BIN": "/bin/usage-monitor-cli"}, clear=True):
            proc = mock.Mock(returncode=0, stdout='{"text":"1%","class":"ok"}\n', stderr="")
            with mock.patch.object(waybar.subprocess, "run", return_value=proc) as run:
                self.assertEqual(waybar.run(), '{"text":"1%","class":"ok"}')
                self.assertEqual(run.call_args.args[0], ["/bin/usage-monitor-cli", "widget", "waybar"])

    def test_run_falls_back_on_invalid_json(self):
        with mock.patch.dict(os.environ, {"USAGE_MONITOR_BIN": "/bin/usage-monitor-cli"}, clear=True):
            proc = mock.Mock(returncode=0, stdout='not json\n', stderr="")
            with mock.patch.object(waybar.subprocess, "run", return_value=proc):
                payload = json.loads(waybar.run())
                self.assertEqual(payload["class"], "stale")

    def test_run_falls_back_on_subprocess_exception(self):
        with mock.patch.dict(os.environ, {"USAGE_MONITOR_BIN": "/bin/usage-monitor-cli"}, clear=True):
            with mock.patch.object(waybar.subprocess, "run", side_effect=subprocess.TimeoutExpired("cmd", 1)):
                payload = json.loads(waybar.run())
                self.assertEqual(payload["class"], "stale")

    def test_executable_runs_from_non_repo_cwd(self):
        script = Path(__file__).resolve().parents[1] / "usage-monitor-waybar"
        with tempfile.TemporaryDirectory() as td:
            proc = subprocess.run(
                [sys.executable, str(script)],
                cwd=td,
                env={"USAGE_MONITOR_BIN": "/bin/true"},
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        payload = json.loads(proc.stdout)
        self.assertEqual(payload["class"], "stale")


if __name__ == "__main__":
    unittest.main()
