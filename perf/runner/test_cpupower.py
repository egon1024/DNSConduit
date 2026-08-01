"""Unit tests for CPU scaling-governor / power-state preflight."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from perf.runner.cpupower import (
    PREFERRED_GOVERNOR,
    CpuPowerCheck,
    collect_available_governors,
    collect_scaling_governors,
    evaluate_cpu_power_state,
    format_power_state_guidance,
    require_cpu_power_ok,
)


def _write_governor_tree(
    root: Path,
    governors: dict[str, str],
    *,
    available: str | dict[str, str] | None = None,
) -> Path:
    """Create a fake ``…/cpu*/cpufreq/`` tree under *root*.

    *available* is either a single space-separated string written to every CPU,
    a per-CPU map, or ``None`` (omit ``scaling_available_governors``).
    """
    for cpu, gov in governors.items():
        d = root / cpu / "cpufreq"
        d.mkdir(parents=True)
        (d / "scaling_governor").write_text(gov + "\n", encoding="utf-8")
        if available is None:
            continue
        if isinstance(available, dict):
            avail_text = available.get(cpu)
            if avail_text is None:
                continue
        else:
            avail_text = available
        (d / "scaling_available_governors").write_text(
            avail_text + "\n", encoding="utf-8"
        )
    return root


class CollectScalingGovernorsTest(unittest.TestCase):
    def test_reads_all_cpu_governors(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = _write_governor_tree(
                Path(tmp),
                {"cpu0": "performance", "cpu1": "performance", "cpu2": "powersave"},
            )
            got = collect_scaling_governors(cpu_root=root)
            self.assertEqual(got, ["performance", "performance", "powersave"])

    def test_returns_none_when_no_cpufreq(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "cpu0").mkdir()
            self.assertIsNone(collect_scaling_governors(cpu_root=root))


class CollectAvailableGovernorsTest(unittest.TestCase):
    def test_unions_available_across_cpus(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = _write_governor_tree(
                Path(tmp),
                {"cpu0": "powersave", "cpu1": "powersave"},
                available={
                    "cpu0": "performance powersave",
                    "cpu1": "performance powersave ondemand",
                },
            )
            got = collect_available_governors(cpu_root=root)
            self.assertEqual(got, frozenset({"performance", "powersave", "ondemand"}))

    def test_returns_none_when_available_files_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = _write_governor_tree(Path(tmp), {"cpu0": "powersave"})
            self.assertIsNone(collect_available_governors(cpu_root=root))


class EvaluateCpuPowerStateTest(unittest.TestCase):
    def test_all_performance_is_ok(self) -> None:
        check = evaluate_cpu_power_state(
            ["performance", "performance"],
            available=frozenset({"performance", "powersave"}),
        )
        self.assertEqual(check.status, "ok")
        self.assertEqual(check.governors, frozenset({PREFERRED_GOVERNOR}))
        self.assertEqual(check.message, "")

    def test_powersave_is_suboptimal_when_performance_available(self) -> None:
        check = evaluate_cpu_power_state(
            ["powersave"],
            available=frozenset({"performance", "powersave"}),
        )
        self.assertEqual(check.status, "suboptimal")
        self.assertIn("powersave", check.governors)
        self.assertIn("powersave", check.message)
        self.assertIn("performance", check.message)

    def test_mixed_governors_are_suboptimal(self) -> None:
        check = evaluate_cpu_power_state(
            ["performance", "powersave"],
            available=frozenset({"performance", "powersave"}),
        )
        self.assertEqual(check.status, "suboptimal")
        self.assertEqual(check.governors, frozenset({"performance", "powersave"}))

    def test_schedutil_is_suboptimal_when_performance_available(self) -> None:
        check = evaluate_cpu_power_state(
            ["schedutil"],
            available=frozenset({"performance", "schedutil"}),
        )
        self.assertEqual(check.status, "suboptimal")

    def test_does_not_require_performance_when_not_in_available(self) -> None:
        # Common on some ARM / embedded boards: only ondemand/schedutil.
        check = evaluate_cpu_power_state(
            ["schedutil"],
            available=frozenset({"ondemand", "schedutil"}),
        )
        self.assertEqual(check.status, "ok")
        self.assertEqual(check.governors, frozenset({"schedutil"}))

    def test_unknown_available_still_requires_performance(self) -> None:
        # If we cannot read available governors, keep the strict default.
        check = evaluate_cpu_power_state(["powersave"], available=None)
        self.assertEqual(check.status, "suboptimal")

    def test_unavailable_when_none(self) -> None:
        check = evaluate_cpu_power_state(None)
        self.assertEqual(check.status, "unavailable")

    def test_unavailable_when_empty(self) -> None:
        check = evaluate_cpu_power_state([])
        self.assertEqual(check.status, "unavailable")


class FormatGuidanceTest(unittest.TestCase):
    def test_lists_all_remediation_alternatives(self) -> None:
        with mock.patch(
            "perf.runner.cpupower._tool_on_path",
            side_effect=lambda name: name in {"cpupower", "powerprofilesctl"},
        ):
            text = format_power_state_guidance(frozenset({"powersave"}))
        self.assertIn("sudo cpupower frequency-set -g performance", text)
        self.assertIn("powerprofilesctl set performance", text)
        self.assertIn("tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor", text)
        self.assertIn("--allow-suboptimal-cpu-power", text)
        self.assertIn("scaling_governor", text)

    def test_marks_missing_tools_but_still_lists_them(self) -> None:
        with mock.patch(
            "perf.runner.cpupower._tool_on_path",
            return_value=False,
        ):
            text = format_power_state_guidance(frozenset({"powersave"}))
        self.assertIn("cpupower", text)
        self.assertIn("not found on $PATH", text)
        self.assertIn("powerprofilesctl", text)
        # Sysfs fallback is always actionable without a package.
        self.assertIn("tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor", text)
        self.assertIn("sudo cpupower frequency-set -g powersave", text)

    def test_restore_uses_observed_governor(self) -> None:
        with mock.patch("perf.runner.cpupower._tool_on_path", return_value=True):
            text = format_power_state_guidance(frozenset({"ondemand"}))
        self.assertIn("frequency-set -g ondemand", text)


class RequireCpuPowerOkTest(unittest.TestCase):
    def test_ok_returns_none(self) -> None:
        self.assertIsNone(
            require_cpu_power_ok(
                CpuPowerCheck(status="ok", governors=frozenset({"performance"}), message="")
            )
        )

    def test_unavailable_returns_none(self) -> None:
        # Hosts without cpufreq (some VMs/containers) must not block runs.
        self.assertIsNone(
            require_cpu_power_ok(
                CpuPowerCheck(status="unavailable", governors=frozenset(), message="no cpufreq")
            )
        )

    def test_suboptimal_returns_message(self) -> None:
        msg = require_cpu_power_ok(
            CpuPowerCheck(
                status="suboptimal",
                governors=frozenset({"powersave"}),
                message="blocked: powersave",
            )
        )
        self.assertEqual(msg, "blocked: powersave")

    def test_suboptimal_allowed_returns_none(self) -> None:
        self.assertIsNone(
            require_cpu_power_ok(
                CpuPowerCheck(
                    status="suboptimal",
                    governors=frozenset({"powersave"}),
                    message="blocked: powersave",
                ),
                allow_suboptimal=True,
            )
        )


class EndToEndPreflightTest(unittest.TestCase):
    def test_powersave_tree_blocks_when_performance_available(self) -> None:
        from perf.runner.cpupower import check_host_cpu_power

        with tempfile.TemporaryDirectory() as tmp:
            root = _write_governor_tree(
                Path(tmp),
                {"cpu0": "powersave", "cpu1": "powersave"},
                available="performance powersave",
            )
            check = check_host_cpu_power(cpu_root=root)
            self.assertEqual(check.status, "suboptimal")
            self.assertIsNotNone(require_cpu_power_ok(check))
            self.assertIsNone(require_cpu_power_ok(check, allow_suboptimal=True))

    def test_arm_like_host_without_performance_governor_proceeds(self) -> None:
        from perf.runner.cpupower import check_host_cpu_power

        with tempfile.TemporaryDirectory() as tmp:
            root = _write_governor_tree(
                Path(tmp),
                {"cpu0": "schedutil", "cpu1": "schedutil"},
                available="ondemand schedutil",
            )
            check = check_host_cpu_power(cpu_root=root)
            self.assertEqual(check.status, "ok")
            self.assertIsNone(require_cpu_power_ok(check))


class CliFlagTest(unittest.TestCase):
    def test_run_parser_exposes_allow_flag(self) -> None:
        from perf.runner.__main__ import build_parser

        ns = build_parser().parse_args(
            [
                "run",
                "--conduit",
                "/tmp/conduit",
                "--allow-suboptimal-cpu-power",
            ]
        )
        self.assertTrue(ns.allow_suboptimal_cpu_power)

    def test_run_parser_defaults_to_refusing_suboptimal(self) -> None:
        from perf.runner.__main__ import build_parser

        ns = build_parser().parse_args(["run", "--conduit", "/tmp/conduit"])
        self.assertFalse(ns.allow_suboptimal_cpu_power)


if __name__ == "__main__":
    unittest.main()
