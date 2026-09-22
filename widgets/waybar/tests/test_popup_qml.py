"""Smoke test for the Waybar popup QML against a real QML engine.

The popup is plain Qt Quick — no Kirigami, no Plasma — so unlike the KDE widget
it can be instantiated by any Qt 6 `qml` runtime. The test creates Popup.qml and
SettingsWindow.qml with a stub `backend` (the object the Python launcher injects
as a context property) and reads back what the components resolved: a QML error,
a renamed backend member or a broken theme binding fails here instead of at the
first `on-click`.

Skipped when the machine has no QML runtime or no QtQuick.Controls module.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

_UI = Path(__file__).resolve().parents[3] / "usage-monitor-cli" / "assets" / "waybar" / "ui"

_QML_BINARIES = ("/usr/lib/qt6/bin/qml", "qml6", "qml")

_SUMMARY = {
    "text": "42%",
    "tooltip": "Codex session: 42%",
    "class": "ok",
    "percentage": 42,
    "barProvider": "codex",
    "providers": [{
        "provider": "codex",
        "displayName": "Codex",
        "accountText": "dev@example.com",
        "maxPercent": 42,
        "usage": {"primary": {"usedPercent": 42, "resetDescription": "in 3h"}},
    }],
    "theme": {
        "mode": "builtin", "id": "nord", "name": "Nord", "dark": True,
        "colors": {"background": "#2e3440", "text": "#eceff4", "subtext": "#81a1c1",
                   "accent": "#88c0d0", "warning": "#ebcb8b", "critical": "#bf616a",
                   "track": "#3b4252", "border": "#4c566a"},
        "font": {}, "metrics": {"radius": 10, "barHeight": 8, "opacity": 0.6},
    },
}

_SETTINGS = {
    "providers": [{"id": "codex", "displayName": "Codex", "enabled": True, "accounts": [],
                   "accountFields": [], "connectHint": "hint", "authKind": "oauth", "setupHint": ""}],
    "pinnableProviders": [{"id": "codex", "displayName": "Codex"}],
    "pinnedProvider": "codex",
    "refreshIntervalSeconds": 30,
    "showAccountEmail": True,
    "keepOpen": False,
    "popupAnchor": "auto",
    "providerOrder": "[]",
    "theme": _SUMMARY["theme"],
    "themeCatalog": {
        "builtin": [{"id": "nord", "name": "Nord", "dark": True,
                     "colors": _SUMMARY["theme"]["colors"], "font": {}, "metrics": {}}],
        "schemes": [],
        "custom": [],
        "system": {"name": "Desktop colors (dark)", "dark": True,
                   "colors": _SUMMARY["theme"]["colors"], "font": {}, "metrics": {}},
        "colorKeys": ["background", "text", "subtext", "accent", "warning", "critical", "track", "border"],
    },
    "themeState": {"themeMode": "builtin", "themeBuiltin": "nord"},
    "session": {"desktop": "hyprland", "type": "wayland"},
    "popupVersion": "test",
    "cliVersion": "test",
    "update": {"installed": "0.8.0", "available": "0.8.1", "outdated": True,
               "dismissed": False, "url": "https://example.com/releases/tag/v0.8.1"},
}

_HARNESS = """
import QtQuick
import QtQuick.Controls as QQC2

QQC2.ApplicationWindow {
    id: harness
    visible: false
    width: 100
    height: 100

    // Stand-in for the context property the Python launcher injects. Every
    // member the QML calls has to exist here, so a rename on either side fails
    // this test.
    QtObject {
        id: backend

        property string summaryJson: '%(summary)s'
        property string settingsJson: '%(settings)s'
        property string costJson: '{"cost": [{"provider": "codex", "last30DaysCostUSD": 1.5}]}'
        property string updateNotesJson: '{"version": "0.8.1", "url": "", "body": "notes", "source": "embedded"}'
        property bool busy: false
        property string errorText: ""
        property string errorDetails: ""
        property string uiDir: "%(ui)s"
        property string version: "test"
        property string sessionType: "wayland"
        property string desktop: "hyprland"

        property var calls: []

        signal settingsRequested()

        function record(name) { backend.calls.push(name) }
        function refresh() { backend.record("refresh") }
        function forceRefresh() { backend.record("forceRefresh") }
        function applyLayerShell(win, role) { backend.record("applyLayerShell:" + role) }
        function loadCache() { backend.record("loadCache") }
        function loadSettings() { backend.record("loadSettings") }
        function fetchCost() { backend.record("fetchCost") }
        function saveStateKey(key, value) { backend.record("saveStateKey:" + key + "=" + value) }
        function batchSetState(pairs) { backend.record("batchSetState:" + pairs) }
        function setProviderEnabled(id, enabled) { backend.record("setProviderEnabled") }
        function accountSave(a, b, c, d) { backend.record("accountSave") }
        function accountRemove(a, b) { backend.record("accountRemove") }
        function workspaceAdd(a, b) { backend.record("workspaceAdd") }
        function workspaceRemove(a) { backend.record("workspaceRemove") }
        function cacheClear() { backend.record("cacheClear") }
        function updateChangelog(v) { backend.record("updateChangelog:" + v) }
        function applyUpdate() { backend.record("applyUpdate") }
        function dismissUpdate(v) { backend.record("dismissUpdate:" + v) }
        function copyToClipboard(t) { backend.record("copyToClipboard") }
        function quitApp() { backend.record("quitApp") }
    }

    Component.onCompleted: {
        var result = ({ "errors": [] })

        var popupComponent = Qt.createComponent("%(ui)s/Popup.qml")
        if (popupComponent.status === Component.Error) {
            result.errors.push("Popup.qml: " + popupComponent.errorString())
            console.log("RESULT " + JSON.stringify(result))
            Qt.exit(0)
            return
        }
        var popup = popupComponent.createObject(harness)
        if (!popup) {
            result.errors.push("Popup.qml: createObject returned null")
            console.log("RESULT " + JSON.stringify(result))
            Qt.exit(0)
            return
        }

        result.theme = {
            "background": String(popup.ui.backgroundColor),
            "critical": String(popup.ui.levelColor(95)),
            "radius": popup.ui.cornerRadius,
            "barHeight": popup.ui.barHeight,
            "opacity": popup.ui.backgroundOpacity,
            "translucent": popup.ui.translucent,
            "name": popup.ui.name
        }
        result.summaryText = popup.summary.text
        result.providerCount = (popup.summary.providers || []).length
        result.pinnedPercent = popup.compactLabelPct()
        result.windowColorIsTransparent = String(popup.color) === "#00000000"
        // The fixture settings carry an outdated update, so the banner shows.
        result.updateBannerVisible = popup.updateBanner.visible === true
        result.updateBannerText = popup.updateBanner.bannerText || ""

        var settingsComponent = Qt.createComponent("%(ui)s/SettingsWindow.qml")
        if (settingsComponent.status === Component.Error) {
            result.errors.push("SettingsWindow.qml: " + settingsComponent.errorString())
        } else {
            var settings = settingsComponent.createObject(harness, { "ui": popup.ui })
            if (!settings) {
                result.errors.push("SettingsWindow.qml: createObject returned null")
            } else {
                settings.setPending("refreshIntervalSeconds", 60)
                result.dirty = settings.dirty
                result.stateValue = settings.stateValue("themeBuiltin", "macos-dark")
                settings.apply()
                result.applied = String(backend.calls[backend.calls.length - 1])
                result.dirtyAfterApply = settings.dirty
                settings.destroy()
            }
        }

        // Both windows have to register for layer-shell placement before they
        // are shown, or the compositor centres them (bug report: "popup opens
        // in the middle of the screen instead of under the bar").
        result.layerCalls = backend.calls.filter(function(call) {
            return String(call).indexOf("applyLayerShell:") === 0
        })

        popup.destroy()
        console.log("RESULT " + JSON.stringify(result))
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


def _run_harness() -> tuple[dict | None, str]:
    binary = _qml_binary()
    if not binary:
        return None, ""
    source = _HARNESS % {
        "ui": _UI.as_uri(),
        "summary": json.dumps(_SUMMARY).replace("'", "\\'"),
        "settings": json.dumps(_SETTINGS).replace("'", "\\'"),
    }
    with tempfile.TemporaryDirectory() as td:
        harness = Path(td) / "harness.qml"
        harness.write_text(source, encoding="utf-8")
        env = {
            **os.environ,
            "QT_QPA_PLATFORM": "offscreen",
            "QT_FORCE_STDERR_LOGGING": "1",
            "QT_QUICK_CONTROLS_STYLE": "Fusion",
            # A stale compiled copy would hide the very regression under test.
            "QML_DISABLE_DISK_CACHE": "1",
        }
        try:
            proc = subprocess.run(
                [binary, str(harness)], capture_output=True, text=True, timeout=90, env=env, check=False
            )
        except (OSError, subprocess.TimeoutExpired):
            return None, ""
    output = proc.stdout + proc.stderr
    for line in output.splitlines():
        marker = line.find("RESULT ")
        if marker != -1:
            try:
                return json.loads(line[marker + len("RESULT "):]), output
            except json.JSONDecodeError:
                return None, output
    return None, output


class PopupQmlTests(unittest.TestCase):
    payload: dict | None = None
    output: str = ""

    @classmethod
    def setUpClass(cls) -> None:
        cls.payload, cls.output = _run_harness()
        if cls.payload is None:
            raise unittest.SkipTest("no Qt 6 QML runtime with QtQuick.Controls available")

    def test_components_load_without_errors(self):
        self.assertEqual(self.payload.get("errors"), [], self.output)

    def test_popup_renders_the_helper_payload(self):
        self.assertEqual(self.payload["summaryText"], "42%")
        self.assertEqual(self.payload["providerCount"], 1)
        # The pinned provider drives the headline number, as in Plasma.
        self.assertEqual(self.payload["pinnedPercent"], 42)

    def test_theme_from_the_payload_styles_the_popup(self):
        theme = self.payload["theme"]
        self.assertEqual(theme["background"].lower()[:7], "#2e3440")
        self.assertEqual(theme["critical"].lower()[:7], "#bf616a")
        self.assertEqual(theme["radius"], 10)
        self.assertEqual(theme["barHeight"], 8)
        self.assertEqual(theme["name"], "Nord")

    def test_transparency_makes_the_window_surface_transparent(self):
        # Without a transparent window colour the rounded/translucent background
        # would be composited onto an opaque frame and the slider would do nothing.
        self.assertTrue(self.payload["theme"]["translucent"])
        self.assertTrue(self.payload["windowColorIsTransparent"])

    def test_both_windows_register_for_layer_shell_placement(self):
        self.assertEqual(self.payload["layerCalls"], ["applyLayerShell:popup", "applyLayerShell:settings"])

    def test_update_banner_shows_when_settings_report_outdated(self):
        self.assertTrue(self.payload["updateBannerVisible"], self.output)
        self.assertIn("0.8.1", self.payload["updateBannerText"])
        self.assertIn("0.8.0", self.payload["updateBannerText"])

    def test_settings_window_applies_pending_edits_in_one_batch(self):
        self.assertTrue(self.payload["dirty"])
        self.assertEqual(self.payload["stateValue"], "nord")
        self.assertEqual(self.payload["applied"], 'batchSetState:[["refreshIntervalSeconds","60"]]')
        self.assertFalse(self.payload["dirtyAfterApply"])


if __name__ == "__main__":
    unittest.main()
