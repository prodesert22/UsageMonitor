"""Tests for the Waybar popup launcher.

Everything here runs without Qt installed: binding detection, the install hints,
the single-instance socket path and the window placement maths are pure Python
precisely so they can be checked on a headless CI box.
"""

from __future__ import annotations

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

_ASSET_DIR = Path(__file__).resolve().parents[3] / "usage-monitor-cli" / "assets" / "waybar"


def _load(name: str):
    spec = importlib.util.spec_from_file_location(name, _ASSET_DIR / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


_load("usage_monitor_waybar_data")
popup = _load("usage_monitor_waybar_popup")


class BindingTests(unittest.TestCase):
    def test_available_binding_returns_none_when_nothing_is_installed(self):
        self.assertIsNone(popup.available_binding(("definitely_not_a_qt_binding",)))

    def test_available_binding_finds_an_installed_module(self):
        self.assertEqual(popup.available_binding(("json", "PySide6")), "json")

    def test_missing_qt_message_names_the_distro_package(self):
        message = popup.missing_qt_message(popup.qt_install_hint(["fedora"]))
        self.assertIn("python3-pyside6", message)
        # The bar module must keep working without Qt; the message says so.
        self.assertIn("Waybar module itself keeps working", message)

    def test_install_hint_falls_back_through_id_like(self):
        self.assertIn("apt", popup.qt_install_hint(["neon", "ubuntu"]))
        self.assertEqual(popup.qt_install_hint(["plan9"]), popup._GENERIC_QT_HINT)

    def test_os_release_ids_reads_id_and_id_like(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "os-release"
            path.write_text('ID=cachyos\nID_LIKE="arch"\nNAME="CachyOS"\n', encoding="utf-8")
            self.assertEqual(popup.os_release_ids(path), ["cachyos", "arch"])

    def test_os_release_ids_is_empty_when_the_file_is_missing(self):
        self.assertEqual(popup.os_release_ids(Path("/nonexistent/os-release")), [])


class SocketTests(unittest.TestCase):
    def test_socket_lives_in_the_runtime_dir_and_is_scoped_per_display(self):
        with mock.patch.dict(os.environ, {"XDG_RUNTIME_DIR": "/run/user/1000"}, clear=True):
            path = popup.socket_path("wayland-1")
        self.assertEqual(str(path), "/run/user/1000/usage-monitor-waybar-wayland-1.sock")

    def test_runtime_dir_falls_back_per_uid_without_xdg_runtime_dir(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            path = popup.runtime_dir()
        # Never a shared, guessable /tmp path: sessions without XDG_RUNTIME_DIR
        # (plain X sessions, FreeBSD) still get a private socket.
        self.assertEqual(path, Path("/tmp") / f"usage-monitor-{os.getuid()}")

    def test_socket_name_without_a_display(self):
        with mock.patch.dict(os.environ, {"XDG_RUNTIME_DIR": "/run/user/1000"}, clear=True):
            self.assertEqual(popup.socket_path("").name, "usage-monitor-waybar.sock")


class PlacementTests(unittest.TestCase):
    SCREEN = (0, 40, 1920, 1040)  # 1080p with a 40px bar at the top
    SIZE = (420, 520)

    def test_cursor_anchor_opens_below_a_pointer_in_the_top_half(self):
        x, y = popup_pos = popup.popup_position("cursor", self.SCREEN, self.SIZE, (960, 45), margin=8)
        self.assertEqual(popup_pos, (960 - 210, 53))
        self.assertGreaterEqual(y, 40)
        self.assertEqual(x, 750)

    def test_cursor_anchor_opens_above_a_pointer_in_the_bottom_half(self):
        _, y = popup.popup_position("cursor", self.SCREEN, self.SIZE, (960, 1000), margin=8)
        self.assertEqual(y, 1000 - 520 - 8)

    def test_auto_uses_the_pointer_when_there_is_one(self):
        with_cursor = popup.popup_position("auto", self.SCREEN, self.SIZE, (300, 45))
        self.assertEqual(with_cursor, popup.popup_position("cursor", self.SCREEN, self.SIZE, (300, 45)))

    def test_auto_falls_back_to_the_top_right_without_a_pointer(self):
        self.assertEqual(
            popup.popup_position("auto", self.SCREEN, self.SIZE, None),
            popup.popup_position("top-right", self.SCREEN, self.SIZE, None),
        )

    def test_every_anchor_keeps_the_window_on_screen(self):
        for anchor in popup.ANCHORS:
            x, y = popup.popup_position(anchor, self.SCREEN, self.SIZE, (1919, 1079))
            self.assertGreaterEqual(x, 0, anchor)
            self.assertGreaterEqual(y, 40, anchor)
            self.assertLessEqual(x + self.SIZE[0], 1920, anchor)
            self.assertLessEqual(y + self.SIZE[1], 40 + 1040, anchor)

    def test_window_larger_than_the_screen_is_pinned_to_the_origin(self):
        self.assertEqual(popup.popup_position("center", (0, 0, 300, 300), (400, 400), None), (0, 0))

    def test_effective_anchor_prefers_the_command_line(self):
        self.assertEqual(popup.effective_anchor("bottom-left", "center"), "bottom-left")
        self.assertEqual(popup.effective_anchor("auto", "center"), "center")
        self.assertEqual(popup.effective_anchor("auto", "nonsense"), "auto")
        self.assertEqual(popup.effective_anchor("auto", ""), "auto")


class LayerShellTests(unittest.TestCase):
    """Wayland placement.

    Bug report: the popup opened in the middle of the screen instead of under
    the bar. A Wayland client cannot move its own toplevel, so the fix is a
    layer surface anchored to the screen edge — the same protocol the bar uses.
    """

    def _plugins(self, td: str, with_plugin: bool = True) -> str:
        directory = Path(td) / "plugins" / "wayland-shell-integration"
        if with_plugin:
            directory.mkdir(parents=True)
            (directory / "liblayer-shell.so").write_bytes(b"")
        return str(Path(td) / "plugins")

    def test_plugin_is_looked_up_inside_the_active_qt_install(self):
        with tempfile.TemporaryDirectory() as td:
            plugins = self._plugins(td)
            self.assertIsNotNone(popup.layer_shell_plugin(plugins))
        with tempfile.TemporaryDirectory() as td:
            # pip-installed PySide6 ships its own Qt without the integration:
            # pointing Qt at a foreign one breaks the Wayland platform plugin
            # outright, so a missing file must disable the feature.
            self.assertIsNone(popup.layer_shell_plugin(self._plugins(td, with_plugin=False)))
        self.assertIsNone(popup.layer_shell_plugin(None))

    def test_enabled_only_on_wayland_with_the_plugin(self):
        with tempfile.TemporaryDirectory() as td:
            plugins = self._plugins(td)
            self.assertTrue(popup.use_layer_shell("wayland", plugins, {}))
            self.assertFalse(popup.use_layer_shell("x11", plugins, {}))
            self.assertFalse(popup.use_layer_shell("wayland", None, {}))
            self.assertFalse(
                popup.use_layer_shell("wayland", plugins, {"USAGE_MONITOR_LAYER_SHELL": "0"})
            )

    def test_anchor_flags_map_to_screen_edges(self):
        top_right = popup.ANCHOR_TOP | popup.ANCHOR_RIGHT
        self.assertEqual(popup.layer_anchor_flags("top-right"), top_right)
        self.assertEqual(popup.layer_anchor_flags("bottom-left"), popup.ANCHOR_BOTTOM | popup.ANCHOR_LEFT)
        self.assertEqual(popup.layer_anchor_flags("top"), popup.ANCHOR_TOP)
        # Centre means "unanchored": the compositor centres a layer surface with
        # no edges.
        self.assertEqual(popup.layer_anchor_flags("center"), 0)
        # Wayland hands out no pointer position, so the pointer modes land where
        # the module usually sits.
        self.assertEqual(popup.layer_anchor_flags("cursor"), top_right)
        self.assertEqual(popup.layer_anchor_flags("auto"), top_right)
        self.assertEqual(popup.layer_anchor_flags("nonsense"), top_right)


class PaletteTests(unittest.TestCase):
    def test_palette_roles_map_the_theme_colors(self):
        theme = {"colors": {"background": "#111111", "text": "#eeeeee", "accent": "#0a84ff",
                            "subtext": "#888888", "track": "#333333"}}
        roles = popup.palette_roles(theme)
        self.assertEqual(roles["Window"], "#111111")
        self.assertEqual(roles["WindowText"], "#eeeeee")
        self.assertEqual(roles["Highlight"], "#0a84ff")
        self.assertEqual(roles["PlaceholderText"], "#888888")

    def test_palette_roles_have_defaults_for_a_broken_theme(self):
        roles = popup.palette_roles({})
        self.assertEqual(set(roles), set(popup.palette_roles({"colors": {}})))
        self.assertTrue(all(value.startswith("#") for value in roles.values()))


class ArgumentTests(unittest.TestCase):
    def test_defaults(self):
        args = popup.build_parser().parse_args([])
        self.assertEqual(args.anchor, "auto")
        self.assertEqual((args.width, args.height), (popup.DEFAULT_WIDTH, popup.DEFAULT_HEIGHT))
        self.assertFalse(args.once)
        self.assertFalse(args.no_single_instance)

    def test_anchor_is_validated(self):
        parser = popup.build_parser()
        self.assertEqual(parser.parse_args(["--anchor", "bottom-right"]).anchor, "bottom-right")
        with self.assertRaises(SystemExit):
            parser.parse_args(["--anchor", "sideways"])

    def test_doctor_reports_the_environment(self):
        payload = popup.doctor_payload(popup.build_parser().parse_args(["--doctor"]))
        for key in ("qtBinding", "sessionType", "socket", "qmlDir", "state", "theme",
                    "layerShell", "qtPluginsDir", "minFetchIntervalSeconds"):
            self.assertIn(key, payload)

    def test_doctor_runs_as_a_subprocess_without_qt(self):
        import json
        import subprocess

        with tempfile.TemporaryDirectory() as td:
            proc = subprocess.run(
                [sys.executable, str(_ASSET_DIR / "usage-monitor-waybar-popup"), "--doctor"],
                cwd=td,
                capture_output=True,
                text=True,
                timeout=60,
                check=False,
                env={**os.environ, "USAGE_MONITOR_BIN": "/bin/true"},
            )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("qtBinding", json.loads(proc.stdout))


class QmlAssetTests(unittest.TestCase):
    def test_every_component_the_popup_loads_is_shipped(self):
        ui = _ASSET_DIR / "ui"
        expected = [
            "Popup.qml", "UsagePage.qml", "UsageBar.qml", "ThemePalette.qml",
            "ThemedToolButton.qml", "GlyphIcon.qml", "FontPicker.qml",
            "SettingsWindow.qml", "SettingsGeneral.qml", "SettingsProviders.qml",
            "SettingsOrder.qml", "SettingsTheme.qml", "images/usage-monitor.png",
        ]
        missing = [name for name in expected if not (ui / name).exists()]
        self.assertEqual(missing, [])


if __name__ == "__main__":
    unittest.main()
