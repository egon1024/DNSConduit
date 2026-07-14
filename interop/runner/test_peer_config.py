"""Unit tests for peer_setup IR and Conduit delta merge."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import yaml

from interop.runner.cases import load_cases
from interop.runner.catalog import load_peers
from interop.runner.conduit_merge import merge_conduit_profile
from interop.runner.setup_ir import LocalRR, SetupIR, parse_peer_setup, resolve_fixture_dirs


class SetupIrTests(unittest.TestCase):
    def test_parse_local_rr_and_fixtures(self):
        raw = {
            "fixtures": ["example.test"],
            "local_rr": [
                {"name": "www.smoke.test.", "type": "A", "rdata": "192.0.2.20", "ttl": 300}
            ],
        }
        ir = parse_peer_setup(raw)
        self.assertEqual(ir.fixtures, ["example.test"])
        self.assertEqual(len(ir.local_rr), 1)
        self.assertEqual(ir.local_rr[0].name, "www.smoke.test.")
        self.assertEqual(ir.local_rr[0].rdata, "192.0.2.20")

    def test_empty_peer_setup(self):
        ir = parse_peer_setup(None)
        self.assertEqual(ir.fixtures, [])
        self.assertEqual(ir.local_rr, [])

    def test_missing_fixture_dir_raises(self):
        ir = SetupIR(fixtures=["does-not-exist"], local_rr=[])
        with self.assertRaises(FileNotFoundError):
            resolve_fixture_dirs(ir)


class ConduitMergeTests(unittest.TestCase):
    def test_replace_top_level_keys_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            profile = Path(tmp) / "profile.yml"
            profile.write_text(
                "schema_version: 1\npools:\n  - name: default\n    backends: []\nrules:\n  match_mode: first_match\n",
                encoding="utf-8",
            )
            out = Path(tmp) / "out.yml"
            merged = merge_conduit_profile(
                profile,
                {"pools": [{"name": "default", "backends": [{"address": "1.2.3.4:53", "weight": 100}]}]},
                out,
            )
            self.assertEqual(merged["pools"][0]["backends"][0]["address"], "1.2.3.4:53")
            self.assertEqual(merged["rules"]["match_mode"], "first_match")
            on_disk = yaml.safe_load(out.read_text(encoding="utf-8"))
            self.assertEqual(on_disk["pools"][0]["backends"][0]["address"], "1.2.3.4:53")


class CatalogFamilyTests(unittest.TestCase):
    def test_every_peer_has_family(self):
        for peer in load_peers():
            self.assertTrue(peer.family, msg=f"{peer.id} missing family")


class CaseHookLoadTests(unittest.TestCase):
    def test_cases_expose_peer_setup_attr(self):
        cases = {c.id: c for c in load_cases()}
        case = cases["basic-a-forward"]
        self.assertIsInstance(case.peer_setup, dict)
        self.assertIsInstance(case.conduit_delta, dict)

    def test_missing_family_includes_peer_id(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "peers.yaml"
            path.write_text(
                yaml.safe_dump({
                    "schema_version": 1,
                    "peers": [{
                        "id": "broken-peer",
                        "publisher": "X",
                        "product": "Y",
                        "version": "1",
                        "role": "stub",
                        "image": "img",
                        # family intentionally omitted
                    }],
                }),
                encoding="utf-8",
            )
            with self.assertRaises(KeyError) as ctx:
                load_peers(path)
            self.assertIn("broken-peer", str(ctx.exception))
            self.assertIn("family", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
