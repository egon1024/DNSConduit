"""Takeaway integrity checks for performance study pages (Gate G5)."""

from __future__ import annotations

import unittest

from perf.render.charts import ChartSpec
from perf.runner.integrity import (
    BANNED_TAKEAWAY_PHRASES,
    AllowedClaims,
    check_banned_phrases,
    check_takeaway_numeric_claims,
    claims_from_charts,
    format_delta_fragment,
    takeaway_section,
)


def _qps_chart(figure_id: str, categories: list[str], values: list[float | None]) -> ChartSpec:
    return ChartSpec(
        id=figure_id,
        title=figure_id,
        y_label="Achieved QPS",
        categories=categories,
        series=[("achieved_qps", values)],
    )


class IntegrityUnitTests(unittest.TestCase):
    def test_claims_from_charts_include_ratio_percent_and_qps_k(self):
        charts = [
            _qps_chart(
                "sync-vs-split-io-forward-fast",
                ["sync", "split_io"],
                [75_000.0, 141_000.0],
            )
        ]
        claims = claims_from_charts(charts, primary_metric="achieved_qps")
        self.assertIn(75, claims.qps_thousands)
        self.assertIn(141, claims.qps_thousands)
        self.assertTrue(any(abs(m - 1.9) < 0.05 for m in claims.multipliers))
        # Tax of sync vs split is large; also allow ~88% style abs delta from baseline.
        self.assertTrue(claims.percents or claims.multipliers)

    def test_stale_multiplier_in_takeaway_fails(self):
        charts = [
            _qps_chart(
                "sync-vs-split-io-forward-fast",
                ["sync", "split_io"],
                [75_000.0, 141_000.0],
            )
        ]
        claims = claims_from_charts(charts, primary_metric="achieved_qps")
        page = (
            "# Study\n\n"
            "## Evidence\n\nok\n\n"
            "## Takeaway\n\n"
            "split_io reaches about **99.9×** the QPS of sync (~999k vs ~75k).\n\n"
            "## Related guides\n\n"
            "- none\n"
        )
        errors = check_takeaway_numeric_claims(page, claims, study_id="sync-vs-split-io")
        self.assertTrue(errors, "expected stale claims to fail")
        joined = " ".join(errors)
        self.assertIn("99.9", joined)
        self.assertIn("999", joined)

    def test_matching_takeaway_passes(self):
        charts = [
            _qps_chart(
                "sync-vs-split-io-forward-fast",
                ["sync", "split_io"],
                [74_932.0, 140_686.0],
            )
        ]
        claims = claims_from_charts(charts, primary_metric="achieved_qps")
        page = (
            "## Takeaway\n\n"
            "split_io reaches about **1.9×** the QPS of sync (~141k vs ~75k).\n"
        )
        errors = check_takeaway_numeric_claims(page, claims, study_id="sync-vs-split-io")
        self.assertEqual(errors, [])

    def test_banned_phrase_fails(self):
        takeaway = (
            "These poles look like same-host noise; treat the inversion as fragile.\n"
        )
        errors = check_banned_phrases(takeaway, study_id="logging-verbosity-tax")
        self.assertTrue(errors)
        self.assertTrue(
            any(p in " ".join(errors).lower() for p in ("noise", "inversion"))
        )
        self.assertTrue(BANNED_TAKEAWAY_PHRASES)

    def test_delta_fragment_lists_ratio(self):
        charts = [
            ChartSpec(
                id="cache-hit-vs-forward-fast",
                title="Warm cache vs forward_fast",
                y_label="Achieved QPS",
                categories=["forward_fast", "cache_hit"],
                series=[("achieved_qps", [75_000.0, 337_000.0])],
            )
        ]
        claims = claims_from_charts(charts, primary_metric="achieved_qps")
        md = format_delta_fragment(
            study_id="cache-hit-vs-forward",
            charts=charts,
            claims=claims,
            primary_metric="achieved_qps",
        )
        self.assertIn("## At a glance", md)
        self.assertIn("Warm cache vs forward_fast", md)
        self.assertIn("4.5×", md)
        self.assertIn("~337k", md)
        self.assertIn("~75k", md)
        self.assertNotIn("Machine-checkable", md)
        self.assertNotIn("promoted reference JSON", md)

    def test_delta_fragment_path_duration_uses_ms(self):
        charts = [
            ChartSpec(
                id="churn-fill",
                title="Cache fill mean duration — sync ingress-8",
                y_label="Fill mean (ms)",
                categories=["memory", "lmdb"],
                series=[
                    ("cache_fill_duration_mean_ms", [0.0005, 2.9683]),
                ],
            )
        ]
        claims = claims_from_charts(charts, primary_metric="achieved_qps")
        md = format_delta_fragment(
            study_id="memory-vs-lmdb-cache-churn",
            charts=charts,
            claims=claims,
            primary_metric="achieved_qps",
        )
        self.assertIn("ms", md)
        self.assertNotIn("QPS", md)
        self.assertIn("~3.0 ms", md)
        self.assertIn("~0.0005 ms", md)
        self.assertIn(0.0005, claims.durations_ms)

    def test_takeaway_section_stops_at_next_heading(self):
        page = "## Takeaway\n\nHello **1.5×**\n\n## Related\n\n**9.9×** junk\n"
        section = takeaway_section(page)
        self.assertIn("1.5×", section)
        self.assertNotIn("9.9×", section)


class LiveReferenceIntegrityTests(unittest.TestCase):
    """Against committed reference JSON + current study pages (G5.5)."""

    def test_published_study_takeaways_match_reference(self):
        from pathlib import Path

        from perf.render.charts import charts_for_studies
        from perf.runner.catalog import load_scenarios, load_studies
        from perf.runner.integrity import check_study_page
        from perf.runner.publish import load_latest_reference

        doc = load_latest_reference()
        if doc is None:
            self.skipTest("no promoted reference JSON")
        studies = load_studies(scenarios=load_scenarios())
        errors: list[str] = []
        for study, charts in charts_for_studies(doc, studies, published_only=True):
            path = Path(
                f"operator-docs/docs/performance/studies/{study.id}.md"
            )
            if not path.is_file():
                errors.append(f"{study.id}: missing study page")
                continue
            errors.extend(
                check_study_page(
                    path.read_text(encoding="utf-8"),
                    charts,
                    study_id=study.id,
                    primary_metric=study.primary_metric,
                )
            )
        self.assertEqual(errors, [], msg="\n".join(errors))


if __name__ == "__main__":
    unittest.main()
