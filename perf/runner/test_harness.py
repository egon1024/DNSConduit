"""Harness unit tests — no live loadgen or Conduit binary required."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from perf.render import FORMATS, render
from perf.runner.catalog import filter_scenarios, load_annotations, load_scenarios
from perf.runner.execute import public_conduit_path
from perf.runner.loadgen import DEFAULT_IMAGE, docker_dnsperf_cmd, parse_dnsperf_output
from perf.runner.paths import RESULTS_SCHEMA, load_json
from perf.runner.run_record import (
    detect_lab_profile_runtime,
    detect_meminfo_total_mb,
    validate_run_document,
    write_run_document,
)


SAMPLE_DNSPERF = """
DNS Performance Testing Tool
Version 2.14.0

[Status] Command line: dnsperf -s 127.0.2.1 -p 15353

Statistics:

  Queries sent:         10000
  Queries completed:    9980
  Queries lost:         20

  Response codes:       NOERROR 9980 (100.00%)

  Average Latency (s):  0.001234
  Latency Min/Max (s):  0.000100/0.010000

  Queries per second:   997.500000
"""


def _minimal_run_doc(**overrides):
    doc = {
        "schema_version": 1,
        "generated_at": "2026-07-26T12:00:00Z",
        "lab_profile": {
            "id": "test-lab",
            "display_name": "Unit test lab",
            "cpu_model": "Test CPU",
            "physical_cores": 4,
            "logical_cores": 8,
            "os": "Linux test",
        },
        "provenance": {
            "conduit_path": "/tmp/conduit",
            "conduit_version": "test",
            "loadgen": {"tool": "dnsperf", "mode": "docker", "image": "dnsconduit-dnsperf:2.14.0"},
        },
        "scenarios": [
            {
                "id": "scale-sync-forward-fast",
                "suite": "scale",
                "status": "ok",
                "axes": {"runtime": "sync", "load_shape": "forward_fast"},
                "metrics": {
                    "achieved_qps": 1000.0,
                    "latency_ms": {"avg": 0.8, "p99": 1.5},
                },
                "annotation_ids": ["ann-example-context"],
            }
        ],
        "annotation_ids": ["ann-example-context"],
    }
    doc.update(overrides)
    return doc


class CatalogTests(unittest.TestCase):
    def test_load_scale_scenarios(self):
        scenarios = load_scenarios()
        ids = {s.id for s in scenarios}
        self.assertIn("scale-sync-forward-fast", ids)
        self.assertIn("scale-split-io-forward-slow", ids)
        self.assertTrue(any(s.intent for s in scenarios))

    def test_load_shutdown_drain_scenarios(self):
        scenarios = filter_scenarios(load_scenarios(), suite="shutdown_drain")
        ids = {s.id for s in scenarios}
        self.assertEqual(
            ids,
            {
                "shutdown-drain-complete-forward-slow",
                "shutdown-drain-budgeted-forward-slow",
                "shutdown-drain-minimal-forward-slow",
            },
        )
        for sc in scenarios:
            self.assertTrue(sc.curated)
            self.assertEqual(sc.axes.get("load_shape"), "forward_slow")
            self.assertTrue(sc.recipe.get("shutdown"))
            self.assertIn(
                sc.axes.get("drain_policy"),
                {"drain_complete", "drain_budgeted", "drain_minimal"},
            )

    def test_load_feature_tax_scenarios(self):
        scenarios = filter_scenarios(load_scenarios(), suite="feature_tax")
        ids = {s.id for s in scenarios}
        self.assertIn("feature-tax-metrics-off-forward-fast", ids)
        self.assertIn("feature-tax-metrics-minimal-scrape-forward-fast", ids)
        self.assertIn("feature-tax-metrics-standard-scrape-forward-fast", ids)
        self.assertIn("feature-tax-metrics-collect-only-forward-fast", ids)
        self.assertIn("feature-tax-metrics-otlp-push-forward-fast", ids)
        self.assertIn("feature-tax-dnstap-off-forward-fast", ids)
        self.assertIn("feature-tax-dnstap-sampled-forward-fast", ids)
        otlp = next(s for s in scenarios if s.id.endswith("otlp-push-forward-fast"))
        self.assertEqual(otlp.recipe.get("skip_unless"), "otlp_tracer")
        curated = {s.id for s in scenarios if s.curated}
        self.assertIn("feature-tax-metrics-off-forward-fast", curated)
        self.assertIn("feature-tax-dnstap-sampled-forward-fast", curated)

    def test_load_lifecycle_scenarios(self):
        scenarios = filter_scenarios(load_scenarios(), suite="lifecycle")
        ids = {s.id for s in scenarios}
        self.assertEqual(ids, {"lifecycle-cold-start", "lifecycle-config-apply"})
        cold = next(s for s in scenarios if s.id == "lifecycle-cold-start")
        self.assertTrue(cold.curated)
        self.assertEqual(cold.recipe.get("lifecycle"), "cold_start")
        apply = next(s for s in scenarios if s.id == "lifecycle-config-apply")
        self.assertEqual(apply.recipe.get("lifecycle"), "config_apply")
        self.assertTrue(apply.recipe.get("overlay"))

    def test_suite_filter(self):
        scenarios = filter_scenarios(load_scenarios(), suite="scale")
        self.assertTrue(scenarios)
        self.assertTrue(all(s.suite == "scale" for s in scenarios))

    def test_scenario_id_filter(self):
        scenarios = filter_scenarios(
            load_scenarios(), scenario_id="scale-sync-forward-fast"
        )
        self.assertEqual(len(scenarios), 1)
        self.assertEqual(scenarios[0].id, "scale-sync-forward-fast")


class SchemaTests(unittest.TestCase):
    def test_schema_file_exists(self):
        self.assertTrue(RESULTS_SCHEMA.is_file())

    def test_valid_run_document(self):
        validate_run_document(_minimal_run_doc())

    def test_missing_lab_profile_rejected(self):
        doc = _minimal_run_doc()
        del doc["lab_profile"]
        with self.assertRaises(ValueError):
            validate_run_document(doc)

    def test_missing_cpu_model_rejected(self):
        doc = _minimal_run_doc()
        doc["lab_profile"]["cpu_model"] = ""
        with self.assertRaises(ValueError):
            validate_run_document(doc)

    def test_write_roundtrip(self):
        doc = _minimal_run_doc()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "run.json"
            write_run_document(doc, path=path)
            loaded = load_json(path)
            self.assertEqual(loaded["lab_profile"]["id"], "test-lab")


class RenderTests(unittest.TestCase):
    def test_all_formats(self):
        doc = _minimal_run_doc()
        for fmt in FORMATS:
            text = render(doc, fmt)
            self.assertTrue(text)
            if fmt == "json":
                json.loads(text)
            if fmt == "html":
                self.assertIn("<html", text)
                self.assertIn("scale-sync-forward-fast", text)
                self.assertIn("avg ms", text)
                self.assertIn("0.800", text)
                self.assertNotIn("p99 ms", text)
            if fmt == "plain":
                self.assertIn("lab_profile:", text)
                self.assertNotIn("✓", text)
                self.assertIn("avg=0.80ms", text)

    def test_fancy_uses_unicode(self):
        text = render(_minimal_run_doc(), "fancy")
        self.assertIn("✓", text)

    def test_render_shutdown_drain_metrics(self):
        doc = _minimal_run_doc(
            scenarios=[
                {
                    "id": "shutdown-drain-budgeted-forward-slow",
                    "suite": "shutdown_drain",
                    "status": "ok",
                    "axes": {
                        "runtime": "sync",
                        "load_shape": "forward_slow",
                        "drain_policy": "drain_budgeted",
                    },
                    "metrics": {
                        "achieved_qps": 500.0,
                        "drain_duration_ms": 260.5,
                        "queries_lost": 42,
                    },
                    "secondary": {"client_failures_during_stop": 42},
                }
            ]
        )
        plain = render(doc, "plain")
        self.assertIn("drain=260.5ms", plain)
        self.assertIn("loss=42", plain)
        html = render(doc, "html")
        self.assertIn("Drain ms", html)
        self.assertIn("260.5", html)
        self.assertIn("42", html)

    def test_render_lifecycle_metrics(self):
        doc = _minimal_run_doc(
            scenarios=[
                {
                    "id": "lifecycle-cold-start",
                    "suite": "lifecycle",
                    "status": "ok",
                    "axes": {"runtime": "sync"},
                    "metrics": {"cold_start_ms": 42.5},
                },
                {
                    "id": "lifecycle-config-apply",
                    "suite": "lifecycle",
                    "status": "ok",
                    "axes": {"runtime": "sync"},
                    "metrics": {"apply_latency_ms": 12.25},
                },
            ]
        )
        plain = render(doc, "plain")
        self.assertIn("cold_start=42.5ms", plain)
        self.assertIn("apply=12.2ms", plain)
        html = render(doc, "html")
        self.assertIn("Cold start ms", html)
        self.assertIn("42.5", html)
        self.assertIn("12.2", html)

    def test_render_otlp_skip(self):
        doc = _minimal_run_doc(
            scenarios=[
                {
                    "id": "feature-tax-metrics-otlp-push-forward-fast",
                    "suite": "feature_tax",
                    "status": "skip",
                    "skip_reason": "conduit-otlp-metrics-tracer not available",
                    "axes": {"obs_posture": "metrics_otlp_push"},
                }
            ]
        )
        plain = render(doc, "plain")
        self.assertIn("SKIP", plain)
        self.assertIn("conduit-otlp-metrics-tracer not available", plain)


class DnsperfParseTests(unittest.TestCase):
    def test_parse_sample(self):
        result = parse_dnsperf_output(SAMPLE_DNSPERF)
        self.assertAlmostEqual(result.achieved_qps or 0, 997.5)
        self.assertEqual(result.queries_sent, 10000)
        self.assertEqual(result.queries_completed, 9980)
        self.assertEqual(result.queries_lost, 20)
        self.assertIn("avg", result.latency_ms)

    def test_docker_cmd_does_not_repeat_entrypoint(self):
        cmd = docker_dnsperf_cmd(
            image=DEFAULT_IMAGE,
            query_dir=Path("/tmp/queries"),
            flags=["-s", "127.0.2.1", "-p", "15353"],
        )
        # Image ENTRYPOINT is dnsperf; argv after the image must be flags only.
        image_idx = cmd.index(DEFAULT_IMAGE)
        self.assertEqual(cmd[image_idx + 1 :], ["-s", "127.0.2.1", "-p", "15353"])
        self.assertNotIn("dnsperf", cmd[image_idx + 1 :])


class AnnotationCatalogTests(unittest.TestCase):
    def test_load_optional(self):
        # May be empty early; must not raise.
        anns = load_annotations()
        self.assertIsInstance(anns, list)


class ProvenancePathTests(unittest.TestCase):
    def test_relative_under_cwd(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "target" / "release" / "conduit"
            binary.parent.mkdir(parents=True)
            binary.write_text("", encoding="utf-8")
            self.assertEqual(
                public_conduit_path(binary, cwd=root),
                "target/release/conduit",
            )

    def test_outside_cwd_uses_basename(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            other = root / "elsewhere"
            other.mkdir()
            binary = other / "conduit"
            binary.write_text("", encoding="utf-8")
            cwd = root / "workdir"
            cwd.mkdir()
            self.assertEqual(public_conduit_path(binary, cwd=cwd), "conduit")


class LabProfileDetectTests(unittest.TestCase):
    def test_meminfo_parse(self):
        mb = detect_meminfo_total_mb()
        # Linux CI/dev hosts expose /proc/meminfo; value must be positive.
        if Path("/proc/meminfo").is_file():
            self.assertIsInstance(mb, int)
            self.assertGreater(mb, 0)

    def test_runtime_profile_includes_memory_when_available(self):
        profile = detect_lab_profile_runtime(profile_id="unit")
        self.assertEqual(profile["id"], "unit")
        self.assertTrue(profile["cpu_model"])
        if Path("/proc/meminfo").is_file():
            self.assertIn("memory_total_mb", profile)
            self.assertGreater(profile["memory_total_mb"], 0)
            validate_run_document(
                _minimal_run_doc(lab_profile={**profile, "cpu_model": "Test CPU"})
            )


if __name__ == "__main__":
    unittest.main()
