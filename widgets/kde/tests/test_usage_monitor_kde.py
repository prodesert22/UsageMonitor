import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from typing import ClassVar
from unittest import mock

# The KDE helper now lives in the CLI asset tree (single source of truth,
# embedded by the installer); load it directly from there.
_KDE_CODE = (
    Path(__file__).resolve().parents[3]
    / "usage-monitor-cli"
    / "assets"
    / "kde"
    / "package"
    / "contents"
    / "code"
)


def _load_module():
    spec = importlib.util.spec_from_file_location(
        "usage_monitor_kde", _KDE_CODE / "usage_monitor_kde.py"
    )
    module = importlib.util.module_from_spec(spec)
    # Register before exec so dataclasses can resolve string annotations
    # (`from __future__ import annotations`) via sys.modules.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


um = _load_module()


def proc(stdout="", returncode=0, stderr=""):
    return subprocess.CompletedProcess(["usage-monitor-cli"], returncode, stdout, stderr)


WIDGET_PAYLOAD = {
    "text": "80%",
    "tooltip": "tt",
    "class": "warning",
    "percentage": 80,
    "providers": [
        {
            "provider_id": "codex",
            "display_name": "Codex",
            "max_percentage": 80,
            "windows": [
                {"id": "primary", "label": "Session", "percentage": 80, "resets_at": "Resets at 14:00"},
                {"id": "secondary", "label": "Weekly", "percentage": 30},
            ],
        },
        {
            "provider_id": "claude",
            "account_id": "work",
            "account_label": "Work",
            "display_name": "Claude",
            "plan": "Claude Pro",
            "max_percentage": 42,
            "windows": [
                {"id": "primary", "label": "Session", "percentage": 42},
                {"id": "secondary", "label": "Weekly", "percentage": 7},
            ],
        },
    ],
}


class BinaryTests(unittest.TestCase):
    def test_binary_prefers_env(self):
        with mock.patch.dict(os.environ, {"USAGE_MONITOR_BIN": "/tmp/bin"}, clear=True):
            self.assertEqual(um.usage_monitor_binary(), "/tmp/bin")

    def test_run_cli_missing_binary(self):
        with mock.patch.object(um.subprocess, "run", side_effect=FileNotFoundError):
            result = um.run_cli(["list"])
            self.assertEqual(result.returncode, 127)
            self.assertIn("not found", result.stderr)


class StateTests(unittest.TestCase):
    def test_state_roundtrip(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
            um._write_state({"barProvider": "codex"})
            self.assertEqual(um.state_value(key="barProvider"), "codex")

    def test_provider_order_parses_json_string(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
            um._write_state({"providerOrder": '["claude","codex"]'})
            self.assertEqual(um._provider_order(), ["claude", "codex"])


class FetchTests(unittest.TestCase):
    def test_fetch_entries_maps_windows_and_identity(self):
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(WIDGET_PAYLOAD)))
        codex = entries[0]
        self.assertEqual(codex["provider"], "codex")
        self.assertEqual(codex["usage"]["primary"]["usedPercent"], 80.0)
        self.assertEqual(codex["usage"]["primary"]["resetDescription"], "Resets at 14:00")
        claude = entries[1]
        self.assertEqual(claude["usage"]["identity"]["accountEmail"], "Work")
        self.assertEqual(claude["usage"]["identity"]["accountOrganization"], "Claude Pro")

    def test_fetch_entries_reports_error(self):
        payload = {"providers": [{"provider_id": "openai", "display_name": "OpenAI", "error": "missing API key", "windows": []}]}
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(payload)))
        self.assertEqual(entries[0]["error"]["message"], "missing API key")

    def test_fetch_entries_handles_cli_failure(self):
        entries = um.fetch_entries(runner=lambda args: proc(returncode=1, stderr="boom"))
        self.assertEqual(entries[0]["error"]["message"], "boom")

    def test_identity_prefers_email_then_falls_back_to_plan(self):
        payload = {"providers": [
            {"provider_id": "codex", "display_name": "Codex", "account_email": "me@x.com",
             "account_label": "Go", "plan": "ChatGPT", "windows": [{"id": "primary", "percentage": 5}]},
            {"provider_id": "claude", "display_name": "Claude", "plan": "Claude Pro",
             "windows": [{"id": "primary", "percentage": 5}]},
        ]}
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(payload)))
        enriched = um.enrich_entries(entries)
        # Email wins over the configured label.
        self.assertEqual(enriched[0]["accountText"], "me@x.com · ChatGPT")
        # No email/label/id → plan is shown so the card is not blank.
        self.assertEqual(enriched[1]["accountText"], "Claude Pro")


class SummaryTests(unittest.TestCase):
    def test_summarize_bar_text_and_class(self):
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(WIDGET_PAYLOAD)))
        payload = um.summarize(entries)
        self.assertEqual(payload["percentage"], 80.0)
        self.assertEqual(payload["class"], "warning")
        self.assertEqual(payload["text"], "80%")  # max across providers, no pin

    def test_summarize_pinned_provider_two_windows(self):
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(WIDGET_PAYLOAD)))
        payload = um.summarize(entries, pinned_provider="claude")
        self.assertEqual(payload["text"], "42% • 7%")
        self.assertEqual(payload["barProvider"], "claude")

    def test_summarize_pinned_provider_three_windows(self):
        payload = {"providers": [
            {"provider_id": "kimi", "display_name": "Kimi",
             "windows": [
                 {"id": "primary", "percentage": 28, "resets_at": "Resets at 19:29"},
                 {"id": "secondary", "percentage": 46},
                 {"id": "tertiary", "percentage": 31},
             ]},
        ]}
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(payload)))
        summary = um.summarize(entries, pinned_provider="kimi")
        self.assertEqual(summary["text"], "28% • 46% • 31%")

    def test_bar_text_skips_missing_windows(self):
        payload = {"providers": [
            {"provider_id": "kimi", "display_name": "Kimi",
             "windows": [
                 {"id": "primary", "percentage": 28},
                 {"id": "tertiary", "percentage": 31},
             ]},
        ]}
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(payload)))
        summary = um.summarize(entries, pinned_provider="kimi")
        self.assertEqual(summary["text"], "28% • 31%")

    def test_summarize_orders_by_state(self):
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(WIDGET_PAYLOAD)))
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
            um._write_state({"providerOrder": '["claude","codex"]'})
            payload = um.summarize(entries, pinned_provider="")
            self.assertEqual(payload["providers"][0]["provider"], "claude")


class SettingsTests(unittest.TestCase):
    LIST_OUT = (
        "codex        enabled          Codex — ChatGPT plan\n"
        "claude       disabled (auto)  Claude — Claude Pro\n"
    )
    SHOWS: ClassVar[dict[tuple[str, ...], str]] = {
        ("codex", "show"): "provider = codex\nstate = enabled\n[default] (auto-detected)\n[work] Work Account\n  disabled\n",
        ("codex", "account", "list"): "[work] Work Account\n  disabled\n",
        ("claude", "show"): "provider = claude\nstate = disabled (auto)\n(no accounts configured)\n",
        ("claude", "account", "list"): "(no accounts configured)\n",
    }

    def fake_output(self, args):
        if tuple(args) == ("list",):
            return self.LIST_OUT
        return self.SHOWS.get(tuple(args), "")

    def test_settings_payload_shape(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
            um._write_state({"refreshIntervalSeconds": 45, "barProvider": "codex", "providerOrder": '["codex"]'})
            with mock.patch.object(um, "cli_output", side_effect=self.fake_output), \
                 mock.patch.object(um, "cli_version", return_value="0.6.0"):
                payload = um.settings_payload()
                self.assertEqual(payload["refreshIntervalSeconds"], 45)
                self.assertEqual(payload["pinnedProvider"], "codex")
                self.assertEqual(payload["providerOrder"], '["codex"]')
                self.assertEqual(payload["cliVersion"], "0.6.0")
                codex = payload["providers"][0]
                self.assertEqual(codex["id"], "codex")
                self.assertTrue(codex["enabled"])
                self.assertEqual(codex["availableSources"], ["auto"])
                ids = [a["id"] for a in codex["accounts"]]
                self.assertIn("default", ids)
                self.assertIn("work", ids)
                work = next(a for a in codex["accounts"] if a["id"] == "work")
                self.assertEqual(work["active"], "false")
                self.assertEqual([p["id"] for p in payload["pinnableProviders"]], ["codex"])
                self.assertIn("connectHint", codex)

    def test_settings_payload_carries_the_theme(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
            um._write_state({"themeMode": "builtin", "themeBuiltin": "tokyo-night"})
            with mock.patch.object(um, "cli_output", side_effect=self.fake_output), \
                 mock.patch.object(um, "cli_version", return_value="0.7.3"), \
                 mock.patch.object(um, "installed_schemes", return_value=[]):
                payload = um.settings_payload()
            self.assertEqual(payload["theme"]["id"], "tokyo-night")
            self.assertEqual(payload["theme"]["colors"]["background"], "#1a1b26")
            self.assertEqual(len(payload["themeCatalog"]["builtin"]), 5)
            # Raw state keys drive the config page's pending-edit fallbacks.
            self.assertEqual(payload["themeState"]["themeBuiltin"], "tokyo-night")


SCHEME_FILE = """[General]
Name=Test Mojave

[Colors:Button]
BackgroundNormal=60,60,60

[Colors:Window]
BackgroundNormal=28,28,30
ForegroundNormal=245,245,247
ForegroundInactive=152,152,157
ForegroundNegative=255,69,58
ForegroundNeutral=255,159,10
DecorationFocus=10,132,255
"""


class ColorTests(unittest.TestCase):
    def test_color_value_accepts_hex_and_rgb(self):
        self.assertEqual(um.color_value("#1C1C1E"), "#1c1c1e")
        self.assertEqual(um.color_value("#abc"), "#abc")
        self.assertEqual(um.color_value("28,28,30"), "#1c1c1e")
        self.assertEqual(um.color_value("28,28,30,255"), "#1c1c1e")

    def test_color_value_rejects_junk(self):
        # Anything QML could not parse is dropped rather than passed through.
        # "#8abc" included: QColor has no 4-digit form.
        for bad in ("", None, "red; evil", "rgb(1,2,3)", "#12345", "#8abc", "not a color"):
            self.assertEqual(um.color_value(bad), "")

    def test_is_dark(self):
        self.assertTrue(um.is_dark("#1c1c1e"))
        self.assertFalse(um.is_dark("#f5f5f7"))
        self.assertTrue(um.is_dark("#ff1c1c1e"))  # #aarrggbb


class ThemeTests(unittest.TestCase):
    def test_default_is_the_desktop_theme(self):
        theme = um.resolve_theme({})
        self.assertEqual(theme["mode"], "plasma")
        self.assertEqual(theme["colors"], {})
        self.assertEqual(theme["font"]["family"], "")

    def test_builtin_theme(self):
        theme = um.resolve_theme({"themeMode": "builtin", "themeBuiltin": "nord"})
        self.assertEqual(theme["mode"], "builtin")
        self.assertEqual(theme["name"], "Nord")
        self.assertEqual(theme["colors"]["background"], "#2e3440")
        self.assertTrue(theme["dark"])

    def test_unknown_builtin_falls_back_to_default(self):
        theme = um.resolve_theme({"themeMode": "builtin", "themeBuiltin": "nope"})
        self.assertEqual(theme["id"], um.DEFAULT_BUILTIN)

    def test_five_builtin_themes_are_offered(self):
        self.assertEqual(len(um.BUILTIN_THEMES), 5)
        self.assertIn("macos-dark", um.BUILTIN_THEMES)
        self.assertIn("macos-light", um.BUILTIN_THEMES)

    def test_custom_theme_overrides_the_base(self):
        theme = um.resolve_theme({
            "themeMode": "custom",
            "themeBuiltin": "nord",
            "themeCustomBackground": "#101010",
            "themeCustomAccent": "not-a-color",
            "themeCustomFontFamily": "Fira Sans",
            "themeCustomFontSize": "12",
            "themeCustomBarHeight": "10",
            "themeCustomOpacity": "0.85",
        })
        self.assertEqual(theme["mode"], "custom")
        self.assertEqual(theme["colors"]["background"], "#101010")
        # Unset keys keep the base theme's value; invalid ones are ignored.
        self.assertEqual(theme["colors"]["text"], um.BUILTIN_THEMES["nord"]["colors"]["text"])
        self.assertEqual(theme["colors"]["accent"], um.BUILTIN_THEMES["nord"]["colors"]["accent"])
        self.assertEqual(theme["font"], {"family": "Fira Sans", "size": 12.0, "headingSize": 0.0, "smallSize": 0.0})
        self.assertEqual(theme["metrics"]["barHeight"], 10.0)
        self.assertEqual(theme["metrics"]["opacity"], 0.85)

    def test_transparency_applies_to_every_mode(self):
        raw = json.dumps([{"id": "t1", "name": "Mine", "base": "nord"}])
        for state in (
            {"themeOpacity": "0.7"},
            {"themeMode": "builtin", "themeBuiltin": "nord", "themeOpacity": "0.7"},
            {"themeMode": "custom", "customThemes": raw, "themeOpacity": "0.7"},
        ):
            self.assertEqual(um.resolve_theme(state)["metrics"]["opacity"], 0.7, state)

    def test_transparency_survives_a_missing_scheme(self):
        with tempfile.TemporaryDirectory() as td, \
             mock.patch.dict(os.environ, {"XDG_DATA_HOME": td, "XDG_DATA_DIRS": td}, clear=True):
            theme = um.resolve_theme({"themeMode": "scheme", "themeScheme": "colors:Gone", "themeOpacity": "0.5"})
            self.assertEqual(theme["mode"], "plasma")
            self.assertEqual(theme["metrics"]["opacity"], 0.5)

    def test_transparency_is_clamped_and_defaults_to_solid(self):
        self.assertEqual(um.resolve_theme({})["metrics"]["opacity"], 1.0)
        self.assertEqual(um.resolve_theme({"themeOpacity": ""})["metrics"]["opacity"], 1.0)
        self.assertEqual(um.resolve_theme({"themeOpacity": "5"})["metrics"]["opacity"], 1.0)
        self.assertEqual(um.resolve_theme({"themeOpacity": "0"})["metrics"]["opacity"], 0.1)
        self.assertEqual(um.resolve_theme({"themeOpacity": "junk"})["metrics"]["opacity"], 1.0)

    def test_theme_opacity_falls_back_to_the_custom_theme_value(self):
        raw = json.dumps([{"id": "t1", "name": "Mine", "base": "nord", "metrics": {"opacity": 0.8}}])
        state = {"themeMode": "custom", "customThemes": raw}
        # No slider value yet: keep what the theme itself carries.
        self.assertEqual(um.resolve_theme(state)["metrics"]["opacity"], 0.8)
        # The slider wins once it is set.
        self.assertEqual(um.resolve_theme({**state, "themeOpacity": "0.5"})["metrics"]["opacity"], 0.5)

    def test_legacy_custom_opacity_is_clamped(self):
        self.assertEqual(um.resolve_theme({"themeMode": "custom", "themeCustomOpacity": "5"})["metrics"]["opacity"], 1.0)
        self.assertEqual(um.resolve_theme({"themeMode": "custom", "themeCustomOpacity": "0"})["metrics"]["opacity"], 0.1)
        self.assertEqual(um.resolve_theme({"themeMode": "custom", "themeCustomOpacity": "x"})["metrics"]["opacity"], 1.0)

    def test_read_color_scheme_maps_kde_keys(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "Mojave.colors"
            path.write_text(SCHEME_FILE, encoding="utf-8")
            colors = um.read_color_scheme(path)
            self.assertEqual(colors["background"], "#1c1c1e")
            self.assertEqual(colors["text"], "#f5f5f7")
            self.assertEqual(colors["subtext"], "#98989d")
            self.assertEqual(colors["accent"], "#0a84ff")
            self.assertEqual(colors["critical"], "#ff453a")
            self.assertEqual(colors["track"], "#3c3c3c")

    def test_read_color_scheme_ignores_unusable_files(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "broken.colors"
            path.write_text("[Colors:Button]\nBackgroundNormal=1,2,3\n", encoding="utf-8")
            self.assertEqual(um.read_color_scheme(path), {})

    def _install_scheme(self, data_home: Path) -> None:
        schemes = data_home / "color-schemes"
        schemes.mkdir(parents=True)
        (schemes / "Mojave.colors").write_text(SCHEME_FILE, encoding="utf-8")
        theme_dir = data_home / "plasma" / "desktoptheme" / "whitesur"
        theme_dir.mkdir(parents=True)
        (theme_dir / "colors").write_text(SCHEME_FILE, encoding="utf-8")
        (theme_dir / "metadata.json").write_text(json.dumps({"KPlugin": {"Name": "WhiteSur"}}), encoding="utf-8")

    def test_installed_schemes_finds_color_schemes_and_desktop_themes(self):
        with tempfile.TemporaryDirectory() as td:
            self._install_scheme(Path(td))
            with mock.patch.dict(os.environ, {"XDG_DATA_HOME": td, "XDG_DATA_DIRS": td}, clear=True):
                schemes = um.installed_schemes()
            ids = [s["id"] for s in schemes]
            self.assertIn("colors:Mojave", ids)
            self.assertIn("desktoptheme:whitesur", ids)
            mojave = next(s for s in schemes if s["id"] == "colors:Mojave")
            self.assertEqual(mojave["name"], "Test Mojave")
            self.assertTrue(mojave["dark"])
            self.assertEqual(mojave["colors"]["background"], "#1c1c1e")

    def test_scheme_mode_resolves_installed_scheme(self):
        with tempfile.TemporaryDirectory() as td:
            self._install_scheme(Path(td))
            with mock.patch.dict(os.environ, {"XDG_DATA_HOME": td, "XDG_DATA_DIRS": td}, clear=True):
                theme = um.resolve_theme({"themeMode": "scheme", "themeScheme": "colors:Mojave"})
            self.assertEqual(theme["mode"], "scheme")
            self.assertEqual(theme["name"], "Test Mojave")
            self.assertEqual(theme["colors"]["background"], "#1c1c1e")

    def test_removed_scheme_falls_back_to_the_desktop_theme(self):
        with tempfile.TemporaryDirectory() as td, \
             mock.patch.dict(os.environ, {"XDG_DATA_HOME": td, "XDG_DATA_DIRS": td}, clear=True):
            theme = um.resolve_theme({"themeMode": "scheme", "themeScheme": "colors:Gone"})
            self.assertEqual(theme["mode"], "plasma")

    def test_custom_themes_are_named_and_sanitized(self):
        raw = json.dumps([
            {"id": "t1", "name": "My Nord", "base": "nord",
             "colors": {"background": "#20242c", "accent": "javascript:alert(1)"},
             "font": {"family": "Fira Sans", "size": "11"},
             "metrics": {"barHeight": 10, "opacity": 0.9}},
            {"id": "t1", "name": "duplicate id, dropped"},
            {"name": "no id", "base": "nope"},
            "not a theme",
        ])
        themes = um.custom_themes({"customThemes": raw})
        self.assertEqual([t["id"] for t in themes], ["t1", "custom-3"])
        nord = themes[0]
        self.assertEqual(nord["name"], "My Nord")
        self.assertEqual(nord["colors"]["background"], "#20242c")
        # Junk colours never reach QML.
        self.assertNotIn("accent", nord["colors"])
        self.assertEqual(nord["font"], {"family": "Fira Sans", "size": 11.0, "headingSize": 0.0, "smallSize": 0.0})
        self.assertEqual(nord["metrics"]["barHeight"], 10.0)
        # An unknown base falls back to the default built-in.
        self.assertEqual(themes[1]["base"], um.DEFAULT_BUILTIN)

    def test_custom_themes_ignores_invalid_json(self):
        self.assertEqual(um.custom_themes({"customThemes": "{not json"}), [])

    def test_resolve_custom_picks_the_selected_theme(self):
        raw = json.dumps([
            {"id": "t1", "name": "Dark one", "base": "nord", "colors": {"background": "#101010"}},
            {"id": "t2", "name": "Light one", "base": "macos-light", "colors": {}},
        ])
        theme = um.resolve_theme({"themeMode": "custom", "customThemes": raw, "themeCustomId": "t2"})
        self.assertEqual(theme["id"], "t2")
        self.assertEqual(theme["name"], "Light one")
        self.assertEqual(theme["colors"]["background"], "#f5f5f7")
        self.assertFalse(theme["dark"])
        # Colours not overridden come from the theme's own base.
        first = um.resolve_theme({"themeMode": "custom", "customThemes": raw, "themeCustomId": "gone"})
        self.assertEqual(first["id"], "t1")
        self.assertEqual(first["colors"]["background"], "#101010")
        self.assertEqual(first["colors"]["text"], um.BUILTIN_THEMES["nord"]["colors"]["text"])

    def test_resolve_custom_without_themes_uses_the_base(self):
        theme = um.resolve_theme({"themeMode": "custom", "themeBuiltin": "dracula"})
        self.assertEqual(theme["mode"], "custom")
        self.assertEqual(theme["colors"], um.BUILTIN_THEMES["dracula"]["colors"])

    def test_legacy_flat_custom_keys_become_a_named_theme(self):
        sf = {
            "themeMode": "custom",
            "themeBuiltin": "macos-light",
            "themeCustomBackground": "#fff8e7",
            "themeCustomFontFamily": "Fira Sans",
        }
        themes = um.custom_themes(sf)
        self.assertEqual([t["id"] for t in themes], ["custom"])
        self.assertEqual(themes[0]["name"], "Custom")
        self.assertEqual(themes[0]["base"], "macos-light")
        self.assertEqual(themes[0]["colors"]["background"], "#fff8e7")
        # And it stays the palette the widget renders.
        self.assertEqual(um.resolve_theme(sf)["colors"]["background"], "#fff8e7")

    def test_no_legacy_theme_when_nothing_was_customised(self):
        self.assertEqual(um.custom_themes({"themeMode": "custom"}), [])

    def test_non_finite_numbers_never_reach_the_payload(self):
        raw = json.dumps([{"id": "t1", "base": "nord",
                           "metrics": {"barHeight": "inf", "radius": "nan", "opacity": "-inf"},
                           "font": {"size": "nan"}}])
        theme = um.resolve_theme({"themeMode": "custom", "customThemes": raw})
        self.assertEqual(theme["metrics"], {"barHeight": 6, "radius": 4, "opacity": 1.0})
        self.assertEqual(theme["font"]["size"], 0.0)
        # json.dumps would emit bare NaN/Infinity, which QML's JSON.parse rejects.
        json.loads(json.dumps(theme, allow_nan=False))

    def test_deleting_the_last_theme_does_not_restore_the_legacy_one(self):
        legacy = {"themeMode": "custom", "themeCustomBackground": "#fff8e7"}
        # Key absent -> the old flat keys are still migrated.
        self.assertEqual(um.resolve_theme(legacy)["colors"]["background"], "#fff8e7")
        # Explicit empty list -> the user deleted their themes; stay deleted.
        emptied = {**legacy, "customThemes": "[]"}
        self.assertEqual(um.custom_themes(emptied), [])
        self.assertNotEqual(um.resolve_theme(emptied)["colors"]["background"], "#fff8e7")
        # Unparsable value still falls back rather than losing the palette.
        self.assertEqual(len(um.custom_themes({**legacy, "customThemes": "{oops"})), 1)

    def test_find_scheme_resolves_by_id_without_scanning(self):
        with tempfile.TemporaryDirectory() as td:
            self._install_scheme(Path(td))
            with mock.patch.dict(os.environ, {"XDG_DATA_HOME": td, "XDG_DATA_DIRS": td}, clear=True), \
                 mock.patch.object(um, "installed_schemes", side_effect=AssertionError("must not scan")):
                entry = um.find_scheme("colors:Mojave")
                self.assertEqual(entry["name"], "Test Mojave")
                self.assertEqual(entry["colors"]["background"], "#1c1c1e")
                self.assertEqual(um.find_scheme("desktoptheme:whitesur")["name"], "WhiteSur")
                # And the render path uses it, so a refresh tick never scans.
                theme = um.resolve_theme({"themeMode": "scheme", "themeScheme": "colors:Mojave"})
                self.assertEqual(theme["colors"]["background"], "#1c1c1e")

    def test_find_scheme_rejects_traversal_and_unknown_kinds(self):
        for bad in ("colors:../../etc/passwd", "colors:", "bogus:Mojave", ""):
            self.assertIsNone(um.find_scheme(bad))

    def test_catalog_builtins_carry_font_and_metrics(self):
        # The settings preview mirrors resolve_theme from these.
        macos = next(t for t in um.theme_catalog(schemes=[])["builtin"] if t["id"] == "macos-dark")
        self.assertEqual(macos["metrics"]["radius"], um.BUILTIN_THEMES["macos-dark"]["metrics"]["radius"])
        self.assertIn("family", macos["font"])

    def test_catalog_lists_custom_themes(self):
        raw = json.dumps([{"id": "t1", "name": "Mine", "base": "nord"}])
        catalog = um.theme_catalog(schemes=[], sf={"customThemes": raw})
        self.assertEqual([(t["id"], t["name"]) for t in catalog["custom"]], [("t1", "Mine")])

    def test_catalog_lists_builtins_and_schemes(self):
        catalog = um.theme_catalog(schemes=[{"id": "colors:X", "name": "X"}])
        self.assertEqual([t["id"] for t in catalog["builtin"]], list(um.BUILTIN_THEMES))
        self.assertEqual(catalog["schemes"][0]["id"], "colors:X")
        self.assertEqual(catalog["colorKeys"], list(um.THEME_COLOR_KEYS))

    def test_theme_survives_a_state_roundtrip(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
            um._write_state({"themeMode": "builtin", "themeBuiltin": "dracula"})
            self.assertEqual(um.resolve_theme()["colors"]["background"], "#282a36")


class CommandTests(unittest.TestCase):
    def test_set_provider_uses_top_level_enable(self):
        with mock.patch.object(um, "run_cli", return_value=proc()) as run, \
             mock.patch.object(um, "command_settings", return_value=0):
            args = mock.Mock(provider="codex", enabled="true")
            um.command_set_provider(args)
            self.assertEqual(run.call_args.args[0], ["enable", "codex"])

    def test_set_provider_disable(self):
        with mock.patch.object(um, "run_cli", return_value=proc()) as run, \
             mock.patch.object(um, "command_settings", return_value=0):
            args = mock.Mock(provider="claude", enabled="false")
            um.command_set_provider(args)
            self.assertEqual(run.call_args.args[0], ["disable", "claude"])

    def test_cache_fallback_stale(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {"XDG_CACHE_HOME": td, "XDG_CONFIG_HOME": td}, clear=True):
                um.write_json(um.paths().last_good, [
                    {"provider": "codex", "displayName": "Codex", "usage": {"primary": {"usedPercent": 5}}}
                ])
                payload = um.summarize(
                    [dict(e, stale=True) for e in um.load_json(um.paths().last_good, [])]
                )
                self.assertEqual(payload["class"], "stale")
                self.assertEqual(payload["text"], "5%")


class AuthMetadataTests(unittest.TestCase):
    def test_auth_kinds(self):
        self.assertEqual(um.provider_auth("openai")["kind"], "api_key")
        self.assertEqual(um.provider_auth("grok")["kind"], "token")
        self.assertEqual(um.provider_auth("abacus")["kind"], "cookie")
        self.assertEqual(um.provider_auth("codex")["kind"], "oauth")
        self.assertEqual(um.provider_auth("opencode-go")["kind"], "opencode")
        # Unknown providers default to an API-key form.
        self.assertEqual(um.provider_auth("brand-new")["kind"], "api_key")

    def test_oauth_has_setup_hint_and_credentials_field(self):
        codex = um.provider_auth("codex")
        self.assertTrue(codex["setupHint"])
        self.assertEqual([f["key"] for f in codex["fields"]], ["credentials_path"])

    def test_list_workspaces_parses_lines(self):
        out = "wrk_a   Alpha\nwrk_b\n(no workspaces configured)\n"
        with mock.patch.object(um, "cli_output", return_value=out):
            ws = um.list_workspaces()
            self.assertEqual(ws, [{"id": "wrk_a", "name": "Alpha"}, {"id": "wrk_b", "name": ""}])


class ManageCommandTests(unittest.TestCase):
    def test_account_save_adds_then_sets_each_field(self):
        with mock.patch.object(um, "run_cli", return_value=proc()) as run:
            args = SimpleNamespace(provider="openai", name="work", label="Work", json='{"api_key":"sk-x","base_url":""}')
            um.command_account_save(args)
            calls = [c.args[0] for c in run.call_args_list]
            self.assertEqual(calls[0], ["openai", "account", "add", "work", "--label", "Work"])
            # Empty values are skipped; only api_key is set.
            self.assertEqual(calls[1], ["openai", "account", "set", "work", "api_key", "sk-x"])
            self.assertEqual(len(calls), 2)

    def test_account_remove(self):
        with mock.patch.object(um, "run_cli", return_value=proc()) as run:
            um.command_account_remove(SimpleNamespace(provider="openai", name="work"))
            self.assertEqual(run.call_args.args[0], ["openai", "account", "remove", "work"])

    def test_workspace_add_with_name(self):
        with mock.patch.object(um, "run_cli", return_value=proc()) as run:
            um.command_workspace_add(SimpleNamespace(workspace="wrk_a", name="Alpha", account=""))
            self.assertEqual(run.call_args.args[0], ["opencode-go", "workspace", "add", "wrk_a", "Alpha"])

    def test_workspace_remove(self):
        with mock.patch.object(um, "run_cli", return_value=proc()) as run:
            um.command_workspace_remove(SimpleNamespace(workspace="wrk_a", account=""))
            self.assertEqual(run.call_args.args[0], ["opencode-go", "workspace", "remove", "wrk_a"])


if __name__ == "__main__":
    unittest.main()
