"""Smoke test for ThemePalette.qml against a real QML engine.

The palette resolves "follow the desktop theme" through the attached
Kirigami.Theme, which only behaves on some root types: making the component an
Item once turned every colour into #000000 and painted the whole widget black.
Nothing in the Python/qmllint gate catches that, so this test instantiates the
component for real and checks the colours it hands to the widget.

Skipped when the machine has no QML runtime or no Kirigami/Plasma QML modules
(CI containers, minimal build environments).
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

_UI = (
    Path(__file__).resolve().parents[3]
    / "usage-monitor-cli"
    / "assets"
    / "kde"
    / "package"
    / "contents"
    / "ui"
)

_QML_BINARIES = ("/usr/lib/qt6/bin/qml", "qml6", "qml")

_HARNESS = """
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import "%(ui)s"

QQC2.ApplicationWindow {
    visible: true
    width: 100
    height: 100

    ThemePalette {
        id: plasmaPalette
        spec: ({"mode": "plasma", "colors": ({}), "font": ({}), "metrics": ({"opacity": 0.3})})
    }

    ThemePalette {
        id: themedPalette
        spec: ({"mode": "builtin", "dark": true,
                "colors": {"background": "#2e3440", "text": "#eceff4", "subtext": "#81a1c1",
                           "accent": "#88c0d0", "warning": "#ebcb8b", "critical": "#bf616a",
                           "track": "#3b4252", "border": "#4c566a"},
                "font": ({}), "metrics": ({"radius": 10})})
    }

    Component.onCompleted: {
        console.log("RESULT " + JSON.stringify({
            "plasma": {
                "themed": plasmaPalette.themed,
                "translucent": plasmaPalette.translucent,
                "background": String(plasmaPalette.backgroundColor),
                "text": String(plasmaPalette.textColor),
                "highlight": String(plasmaPalette.highlightColor),
                "subtext": String(plasmaPalette.subtextColor)
            },
            "kirigami": {
                "background": String(Kirigami.Theme.backgroundColor),
                "text": String(Kirigami.Theme.textColor),
                "highlight": String(Kirigami.Theme.highlightColor)
            },
            "themed": {
                "background": String(themedPalette.backgroundColor),
                "critical": String(themedPalette.levelColor(95)),
                "radius": themedPalette.cornerRadius
            }
        }))
        Qt.exit(0)
    }
}
"""


def _qml_binary() -> str | None:
    for candidate in _QML_BINARIES:
        found = candidate if os.path.isabs(candidate) and os.path.exists(candidate) else shutil.which(candidate)
        if found:
            return found
    return None


def _run_harness() -> dict | None:
    binary = _qml_binary()
    if not binary:
        return None
    with tempfile.TemporaryDirectory() as td:
        harness = Path(td) / "harness.qml"
        harness.write_text(_HARNESS % {"ui": _UI.as_uri()}, encoding="utf-8")
        env = {
            **os.environ,
            "QT_QPA_PLATFORM": "offscreen",
            "QT_FORCE_STDERR_LOGGING": "1",
            # Same controls style Plasma uses, so the palette resolves the way it
            # does in the shell.
            "QT_QUICK_CONTROLS_STYLE": "org.kde.desktop",
            # A stale compiled copy would hide the very regression under test.
            "QML_DISABLE_DISK_CACHE": "1",
        }
        try:
            proc = subprocess.run(
                [binary, str(harness)], capture_output=True, text=True, timeout=90, env=env, check=False
            )
        except (OSError, subprocess.TimeoutExpired):
            return None
    for line in (proc.stdout + proc.stderr).splitlines():
        marker = line.find("RESULT ")
        if marker != -1:
            try:
                return json.loads(line[marker + len("RESULT "):])
            except json.JSONDecodeError:
                return None
    return None


class ThemePaletteQmlTests(unittest.TestCase):
    payload: dict | None = None

    @classmethod
    def setUpClass(cls) -> None:
        cls.payload = _run_harness()
        if cls.payload is None:
            raise unittest.SkipTest("no QML runtime with Kirigami/Plasma modules available")

    def test_desktop_theme_mode_uses_the_live_plasma_colors(self):
        plasma = self.payload["plasma"]
        kirigami = self.payload["kirigami"]
        self.assertFalse(plasma["themed"])
        # Without a theme of its own the palette must hand back exactly what the
        # attached Kirigami theme reports — anything else means it resolved
        # against something other than the desktop.
        self.assertEqual(plasma["background"].lower(), kirigami["background"].lower())
        self.assertEqual(plasma["text"].lower(), kirigami["text"].lower())

    def test_desktop_theme_mode_is_not_an_uninitialised_black_palette(self):
        # The regression this file exists for: an uninitialised attached theme
        # reports #000000 for every role, painting the whole widget black. A real
        # theme is never black in all of them at once.
        plasma = self.payload["plasma"]
        roles = [plasma[key].lower() for key in ("background", "text", "highlight", "subtext")]
        self.assertNotEqual(roles, ["#000000"] * len(roles), plasma)

    def test_highlight_follows_the_desktop_instead_of_our_accent(self):
        # Selection inside the popup must not be repainted with the widget's
        # fixed accent while following the desktop theme.
        self.assertEqual(
            self.payload["plasma"]["highlight"].lower(),
            self.payload["kirigami"]["highlight"].lower(),
        )

    def test_transparency_is_read_without_a_theme(self):
        self.assertTrue(self.payload["plasma"]["translucent"])

    def test_themed_palette_wins_over_the_desktop_colors(self):
        themed = self.payload["themed"]
        self.assertEqual(themed["background"].lower(), "#2e3440")
        self.assertEqual(themed["critical"].lower(), "#bf616a")
        self.assertEqual(themed["radius"], 10)


if __name__ == "__main__":
    unittest.main()
