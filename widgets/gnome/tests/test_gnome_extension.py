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
                    "um-stale", "um-card", "um-track", "um-fill"]:
            self.assertIn(cls, css, f"stylesheet missing .{cls}")

    def test_extension_spawns_widget_gnome(self):
        js = (_EXT_DIR / "extension.js").read_text()
        self.assertIn("'widget', 'gnome'", js)
        self.assertIn("USAGE_MONITOR_BIN", js)
        self.assertIn("usage-monitor-cli not found", js,
                      "missing-CLI notice required for store installs")

    def test_js_syntax_with_node_when_available(self):
        if shutil.which("node") is None:
            self.skipTest("node not installed")
        for js in ["extension.js", "prefs.js"]:
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
