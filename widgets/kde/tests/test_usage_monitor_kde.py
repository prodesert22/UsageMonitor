import json
import os
import subprocess
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

from widgets.kde.package.contents.code import usage_monitor_kde as um


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
                {"id": "primary", "label": "Session", "percentage": 80, "resets_at": "resets at 14:00"},
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
        with tempfile.TemporaryDirectory() as td:
            with mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
                um._write_state({"barProvider": "codex"})
                self.assertEqual(um.state_value(key="barProvider"), "codex")

    def test_provider_order_parses_json_string(self):
        with tempfile.TemporaryDirectory() as td:
            with mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
                um._write_state({"providerOrder": '["claude","codex"]'})
                self.assertEqual(um._provider_order(), ["claude", "codex"])


class FetchTests(unittest.TestCase):
    def test_fetch_entries_maps_windows_and_identity(self):
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(WIDGET_PAYLOAD)))
        codex = entries[0]
        self.assertEqual(codex["provider"], "codex")
        self.assertEqual(codex["usage"]["primary"]["usedPercent"], 80.0)
        self.assertEqual(codex["usage"]["primary"]["resetDescription"], "resets at 14:00")
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

    def test_summarize_orders_by_state(self):
        entries = um.fetch_entries(runner=lambda args: proc(json.dumps(WIDGET_PAYLOAD)))
        with tempfile.TemporaryDirectory() as td:
            with mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
                um._write_state({"providerOrder": '["claude","codex"]'})
                payload = um.summarize(entries, pinned_provider="")
                self.assertEqual(payload["providers"][0]["provider"], "claude")


class SettingsTests(unittest.TestCase):
    LIST_OUT = (
        "codex        enabled          Codex — ChatGPT plan\n"
        "claude       disabled (auto)  Claude — Claude Pro\n"
    )
    SHOWS = {
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
        with tempfile.TemporaryDirectory() as td:
            with mock.patch.dict(os.environ, {"XDG_CONFIG_HOME": td}, clear=True):
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
        with tempfile.TemporaryDirectory() as td:
            with mock.patch.dict(os.environ, {"XDG_CACHE_HOME": td, "XDG_CONFIG_HOME": td}, clear=True):
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
