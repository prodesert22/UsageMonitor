"""Tests for the Waybar popup data/theme helper.

Loaded straight from the CLI asset tree (single source of truth, embedded by the
installer) like the Waybar wrapper tests.
"""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

_ASSET_DIR = Path(__file__).resolve().parents[3] / "usage-monitor-cli" / "assets" / "waybar"


def _load_module():
    spec = importlib.util.spec_from_file_location(
        "usage_monitor_waybar_data", _ASSET_DIR / "usage_monitor_waybar_data.py"
    )
    module = importlib.util.module_from_spec(spec)
    # Registered before exec: the module defines a dataclass, and dataclasses
    # resolve annotations through sys.modules.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


data = _load_module()

_KDEGLOBALS = """
[Colors:Window]
BackgroundNormal=35,38,41
ForegroundNormal=252,252,252
ForegroundInactive=161,169,177
DecorationFocus=61,174,233
ForegroundNeutral=246,116,0
ForegroundNegative=218,68,83

[Colors:Button]
BackgroundNormal=49,54,59
"""


class PathTests(unittest.TestCase):
    def test_state_and_cache_live_under_the_waybar_namespace(self):
        with tempfile.TemporaryDirectory() as td:
            env = {"XDG_CONFIG_HOME": f"{td}/config", "XDG_CACHE_HOME": f"{td}/cache", "HOME": td}
            with mock.patch.dict(os.environ, env, clear=True):
                paths = data.paths()
        # Independent from the KDE widget's own state, so both can be installed.
        self.assertTrue(str(paths.state).endswith("config/usage-monitor-waybar/state.json"))
        self.assertTrue(str(paths.last_good).endswith("cache/usage-monitor-waybar/last.json"))

    def test_state_round_trips_through_set_state_keys(self):
        with tempfile.TemporaryDirectory() as td:
            state = Path(td) / "state.json"
            data.set_state_key("barProvider", "codex", state)
            data.set_state_keys([("refreshIntervalSeconds", "60"), ("themeMode", "builtin")], state)
            self.assertEqual(
                data.state_full(state),
                {"barProvider": "codex", "refreshIntervalSeconds": "60", "themeMode": "builtin"},
            )


class BinaryDiscoveryTests(unittest.TestCase):
    def test_env_override_wins(self):
        self.assertEqual(data.usage_monitor_binary({"USAGE_MONITOR_BIN": "/opt/um"}), "/opt/um")

    def test_falls_back_to_the_plain_name(self):
        with mock.patch.object(data, "which", return_value=None), \
             mock.patch.object(data.Path, "exists", return_value=False):
            self.assertEqual(data.usage_monitor_binary({}), "usage-monitor-cli")

    def test_search_dirs_cover_non_arch_layouts(self):
        # A compositor-launched process gets a minimal PATH, so these prefixes
        # (Nix, FreeBSD/source installs, snap) must stay in the fallback list.
        dirs = {str(path) for path in data._SEARCH_DIRS}
        self.assertIn("/usr/local/bin", dirs)
        self.assertIn("/run/current-system/sw/bin", dirs)
        self.assertIn("/snap/bin", dirs)


class ThemeTests(unittest.TestCase):
    def _system_env(self, td: str, dark: str = "1") -> dict[str, str]:
        return {"XDG_CONFIG_HOME": td, "HOME": td, "USAGE_MONITOR_DARK": dark, "XDG_DATA_DIRS": td}

    def test_default_mode_is_system(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._system_env(td), clear=True):
            theme = data.resolve_theme({})
        self.assertEqual(theme["mode"], "system")
        # Unlike the Plasma widget, "follow the desktop" must emit real colors:
        # plain Qt Quick has no Kirigami palette to fall back to.
        self.assertTrue(theme["colors"]["background"].startswith("#"))
        self.assertTrue(theme["colors"]["text"].startswith("#"))

    def test_system_mode_uses_kdeglobals_when_the_session_has_one(self):
        with tempfile.TemporaryDirectory() as td:
            (Path(td) / "kdeglobals").write_text(_KDEGLOBALS, encoding="utf-8")
            with mock.patch.dict(os.environ, self._system_env(td), clear=True):
                theme = data.resolve_theme({"themeMode": "system"})
        self.assertEqual(theme["colors"]["background"], "#232629")
        self.assertEqual(theme["colors"]["accent"], "#3daee9")
        self.assertIn("KDE", theme["name"])

    def test_system_mode_follows_the_dark_preference_without_kde(self):
        with tempfile.TemporaryDirectory() as td:
            with mock.patch.dict(os.environ, self._system_env(td, dark="0"), clear=True):
                light = data.resolve_theme({"themeMode": "system"})
            with mock.patch.dict(os.environ, self._system_env(td, dark="1"), clear=True):
                dark = data.resolve_theme({"themeMode": "system"})
        self.assertFalse(light["dark"])
        self.assertEqual(light["colors"], data.BUILTIN_THEMES["macos-light"]["colors"])
        self.assertTrue(dark["dark"])
        self.assertEqual(dark["colors"], data.BUILTIN_THEMES["macos-dark"]["colors"])

    def test_prefers_dark_reads_the_gtk_settings_file(self):
        with tempfile.TemporaryDirectory() as td:
            gtk = Path(td) / "gtk-3.0"
            gtk.mkdir()
            (gtk / "settings.ini").write_text(
                "[Settings]\ngtk-application-prefer-dark-theme=0\ngtk-theme-name=Adwaita\n", encoding="utf-8"
            )
            env = {"XDG_CONFIG_HOME": td, "HOME": td}
            with mock.patch.dict(os.environ, env, clear=True), \
                 mock.patch.object(data, "_gsettings_value", return_value=""):
                self.assertFalse(data.prefers_dark())

    def test_unknown_mode_falls_back_to_system(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._system_env(td), clear=True):
            self.assertEqual(data.resolve_theme({"themeMode": "plasma"})["mode"], "system")

    def test_builtin_mode_emits_that_palette(self):
        theme = data.resolve_theme({"themeMode": "builtin", "themeBuiltin": "nord"})
        self.assertEqual(theme["id"], "nord")
        self.assertEqual(theme["colors"]["background"], "#2e3440")

    def test_missing_scheme_falls_back_to_the_desktop_colors(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._system_env(td), clear=True):
            theme = data.resolve_theme({"themeMode": "scheme", "themeScheme": "colors:does-not-exist"})
        self.assertEqual(theme["mode"], "system")

    def test_custom_theme_overrides_its_base(self):
        custom = [{"id": "mine", "name": "Mine", "base": "nord", "colors": {"accent": "#ff00ff"}}]
        theme = data.resolve_theme({
            "themeMode": "custom",
            "themeCustomId": "mine",
            "customThemes": json.dumps(custom),
        })
        self.assertEqual(theme["colors"]["accent"], "#ff00ff")
        self.assertEqual(theme["colors"]["background"], "#2e3440")

    def test_transparency_applies_to_every_mode(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._system_env(td), clear=True):
            theme = data.resolve_theme({"themeOpacity": "0.4"})
        self.assertAlmostEqual(theme["metrics"]["opacity"], 0.4)

    def test_color_value_rejects_values_qml_cannot_parse(self):
        self.assertEqual(data.color_value("#ABC"), "#abc")
        self.assertEqual(data.color_value("35,38,41"), "#232629")
        self.assertEqual(data.color_value("#abcd"), "")
        self.assertEqual(data.color_value("rebeccapurple"), "")

    def test_catalog_carries_the_system_entry_for_the_settings_preview(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._system_env(td), clear=True):
            catalog = data.theme_catalog([], {})
        self.assertIn("system", catalog)
        self.assertTrue(catalog["system"]["colors"])
        self.assertEqual([t["id"] for t in catalog["builtin"]][:1], ["macos-dark"])


class FetchTests(unittest.TestCase):
    def _proc(self, payload: dict) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess([], 0, json.dumps(payload), "")

    def test_fetch_uses_the_waybar_widget_payload(self):
        calls: list[list[str]] = []

        def runner(args):
            calls.append(args)
            return self._proc({"providers": [{
                "provider_id": "codex",
                "display_name": "Codex",
                "account_email": "dev@example.com",
                "max_percentage": 42,
                "windows": [
                    {"id": "primary", "percentage": 42, "resets_at": "in 3h"},
                    {"id": "secondary", "percentage": 12, "resets_at": ""},
                ],
            }]})

        entries = data.fetch_entries(runner)
        self.assertEqual(calls, [["widget", "waybar"]])
        self.assertEqual(entries[0]["provider"], "codex")
        self.assertEqual(entries[0]["usage"]["primary"]["usedPercent"], 42.0)
        self.assertEqual(entries[0]["usage"]["identity"]["accountEmail"], "dev@example.com")

    def test_cli_failure_becomes_an_error_entry(self):
        def runner(args):
            return subprocess.CompletedProcess(args, 1, "", "boom")

        entries = data.fetch_entries(runner)
        self.assertEqual(entries[0]["error"]["message"], "boom")

    def test_summarize_classifies_and_labels(self):
        entries = [{
            "provider": "codex",
            "usage": {"primary": {"usedPercent": 91.0, "resetDescription": "in 3h"}},
        }]
        summary = data.summarize(entries)
        self.assertEqual(summary["class"], "critical")
        self.assertEqual(summary["text"], "91%")
        self.assertIn("Codex session: 91% — in 3h", summary["tooltip"])

    def test_pinned_provider_drives_the_summary_text(self):
        entries = [
            {"provider": "codex", "usage": {"primary": {"usedPercent": 20.0}, "secondary": {"usedPercent": 30.0}}},
            {"provider": "claude", "usage": {"primary": {"usedPercent": 80.0}}},
        ]
        self.assertEqual(data.summarize(entries, "codex")["text"], "20% • 30%")

    def test_pinned_account_drives_the_summary_text(self):
        entries = [
            {"provider": "codex", "account": "",
             "usage": {"primary": {"usedPercent": 10.0}, "secondary": {"usedPercent": 98.0}}},
            {"provider": "codex", "account": "plus2",
             "usage": {"primary": {"usedPercent": 0.0}, "secondary": {"usedPercent": 5.0}}},
        ]
        self.assertEqual(data.summarize(entries, "codex/plus2")["text"], "0% • 5%")
        # Legacy provider-level pin still resolves to the implicit default.
        self.assertEqual(data.summarize(entries, "codex")["text"], "10% • 98%")
        # Unknown account falls back to the max across providers.
        self.assertEqual(data.summarize(entries, "codex/ghost")["text"], "98%")

    def test_pin_key_for_entry(self):
        self.assertEqual(data.pin_key_for_entry({"provider": "codex", "account": ""}), "codex")
        self.assertEqual(data.pin_key_for_entry({"provider": "codex"}), "codex")
        self.assertEqual(data.pin_key_for_entry({"provider": "codex", "account": "plus2"}), "codex/plus2")

    def test_pinnable_targets_lists_each_active_account(self):
        accounts = [
            {"id": "default", "label": "(auto-detected)", "active": "true"},
            {"id": "plus2", "label": "Plus 2", "active": "true"},
            {"id": "old", "label": "Old", "active": "false"},
        ]
        targets = data.pinnable_targets("codex", "Codex", accounts)
        self.assertEqual([t["id"] for t in targets], ["codex", "codex/plus2"])
        self.assertEqual(targets[1]["displayName"], "Codex — Plus 2")

    def test_pinnable_targets_lone_login_stays_provider_level(self):
        accounts = [{"id": "default", "label": "(auto-detected)", "active": "true"}]
        self.assertEqual(
            data.pinnable_targets("codex", "Codex", accounts),
            [{"id": "codex", "displayName": "Codex"}],
        )

    def test_extra_windows_do_not_fill_monthly_slot(self):
        payload = {"providers": [{"provider_id": "codex", "windows": [
            {"id": "primary", "percentage": 10.0},
            {"id": "secondary", "percentage": 98.0},
            {"id": "Additional", "label": "Additional", "percentage": 0.0},
        ]}]}

        def runner(args):
            return subprocess.CompletedProcess(args, 0, json.dumps(payload), "")

        entries = data.fetch_entries(runner)
        self.assertNotIn("tertiary", entries[0]["usage"])
        summary = data.summarize(entries, "codex")
        self.assertEqual(summary["text"], "10% • 98%")
        self.assertIn("Codex additional: 0%", summary["tooltip"])
        self.assertEqual(summary["pinnedPercent"], 98.0)

    def test_labeled_windows_keep_real_names(self):
        # Antigravity weekly buckets: the card/tooltip show "Gemini weekly",
        # not the generic slot names.
        payload = {"providers": [{"provider_id": "antigravity", "windows": [
            {"id": "primary", "label": "Gemini weekly", "percentage": 3.0},
            {"id": "secondary", "label": "Claude/GPT weekly", "percentage": 0.0},
        ]}]}

        def runner(args):
            return subprocess.CompletedProcess(args, 0, json.dumps(payload), "")

        entries = data.fetch_entries(runner)
        self.assertEqual(entries[0]["usage"]["primary"]["label"], "Gemini weekly")
        self.assertEqual(entries[0]["usage"]["secondary"]["label"], "Claude/GPT weekly")
        summary = data.summarize(entries, "antigravity")
        self.assertEqual(summary["text"], "3% • 0%")
        self.assertIn("Antigravity gemini weekly: 3%", summary["tooltip"])
        self.assertIn("Antigravity claude/gpt weekly: 0%", summary["tooltip"])

    def test_bar_text_respects_enabled_windows(self):
        entries = [
            {"provider": "kimi", "account": "",
             "usage": {"primary": {"usedPercent": 28.0},
                       "secondary": {"usedPercent": 46.0},
                       "tertiary": {"usedPercent": 31.0}}},
        ]
        self.assertEqual(
            data.bar_text(entries, "kimi", ["primary", "secondary"]), "28% • 46%"
        )
        self.assertEqual(data.bar_text(entries, "kimi", ["tertiary"]), "31%")
        self.assertEqual(data.pinned_percent(entries, "kimi", ["tertiary"]), 31.0)
        self.assertIsNone(data.pinned_percent(entries, ""))

    def test_enabled_bar_windows_reads_state(self):
        self.assertEqual(data.enabled_bar_windows({}), ["primary", "secondary", "tertiary"])
        self.assertEqual(data.enabled_bar_windows({"barMonthly": "false"}), ["primary", "secondary"])

    def test_merge_with_cache_keeps_two_accounts_separate(self):
        with tempfile.TemporaryDirectory() as td:
            cache = Path(td) / "last.json"
            good = [
                {"provider": "codex", "account": "",
                 "usage": {"primary": {"usedPercent": 10.0}}},
                {"provider": "codex", "account": "plus2",
                 "usage": {"primary": {"usedPercent": 5.0}}},
            ]
            data.merge_with_cache(good, ["codex", "codex/plus2"], cache)
            self.assertEqual(len(json.loads(cache.read_text())), 2)
            failed = [
                {"provider": "codex", "account": "", "error": {"message": "nope"}},
                {"provider": "codex", "account": "plus2", "error": {"message": "nope"}},
            ]
            merged = data.merge_with_cache(failed, ["codex", "codex/plus2"], cache)
            self.assertEqual(len(merged), 2)
            by_account = {m["account"]: m for m in merged}
            self.assertEqual(by_account[""]["usage"]["primary"]["usedPercent"], 10.0)
            self.assertEqual(by_account["plus2"]["usage"]["primary"]["usedPercent"], 5.0)

    def test_merge_with_cache_marks_failed_providers_stale(self):
        with tempfile.TemporaryDirectory() as td:
            cache = Path(td) / "last.json"
            good = [{"provider": "codex", "usage": {"primary": {"usedPercent": 10.0}}}]
            data.merge_with_cache(good, ["codex"], cache)
            merged = data.merge_with_cache([{"provider": "codex", "error": {"message": "nope"}}], ["codex"], cache)
        self.assertTrue(merged[0]["stale"])
        self.assertEqual(merged[0]["usage"]["primary"]["usedPercent"], 10.0)


class FetchPacingTests(unittest.TestCase):
    """The bar and the popup share one fetched value.

    Two processes on 30 s timers was enough to get Claude to answer "rate
    limited", which turned every provider into an error and the bar into "⚠".
    """

    def _env(self, td: str) -> dict[str, str]:
        return {"XDG_CONFIG_HOME": f"{td}/config", "XDG_CACHE_HOME": f"{td}/cache",
                "HOME": td, "XDG_DATA_DIRS": td, "USAGE_MONITOR_DARK": "1"}

    def _entry(self, percent: float) -> list[dict]:
        return [{"provider": "codex", "usage": {"primary": {"usedPercent": percent}}}]

    def test_second_call_inside_the_interval_serves_the_cache(self):
        calls = []

        def fetch():
            calls.append(1)
            return self._entry(10.0)

        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True), \
             mock.patch.object(data, "fetch_entries", side_effect=lambda *a, **k: fetch()):
            first = data.summary_payload()
            second = data.summary_payload()
        self.assertEqual(len(calls), 1)
        self.assertFalse(first["cached"])
        self.assertTrue(second["cached"])
        self.assertEqual(second["text"], "10%")

    def test_force_always_fetches(self):
        calls = []
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True), \
             mock.patch.object(data, "fetch_entries", side_effect=lambda *a, **k: (calls.append(1), self._entry(5.0))[1]):
            data.summary_payload()
            data.summary_payload(force=True)
        self.assertEqual(len(calls), 2)

    def test_a_failed_fetch_still_paces_the_next_one(self):
        # Retrying a rate-limited endpoint every tick is what keeps it
        # rate-limited, so failures reset the timer too.
        calls = []

        def failing():
            calls.append(1)
            return [{"provider": "codex", "error": {"message": "Rate limited"}}]

        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True), \
             mock.patch.object(data, "fetch_entries", side_effect=lambda *a, **k: failing()):
            data.summary_payload()
            data.summary_payload()
        self.assertEqual(len(calls), 1)

    def test_interval_is_configurable_and_zero_disables_pacing(self):
        calls = []
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True), \
             mock.patch.object(data, "fetch_entries", side_effect=lambda *a, **k: (calls.append(1), self._entry(5.0))[1]):
            data.set_state_key("minFetchIntervalSeconds", "0")
            data.summary_payload()
            data.summary_payload()
        self.assertEqual(len(calls), 2)

    def test_min_fetch_interval_defaults_and_rejects_junk(self):
        self.assertEqual(data.min_fetch_interval({}), data.DEFAULT_MIN_FETCH_SECONDS)
        self.assertEqual(data.min_fetch_interval({"minFetchIntervalSeconds": "300"}), 300.0)
        self.assertEqual(data.min_fetch_interval({"minFetchIntervalSeconds": "nope"}),
                         data.DEFAULT_MIN_FETCH_SECONDS)

    def test_a_busy_lock_serves_the_cache_instead_of_a_second_fetch(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True), \
             mock.patch.object(data, "fetch_entries", side_effect=lambda *a, **k: self._entry(7.0)):
            data.summary_payload()  # seed the cache
            held = data._fetch_lock(data.paths().lock)
            self.assertIsNotNone(held)
            try:
                with mock.patch.object(data, "fetch_entries", side_effect=AssertionError("must not fetch")):
                    payload = data.summary_payload(force=True)
            finally:
                data._release_lock(held)
        self.assertTrue(payload["cached"])
        self.assertEqual(payload["text"], "7%")

    def test_cache_survives_a_provider_failure_as_stale(self):
        # The bar must keep showing the last good percentage instead of "⚠".
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            with mock.patch.object(data, "fetch_entries", side_effect=lambda *a, **k: self._entry(44.0)):
                data.summary_payload()
            with mock.patch.object(
                data, "fetch_entries",
                side_effect=lambda *a, **k: [{"provider": "codex", "error": {"message": "boom"}}],
            ):
                payload = data.summary_payload(force=True)
        self.assertEqual(payload["text"], "44%")
        self.assertNotEqual(payload["text"], "⚠")
        self.assertTrue(payload["providers"][0]["stale"])


class SessionTests(unittest.TestCase):
    def test_session_type_prefers_the_explicit_variable(self):
        with mock.patch.dict(os.environ, {"XDG_SESSION_TYPE": "x11"}, clear=True):
            self.assertEqual(data.session_type(), "x11")
        with mock.patch.dict(os.environ, {"WAYLAND_DISPLAY": "wayland-0"}, clear=True):
            self.assertEqual(data.session_type(), "wayland")
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertEqual(data.session_type(), "")

    def test_desktop_environment_detects_bare_compositors(self):
        with mock.patch.dict(os.environ, {"XDG_CURRENT_DESKTOP": "Hyprland"}, clear=True):
            self.assertEqual(data.desktop_environment(), "hyprland")
        with mock.patch.dict(os.environ, {"SWAYSOCK": "/run/sway.sock"}, clear=True):
            self.assertEqual(data.desktop_environment(), "sway")


class UpdateTests(unittest.TestCase):
    def _env(self, td):
        return {"XDG_CONFIG_HOME": f"{td}/config", "XDG_CACHE_HOME": f"{td}/cache", "HOME": td}

    def _proc(self, stdout="", returncode=0, stderr=""):
        return subprocess.CompletedProcess(["usage-monitor-cli"], returncode, stdout, stderr)

    def test_compare_versions_orders_releases(self):
        self.assertLess(data.compare_versions("0.8.0", "0.8.1"), 0)
        self.assertEqual(data.compare_versions("0.8.1", "0.8.1"), 0)
        self.assertEqual(data.compare_versions("0.8", "0.8.0"), 0)
        self.assertGreater(data.compare_versions("v0.9", "0.8.1"), 0)

    def test_is_outdated_ignores_missing_and_unknown(self):
        self.assertFalse(data.is_outdated("", "0.8.1"))
        self.assertFalse(data.is_outdated(None, "0.8.1"))
        self.assertFalse(data.is_outdated("0.8.0", "unknown"))
        self.assertFalse(data.is_outdated("0.8.1", "0.8.1"))
        self.assertFalse(data.is_outdated("0.9.0", "0.8.1"))
        self.assertTrue(data.is_outdated("0.8.0", "0.8.1"))

    def test_update_info_reports_outdated_with_release_url(self):
        info = data.update_info("0.8.0", "0.8.1", {})
        self.assertTrue(info["outdated"])
        self.assertFalse(info["dismissed"])
        self.assertIn("/releases/tag/v0.8.1", info["url"])

    def test_update_info_honours_dismissal(self):
        info = data.update_info("0.8.0", "0.8.1", {"dismissedUpdateVersion": "0.8.1"})
        self.assertTrue(info["outdated"])
        self.assertTrue(info["dismissed"])
        current = data.update_info("0.8.1", "0.8.1", {})
        self.assertFalse(current["outdated"])
        self.assertEqual(current["url"], "")

    def _write_release_cache(self, version, age_seconds, payload):
        from datetime import datetime, timezone
        stamp = datetime.now(timezone.utc).timestamp() - age_seconds
        iso = datetime.fromtimestamp(stamp, timezone.utc).isoformat().replace("+00:00", "Z")
        data.write_json(data._release_cache_path(version), {"fetchedAt": iso, "payload": payload})

    def test_fetch_changelog_serves_fresh_cache_without_spawning(self):
        cached = {"version": "0.8.1", "url": "u", "body": "notes", "source": "github"}
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            self._write_release_cache("0.8.1", 60, cached)
            runner = mock.Mock(side_effect=AssertionError("must not spawn"))
            self.assertEqual(data.fetch_changelog("0.8.1", runner=runner), cached)

    def test_fetch_changelog_stores_successful_fetch(self):
        body = {"version": "0.8.1", "url": "u", "body": "notes", "source": "github"}
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            runner = lambda args: self._proc(json.dumps(body))
            self.assertEqual(data.fetch_changelog("0.8.1", runner=runner), body)
            # Second call is served from the cache just written.
            failing = mock.Mock(side_effect=AssertionError("must use cache"))
            self.assertEqual(data.fetch_changelog("0.8.1", runner=failing), body)

    def test_fetch_changelog_falls_back_to_stale_cache_then_unavailable(self):
        stale = {"version": "0.8.1", "url": "u", "body": "old", "source": "github"}
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            dead = lambda args: self._proc(returncode=1, stderr="offline")
            # No cache at all: report unavailable with the release page URL.
            missing = data.fetch_changelog("0.8.1", runner=dead)
            self.assertEqual(missing["source"], "unavailable")
            self.assertIn("/releases/tag/v0.8.1", missing["url"])
            # A stale cache beats "unavailable" when the fetch fails.
            self._write_release_cache("0.8.1", 48 * 3600, stale)
            self.assertEqual(data.fetch_changelog("0.8.1", runner=dead), stale)

    def test_apply_update_runs_widget_install_for_target(self):
        run = mock.Mock(return_value=self._proc("ok"))
        data.apply_update("waybar", runner=run)
        self.assertEqual(run.call_args.args[0], ["widget", "install", "waybar"])

    def test_update_dismiss_records_version_in_state(self):
        import argparse

        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            args = argparse.Namespace(version="0.8.1")
            data.command_update_dismiss(args)
            self.assertEqual(data.state_value(key="dismissedUpdateVersion"), "0.8.1")

    def test_fetch_changelog_survives_runner_crash(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            missing = data.fetch_changelog("0.8.1", runner=mock.Mock(side_effect=FileNotFoundError("no cli")))
            self.assertEqual(missing["source"], "unavailable")
            self.assertIn("/releases/tag/v0.8.1", missing["url"])

    def test_release_cache_path_cannot_escape_cache_dir(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            cache_dir = data.paths().cache_dir
            path = data._release_cache_path("../../evil")
            self.assertEqual(path.parent, cache_dir)
            self.assertNotIn(os.sep, path.name)

    def test_fetch_changelog_caches_body_verbatim(self):
        raw = {"version": "0.8.1", "url": "u", "body": "## Notes\n- item", "source": "release-file"}
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, self._env(td), clear=True):
            runner = lambda args: self._proc(json.dumps(raw))
            fetched = data.fetch_changelog("0.8.1", runner=runner)
            self.assertEqual(fetched["body"], raw["body"])
            again = data.fetch_changelog("0.8.1", runner=mock.Mock(side_effect=AssertionError("cache")))
            self.assertEqual(again["body"], raw["body"])


if __name__ == "__main__":
    unittest.main()
