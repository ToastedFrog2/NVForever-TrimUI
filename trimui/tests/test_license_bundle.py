"""Release licensing regressions; all fixtures live in temporary directories."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1] / 'tools'
sys.path.insert(0, str(TOOLS))
from license_bundle import validate
spec = importlib.util.spec_from_file_location('generate_licenses', TOOLS / 'generate-licenses.py')
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)


class NoticeTests(unittest.TestCase):
    def test_exact_graph_selection(self):
        wanted = gen.parse_tree('engine v1.0\nbevy v0.16.0 (*)\nbevy v0.16.0\n')
        meta = {'packages': [{'name': n, 'version': v} for n, v in [('engine','1.0'),('bevy','0.16.0'),('dev-only','1.0')]]}
        self.assertEqual(len(gen.select_packages(meta, wanted)), 2)
        with self.assertRaises(ValueError):
            gen.select_packages(meta, wanted | {('missing','1.0')})

    def test_vendor_is_third_party(self):
        root = Path('/example')
        self.assertTrue(gen.ours({'manifest_path':'/example/crates/game/Cargo.toml'}, root))
        self.assertFalse(gen.ours({'manifest_path':'/example/engine/vendor/bevy/Cargo.toml'}, root))

    def test_nested_notices_and_missing_supplement(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root/'licenses').mkdir()
            (root/'licenses/attribution.txt').write_text('copyright')
            (root/'NOTICE').write_text('notice')
            (root/'lib.rs').write_text('code')
            self.assertEqual(len(gen.license_files(root)), 2)
            pkg = {'name':'missing', 'version':'1.0', 'manifest_path':str(root/'Cargo.toml')}
            with self.assertRaisesRegex(ValueError, 'Missing license text'):
                gen.supplemental_files(pkg, {}, root)
            records = {'missing@1.0': {'files':[{'file':'NOTICE','source':'https://example.org','sha256':'bad'}]}}
            with self.assertRaisesRegex(ValueError, 'integrity'):
                gen.supplemental_files(pkg, records, root)

    def test_bundle_rejects_missing_stale_report_and_lock(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root/'engine').mkdir(); (root/'licensing').mkdir()
            for name in ('Cargo.lock','engine/Cargo.lock','THIRD-PARTY-LICENSES.txt'):
                (root/name).write_text(name)
            sha = lambda name: hashlib.sha256((root/name).read_bytes()).hexdigest()
            data = {'schema':1,'target':'aarch64-unknown-linux-gnu','features':['trimui','intro-video'],
                    'packages':[{'name':'test'}], 'local_manifests':{},'notice_inputs':{},
                    'root_lock_sha256':sha('Cargo.lock'),'engine_lock_sha256':sha('engine/Cargo.lock'),
                    'report_sha256':sha('THIRD-PARTY-LICENSES.txt')}
            (root/'licensing/inventory.json').write_text(json.dumps(data))
            validate(root)
            (root/'THIRD-PARTY-LICENSES.txt').write_text('stale')
            with self.assertRaises(ValueError): validate(root)
            (root/'THIRD-PARTY-LICENSES.txt').unlink()
            with self.assertRaises(ValueError): validate(root)
            (root/'THIRD-PARTY-LICENSES.txt').write_text('THIRD-PARTY-LICENSES.txt')
            (root/'Cargo.lock').write_text('new dependency')
            with self.assertRaises(ValueError): validate(root)

    def test_legal_relative_links_exist(self):
        import re
        root = TOOLS.parents[1]
        for target in re.findall(r'\]\(([^)]+)\)', (root/'LEGAL.md').read_text()):
            if '://' not in target:
                self.assertTrue((root/target.split('#')[0]).exists(), target)


if __name__ == '__main__':
    unittest.main()
