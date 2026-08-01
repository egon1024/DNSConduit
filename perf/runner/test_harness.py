"""Harness unit tests — no live loadgen or Conduit binary required."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from perf.render import FORMATS, render
from perf.runner.catalog import (
    filter_scenarios,
    load_annotations,
    load_scenarios,
    load_studies,
    publish_set_member_ids,
    resolve_scenario_ids_from_studies,
    select_scenarios,
)
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


class StudyCatalogTests(unittest.TestCase):
    def test_load_wave1_studies(self):
        scenarios = load_scenarios()
        studies = load_studies(scenarios=scenarios)
        ids = {s.id for s in studies}
        self.assertIn("sync-vs-split-io", ids)
        self.assertIn("metrics-scrape-ladder", ids)
        self.assertIn("dnstap-emit-tax", ids)
        self.assertIn("drain-policy-under-slow", ids)
        self.assertIn("cache-hit-vs-forward", ids)
        self.assertIn("split-io-thread-bulk", ids)
        sync = next(s for s in studies if s.id == "sync-vs-split-io")
        self.assertTrue(sync.published)
        self.assertEqual(len(sync.figures), 2)
        self.assertEqual(
            list(sync.members)[:2],
            ["scale-sync-forward-fast", "scale-split-io-forward-fast"],
        )

    def test_study_selection_preserves_order(self):
        scenarios = load_scenarios()
        selected = select_scenarios(
            scenarios, study_ids=["metrics-scrape-ladder"]
        )
        self.assertEqual(
            [s.id for s in selected],
            [
                "feature-tax-metrics-off-scrape-ladder-forward-fast",
                "feature-tax-metrics-minimal-scrape-ladder-forward-fast",
                "feature-tax-metrics-standard-scrape-ladder-forward-fast",
            ],
        )

    def test_studies_index_order_matches_mkdocs_nav(self):
        from perf.runner import publish as publish_mod

        nav_ids = publish_mod._study_nav_order_ids()
        self.assertEqual(
            nav_ids[:3],
            [
                "sync-vs-split-io",
                "ingress-concurrency-sync",
                "io-vs-ingress-split",
            ],
        )
        published = [s for s in load_studies(scenarios=load_scenarios()) if s.published]
        ordered = publish_mod._order_studies_like_nav(published)
        self.assertEqual([s.id for s in ordered], nav_ids)

    def test_publish_set_is_union(self):
        scenarios = load_scenarios()
        studies = load_studies(scenarios=scenarios)
        members = publish_set_member_ids(studies)
        self.assertIn("scale-sync-forward-fast", members)
        self.assertIn("feature-tax-metrics-off-forward-fast", members)
        self.assertEqual(len(members), len(set(members)))
        # Shared baseline appears once even though multiple studies cite it.
        self.assertEqual(members.count("scale-sync-forward-fast"), 1)
        selected = select_scenarios(scenarios, publish_set=True, studies=studies)
        self.assertEqual([s.id for s in selected], members)

    def test_unknown_study_id_errors(self):
        with self.assertRaises(ValueError) as ctx:
            select_scenarios(load_scenarios(), study_ids=["no-such-study"])
        self.assertIn("unknown study id", str(ctx.exception))

    def test_resolve_unknown_study_keyerror(self):
        studies = load_studies(scenarios=load_scenarios())
        with self.assertRaises(KeyError):
            resolve_scenario_ids_from_studies(studies, study_ids=["missing-study"])


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

    def test_rich_uses_unicode(self):
        text = render(_minimal_run_doc(), "rich")
        self.assertIn("✓", text)

    def test_rich_includes_charts_when_scale_present(self):
        doc = _minimal_run_doc(
            scenarios=[
                {
                    "id": "scale-sync-forward-fast",
                    "suite": "scale",
                    "status": "ok",
                    "axes": {"runtime": "sync", "load_shape": "forward_fast"},
                    "metrics": {"achieved_qps": 1000.0, "latency_ms": {"avg": 0.8}},
                },
                {
                    "id": "scale-split-io-forward-fast",
                    "suite": "scale",
                    "status": "ok",
                    "axes": {"runtime": "split_io", "load_shape": "forward_fast"},
                    "metrics": {"achieved_qps": 1100.0, "latency_ms": {"avg": 0.7}},
                },
            ]
        )
        text = render(doc, "rich")
        self.assertIn("── charts ──", text)
        self.assertIn("█", text)
        html = render(doc, "html")
        self.assertIn("<svg", html)
        self.assertIn("forward_fast", html)

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

    def test_render_otlp_secondary(self):
        doc = _minimal_run_doc(
            scenarios=[
                {
                    "id": "feature-tax-metrics-otlp-push-forward-fast",
                    "suite": "feature_tax",
                    "status": "ok",
                    "axes": {"obs_posture": "metrics_otlp_push"},
                    "metrics": {"achieved_qps": 1000.0, "latency_ms": {"avg": 1.5}},
                    "secondary": {"otlp_accepts": 6, "otlp_failures": 0},
                }
            ]
        )
        plain = render(doc, "plain")
        self.assertIn("otlp_accepts=6", plain)
        self.assertIn("otlp_failures=0", plain)
        html = render(doc, "html")
        self.assertIn("OTLP accepts", html)
        self.assertIn("OTLP failures", html)
        self.assertIn("<td>6</td><td>0</td>", html)


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
        anns = load_annotations()
        self.assertIsInstance(anns, list)
        ids = {a.id for a in anns}
        self.assertIn("ann-example-context", ids)
        self.assertIn("ann-thin-spine-context", ids)
        example = next(a for a in anns if a.id == "ann-example-context")
        self.assertEqual(example.tone, "context")
        self.assertTrue(example.title)
        self.assertTrue(example.body)

    def test_include_fragments_and_page_markers(self):
        from unittest import mock

        from perf.runner import publish as publish_mod
        from perf.runner.catalog import Annotation

        sample = Annotation(
            id="ann-unit-noise",
            tone="known_noise",
            title="Unit noise title",
            body="Body line one.\n\nBody line two.",
            related_scenarios=("scale-sync-forward-slow",),
            related_releases=(),
        )
        md = publish_mod.annotation_include_markdown(sample)
        self.assertIn('!!! warning "Unit noise title"', md)
        self.assertIn("    Body line one.", md)

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            perf_docs = root / "performance"
            includes = perf_docs / "includes"
            includes.mkdir(parents=True)
            page = perf_docs / "methodology.md"
            page.write_text(
                "# Meth\n\n"
                "<!-- perf-ann:ann-unit-noise:start -->\n"
                "_placeholder_\n"
                "<!-- perf-ann:ann-unit-noise:end -->\n",
                encoding="utf-8",
            )
            with mock.patch.object(publish_mod, "OPERATOR_PERF", perf_docs):
                written = publish_mod.write_annotation_include_fragments([sample])
                self.assertEqual(len(written), 1)
                self.assertTrue((includes / "ann-unit-noise.fragment.md").is_file())
                touched = publish_mod._inject_annotation_includes_into_pages()
            self.assertEqual(touched, [page])
            text = page.read_text(encoding="utf-8")
            self.assertIn('!!! warning "Unit noise title"', text)
            self.assertNotIn("_placeholder_", text)


class PublishTests(unittest.TestCase):
    def test_promote_and_generate_docs(self):
        from unittest import mock

        from perf.runner import publish as publish_mod

        doc = _minimal_run_doc(
            lab_profile={
                "id": "maintainer-ws-1",
                "display_name": "Maintainer workstation",
                "cpu_model": "Test CPU",
                "physical_cores": 4,
                "logical_cores": 8,
                "os": "Linux test",
            },
            scenarios=[
                {
                    "id": "scale-sync-forward-fast",
                    "suite": "scale",
                    "status": "ok",
                    "axes": {"runtime": "sync", "load_shape": "forward_fast"},
                    "metrics": {
                        "achieved_qps": 1000.0,
                        "latency_ms": {"avg": 0.8},
                    },
                },
                {
                    "id": "scale-sync-forward-slow",
                    "suite": "scale",
                    "status": "ok",
                    "axes": {"runtime": "sync", "load_shape": "forward_slow"},
                    "metrics": {
                        "achieved_qps": 20.0,
                        "latency_ms": {"avg": 50.0},
                    },
                },
                {
                    "id": "scale-split-io-forward-fast",
                    "suite": "scale",
                    "status": "ok",
                    "axes": {"runtime": "split_io", "load_shape": "forward_fast"},
                    "metrics": {
                        "achieved_qps": 1100.0,
                        "latency_ms": {"avg": 0.7},
                    },
                },
                {
                    "id": "scale-split-io-forward-slow",
                    "suite": "scale",
                    "status": "ok",
                    "axes": {"runtime": "split_io", "load_shape": "forward_slow"},
                    "metrics": {
                        "achieved_qps": 40.0,
                        "latency_ms": {"avg": 40.0},
                    },
                },
                {
                    "id": "shutdown-drain-complete-forward-slow",
                    "suite": "shutdown_drain",
                    "status": "ok",
                    "axes": {"drain_policy": "drain_complete"},
                    "metrics": {"drain_duration_ms": 200.0, "achieved_qps": 5.0},
                    "secondary": {"client_failures_during_stop": 10},
                },
                {
                    "id": "shutdown-drain-budgeted-forward-slow",
                    "suite": "shutdown_drain",
                    "status": "ok",
                    "axes": {"drain_policy": "drain_budgeted"},
                    "metrics": {"drain_duration_ms": 150.0, "achieved_qps": 5.0},
                    "secondary": {"client_failures_during_stop": 20},
                },
                {
                    "id": "shutdown-drain-minimal-forward-slow",
                    "suite": "shutdown_drain",
                    "status": "ok",
                    "axes": {"drain_policy": "drain_minimal"},
                    "metrics": {"drain_duration_ms": 50.0, "achieved_qps": 5.0},
                    "secondary": {"client_failures_during_stop": 30},
                },
            ],
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            gen = root / "generated"
            perf_docs = root / "performance"
            gen.mkdir()
            perf_docs.mkdir()
            with mock.patch.object(publish_mod, "GENERATED_DIR", gen), mock.patch.object(
                publish_mod, "OPERATOR_PERF", perf_docs
            ):
                written = publish_mod.generate_operator_docs_fragments(doc)
            names = {p.name for p in written}
            self.assertIn("scale-sync-vs-split-io-forward-fast.svg", names)
            self.assertIn("scale-sync-vs-split-io-forward-slow.svg", names)
            self.assertIn("shutdown-drain-forward-slow.svg", names)
            self.assertIn("shutdown-drain-forward-slow.csv", names)
            self.assertIn("reference.md", names)
            self.assertIn("scenarios.md", names)
            self.assertTrue(
                (gen / "scale-sync-vs-split-io-forward-fast.svg").is_file()
            )
            csv_text = (
                gen / "scale-sync-vs-split-io-forward-fast.csv"
            ).read_text(encoding="utf-8")
            self.assertIn("scale-sync-forward-fast", csv_text)
            self.assertIn("1000", csv_text)
            ref = (perf_docs / "reference.md").read_text(encoding="utf-8")
            self.assertIn("/performance/scenarios.md#", ref)
            self.assertNotIn("Annotations referenced by this reference", ref)
            includes = perf_docs / "includes"
            self.assertTrue((includes / "ann-example-context.fragment.md").is_file())
            self.assertTrue(
                (includes / "ann-forward-slow-lossy-context.fragment.md").is_file()
            )
            self.assertFalse((perf_docs / "annotations.md").is_file())
            self.assertFalse(
                (gen / "annotations-from-reference.fragment.md").is_file()
            )

    def test_lossless_upgrade_scenario_exists(self):
        scenarios = filter_scenarios(load_scenarios(), suite="lossless_upgrade")
        self.assertTrue(scenarios)
        self.assertTrue(all(s.suite == "lossless_upgrade" for s in scenarios))
        self.assertTrue(any(s.recipe.get("skip_unless") == "zdu" for s in scenarios))


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
