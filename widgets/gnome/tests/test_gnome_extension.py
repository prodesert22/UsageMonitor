"""Tests for the GNOME Shell extension.

The extension lives in the CLI asset tree (single source of truth, embedded
by the installer); these tests pin down the packaging contract without a
running Shell: the uuid directory matches metadata, the version tracks the
workspace Cargo.toml (the update banner compares the stamp against the
binary), the GSettings schema is valid and covers every key the JS reads,
and the JS/CSS parses with the host tools when they exist.
"""

import json
import re
import shutil
import subprocess
import tempfile
import unittest
import xml.dom.minidom
from pathlib import Path

_REPO = Path(__file__).resolve().parents[3]
_EXT_DIR = _REPO / "usage-monitor-cli" / "assets" / "gnome" / "usage-monitor@usage-monitor.dev"
_METADATA = _EXT_DIR / "metadata.json"
_SCHEMA = _EXT_DIR / "schemas" / "org.gnome.shell.extensions.usage-monitor.gschema.xml"

EXPECTED_FILES = [
    "metadata.json",
    "extension.js",
    "panel_values.js",
    "provider_settings.js",
    "prefs.js",
    "stylesheet.css",
    "schemas/org.gnome.shell.extensions.usage-monitor.gschema.xml",
    "icons/usage-monitor.png",
]

# Every GSettings key the extension or prefs UI reads (get_*('...')).
SETTINGS_READS = re.compile(r"get_(?:int|string|boolean|double|strv)\('([^']+)'\)")


def _workspace_version() -> str:
    for line in (_REPO / "Cargo.toml").read_text().splitlines():
        m = re.match(r'version\s*=\s*"([^"]+)"', line.strip())
        if m:
            return m.group(1)
    raise AssertionError("version not found in workspace Cargo.toml")


def _schema_keys() -> set[str]:
    doc = xml.dom.minidom.parse(str(_SCHEMA))
    return {k.getAttribute("name") for k in doc.getElementsByTagName("key")}


class GnomePackagingTests(unittest.TestCase):
    def test_asset_tree_is_complete(self):
        for name in EXPECTED_FILES:
            self.assertTrue((_EXT_DIR / name).is_file(), f"missing {name}")

    def test_metadata_contract(self):
        meta = json.loads(_METADATA.read_text())
        self.assertEqual(meta["uuid"], "usage-monitor@usage-monitor.dev")
        self.assertEqual(_EXT_DIR.name, meta["uuid"],
                         "uuid directory must match the metadata uuid")
        self.assertEqual(meta["version"], _workspace_version(),
                         "extension version must track the CLI (update banner compares them)")
        self.assertTrue(meta["shell-version"], "shell-version must list supported Shells")
        self.assertEqual(meta["settings-schema"],
                         "org.gnome.shell.extensions.usage-monitor")

    def test_schema_covers_every_key_the_js_reads(self):
        keys = _schema_keys()
        self.assertTrue(keys, "schema must declare keys")
        for js in ["extension.js", "prefs.js"]:
            reads = set(SETTINGS_READS.findall((_EXT_DIR / js).read_text()))
            self.assertTrue(reads, f"{js} should read at least one setting")
            self.assertEqual(reads - keys, set(),
                             f"{js} reads undeclared schema keys: {reads - keys}")

    def test_stylesheet_has_level_classes(self):
        css = (_EXT_DIR / "stylesheet.css").read_text()
        for cls in ["um-panel-text", "um-ok", "um-warning", "um-critical",
                    "um-stale", "um-card", "um-card-cost", "um-track", "um-fill"]:
            self.assertIn(cls, css, f"stylesheet missing .{cls}")

    def test_extension_spawns_widget_gnome(self):
        js = (_EXT_DIR / "extension.js").read_text()
        self.assertIn("this._widgetTarget = 'gnome'", js)
        self.assertIn("'widget', 'kde'", js,
                      "older CLIs must use the shared KDE widget JSON")
        self.assertIn("Gio._promisify(Gio.Subprocess.prototype, 'communicate_utf8_async')", js,
                      "async subprocess output must be promisified before awaiting it")
        self.assertIn("USAGE_MONITOR_BIN", js)
        self.assertIn("usage-monitor-cli not found", js,
                      "missing-CLI notice required for store installs")
        self.assertIn("GObject.registerClass", js,
                      "GObject subclasses must be registered or construction throws")
        self.assertIn("panelPercentages(", js,
                      "the panel must render the configured rate windows")
        self.assertIn("get_strv('bar-windows')", js)

    def test_panel_percentages_include_each_selected_window(self):
        if shutil.which("node") is None:
            self.skipTest("node not installed")
        with tempfile.TemporaryDirectory() as tmp:
            module_path = Path(tmp) / "panel_values.mjs"
            module_path.write_text((_EXT_DIR / "panel_values.js").read_text())
            test_path = Path(tmp) / "test.mjs"
            test_path.write_text("""
import assert from 'node:assert/strict';
import { panelPercentages } from './panel_values.mjs';

const summary = { providers: [
    { provider_id: 'codex', windows: [
        { id: 'primary', percentage: 42 },
        { id: 'secondary', percentage: 84 },
    ] },
    { provider_id: 'claude', windows: [
        { id: 'primary', percentage: 70 },
        { id: 'secondary', percentage: 20 },
    ] },
] };
assert.deepEqual(panelPercentages({ providers: [summary.providers[0]] }, '',
    ['secondary', 'primary']), [42, 84]);
assert.deepEqual(panelPercentages(summary, '', ['secondary', 'primary']), [70, 84]);
assert.deepEqual(panelPercentages(summary, 'codex', ['secondary', 'primary']), [42, 84]);
""")
            proc = subprocess.run(["node", str(test_path)], cwd=tmp,
                                  capture_output=True, text=True, timeout=60,
                                  check=False)
            self.assertEqual(proc.returncode, 0, proc.stderr.strip())

    def test_provider_settings_parsers(self):
        if shutil.which("node") is None:
            self.skipTest("node not installed")
        with tempfile.TemporaryDirectory() as tmp:
            module_path = Path(tmp) / "provider_settings.mjs"
            module_path.write_text((_EXT_DIR / "provider_settings.js").read_text())
            test_path = Path(tmp) / "test.mjs"
            test_path.write_text(r"""
import assert from 'node:assert/strict';
import {
    parseProviderAccounts,
    parseProviderList,
    providerAuth,
} from './provider_settings.mjs';

assert.deepEqual(parseProviderList([
    'openai enabled OpenAI — Usage API',
    'codex disabled (auto) Codex — Local CLI',
].join('\n')), [
    { id: 'openai', displayName: 'OpenAI', enabled: true, state: 'enabled' },
    { id: 'codex', displayName: 'Codex', enabled: false, state: 'disabled (auto)' },
]);

assert.deepEqual(parseProviderAccounts([
    'provider = codex',
    'state = enabled',
    '[default] (auto-detected)',
    '  token = secret-value',
    '[work] Work',
    '  disabled',
    '  api_key = another-secret',
].join('\n')), [
    {
        id: 'default',
        label: 'Auto-detected credentials',
        active: true,
        removable: false,
        autoDetected: true,
    },
    {
        id: 'work',
        label: 'Work',
        active: false,
        removable: true,
        autoDetected: false,
    },
]);
assert.equal(providerAuth('openai').fields[0].secret, true);
assert.equal(providerAuth('deepgram').fields.some(field => field.key === 'project_id'), true);
assert.deepEqual(providerAuth('codex').requiredFields, ['credentials_path']);
assert.deepEqual(providerAuth('gemini').requiredAny, ['credentials_path', 'access_token']);
""")
            proc = subprocess.run(["node", str(test_path)], cwd=tmp,
                                  capture_output=True, text=True, timeout=60,
                                  check=False)
            self.assertEqual(proc.returncode, 0, proc.stderr.strip())

    def test_provider_ui_has_account_management_and_clickable_about_site(self):
        prefs = (_EXT_DIR / "prefs.js").read_text()
        self.assertIn("new Adw.PasswordEntryRow", prefs)
        self.assertIn("'account', 'set'", prefs)
        self.assertIn("'account', 'remove'", prefs)
        self.assertIn("new Gtk.LinkButton", prefs)
        self.assertIn("uri: website", prefs)

    def test_js_syntax_with_node_when_available(self):
        if shutil.which("node") is None:
            self.skipTest("node not installed")
        for js in ["extension.js", "panel_values.js", "provider_settings.js", "prefs.js"]:
            with tempfile.NamedTemporaryFile(suffix=".mjs", delete=False) as tmp:
                tmp.write((_EXT_DIR / js).read_bytes())
                tmp_path = tmp.name
            try:
                proc = subprocess.run(["node", "--check", tmp_path],
                                      capture_output=True, text=True, timeout=60,
                                      check=False)
            finally:
                Path(tmp_path).unlink(missing_ok=True)
            self.assertEqual(proc.returncode, 0, f"{js}: {proc.stderr.strip()}")

    def test_schema_compiles_when_tool_available(self):
        if shutil.which("glib-compile-schemas") is None:
            self.skipTest("glib-compile-schemas not installed")
        with tempfile.TemporaryDirectory() as tmp:
            shutil.copy(_SCHEMA, tmp)
            proc = subprocess.run(["glib-compile-schemas", "--strict", tmp],
                                  capture_output=True, text=True, timeout=60,
                                  check=False)
            self.assertEqual(proc.returncode, 0, proc.stderr.strip())


if __name__ == "__main__":
    unittest.main()
