"""Packaging regressions runnable without native packagers."""
import pathlib
import subprocess
import tempfile
import tomllib
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[2]


class PackagingTests(unittest.TestCase):
    def run_script(self, name, *args):
        return subprocess.run(
            [str(ROOT / 'dist' / name), *map(str, args)],
            capture_output=True, text=True, check=False,
        )

    def test_manifest_has_offline_rust_toolchain_and_exported_icon(self):
        manifest = yaml.safe_load((ROOT / 'dist/flatpak/manifest.template.yml').read_text())
        self.assertIn('org.freedesktop.Sdk.Extension.rust-stable', manifest['sdk-extensions'])
        self.assertIn('/rust-stable/bin', manifest['build-options']['append-path'])
        commands = '\n'.join(manifest['modules'][0]['build-commands'])
        self.assertIn('--frozen', commands)
        self.assertIn('apps/dev.usage_monitor.UsageMonitor.png', commands)
        self.assertIn('Icon=dev.usage_monitor.UsageMonitor', commands)
        self.assertNotIn('--talk-name=org.freedesktop.Flatpak', manifest['finish-args'])

    def test_deb_detects_native_library_requirements(self):
        cargo = tomllib.loads((ROOT / 'usage-monitor-cli/Cargo.toml').read_text())
        self.assertIn('$auto', cargo['package']['metadata']['deb']['depends'])

    def test_renderer_supports_local_archive_and_preserves_checksum(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = pathlib.Path(tmp) / 'source bundle.tar.zst'
            archive.touch()
            out = pathlib.Path(tmp) / 'manifest.yml'
            result = self.run_script('flatpak/render-manifest.sh', '--version', '1.2.3',
                                     '--source-sha256', 'a' * 64, '--source-path', archive,
                                     '--out', out)
            self.assertEqual(result.returncode, 0, result.stderr)
            source = yaml.safe_load(out.read_text())['modules'][0]['sources'][0]
            self.assertEqual(source['path'], archive.name)
            self.assertEqual(source['sha256'], 'a' * 64)
            self.assertNotIn('url', source)

    def test_renderer_rejects_invalid_inputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            for version, sha in [('1/2/3', 'a' * 64), ('1.2.3', 'bad')]:
                with self.subTest(version=version, sha=sha):
                    result = self.run_script('flatpak/render-manifest.sh', '--version', version,
                                             '--source-sha256', sha, '--out', pathlib.Path(tmp) / 'out')
                    self.assertNotEqual(result.returncode, 0)

    def test_build_all_rejects_old_artifacts_without_touching_them(self):
        with tempfile.TemporaryDirectory() as tmp:
            old = pathlib.Path(tmp) / 'old.tar.gz'
            old.write_text('old')
            result = self.run_script('build-all.sh', '--out-dir', tmp)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('must be empty', result.stderr)
            self.assertEqual(old.read_text(), 'old')

    def test_incomplete_tarball_install_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = self.run_script('install.sh', '--prefix', pathlib.Path(tmp) / 'prefix')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('not in tarball', result.stderr)

    def test_release_jobs_checkout_the_selected_tag(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/release.yml').read_text())
        # All checkouts must use the normalized TAG (never main): the env
        # definition tolerates a missing `v` prefix and stray whitespace.
        tag_expr = workflow['env']['TAG']
        self.assertIn('github.ref_name', tag_expr)
        self.assertIn("startsWith(inputs.tag, 'v')", tag_expr)
        for job in workflow['jobs'].values():
            for step in job['steps']:
                if step.get('uses', '').startswith('actions/checkout@'):
                    self.assertEqual(step['with']['ref'], '${{ env.TAG }}')


if __name__ == '__main__':
    unittest.main()
