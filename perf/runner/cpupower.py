"""CPU frequency governor / power-state preflight for the performance harness.

Publish-quality throughput cells are sensitive to host frequency scaling. A
``powersave`` (or other non-``performance``) governor can swing single-shot
QPS by multiple× on the same binary and scenario — noise that dwarfs the
feature deltas the harness is measuring.

Before any scenario executes, ``run`` checks that every online CPU reporting
a scaling governor is in ``performance`` **when that governor is offered** by
the host (``scaling_available_governors``). Boards that only expose
``ondemand`` / ``schedutil`` (common on some ARM images) are not blocked —
there is no better governor to require. Hosts without cpufreq sysfs (some
VMs/containers) skip the check rather than blocking.

Remediation guidance lists several alternatives (``cpupower``,
``powerprofilesctl``, direct sysfs write, and ``--allow-suboptimal-cpu-power``),
noting which helper tools are present on ``$PATH``.
"""

from __future__ import annotations

import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

PREFERRED_GOVERNOR = "performance"

_DEFAULT_CPU_ROOT = Path("/sys/devices/system/cpu")


@dataclass(frozen=True)
class CpuPowerCheck:
    """Result of inspecting host CPU frequency governors."""

    status: Literal["ok", "suboptimal", "unavailable"]
    governors: frozenset[str]
    message: str = ""
    available: frozenset[str] | None = None


def _tool_on_path(name: str) -> bool:
    return shutil.which(name) is not None


def _cpu_dirs(cpu_root: Path) -> list[Path]:
    try:
        return sorted(cpu_root.glob("cpu[0-9]*"))
    except OSError:
        return []


def collect_scaling_governors(
    *,
    cpu_root: Path = _DEFAULT_CPU_ROOT,
) -> list[str] | None:
    """Read ``scaling_governor`` for every CPU under *cpu_root*.

    Returns ``None`` when no cpufreq governor files are present (check not
    applicable on this host).
    """
    values: list[str] = []
    for cpu_dir in _cpu_dirs(cpu_root):
        gov_path = cpu_dir / "cpufreq" / "scaling_governor"
        try:
            text = gov_path.read_text(encoding="utf-8").strip()
        except OSError:
            continue
        if text:
            values.append(text)
    return values or None


def collect_available_governors(
    *,
    cpu_root: Path = _DEFAULT_CPU_ROOT,
) -> frozenset[str] | None:
    """Union of ``scaling_available_governors`` across CPUs, or ``None``.

    Returns ``None`` when no available-governor files are present (caller
    should fall back to requiring ``performance`` unconditionally).
    """
    names: set[str] = set()
    found = False
    for cpu_dir in _cpu_dirs(cpu_root):
        avail_path = cpu_dir / "cpufreq" / "scaling_available_governors"
        try:
            text = avail_path.read_text(encoding="utf-8").strip()
        except OSError:
            continue
        found = True
        names.update(part for part in text.split() if part)
    if not found:
        return None
    return frozenset(names)


def _tool_label(name: str, *, present: bool, install_hint: str) -> str:
    if present:
        return f"{name} (found on $PATH)"
    return f"{name} (not found on $PATH — {install_hint})"


def format_power_state_guidance(governors: frozenset[str]) -> str:
    """Human-facing block: why we refuse, and alternate ways to switch/restore."""
    observed = ", ".join(sorted(governors)) or "(unknown)"
    # Prefer restoring to the observed non-performance governor when unique;
    # otherwise fall back to powersave as the common desktop default.
    non_perf = sorted(g for g in governors if g != PREFERRED_GOVERNOR)
    restore = non_perf[0] if len(non_perf) == 1 else "powersave"

    has_cpupower = _tool_on_path("cpupower")
    has_ppd = _tool_on_path("powerprofilesctl")

    lines = [
        "CPU frequency governor is not in performance mode — refusing to run.",
        f"  Observed governor(s): {observed}",
        f"  Required: {PREFERRED_GOVERNOR} on every CPU with cpufreq "
        f"(when that governor is offered by the host)",
        "",
        "Powersave / ondemand / schedutil / mixed governors can swing measured",
        "QPS by multiple× independent of Conduit or the scenario under test.",
        "",
        "Alternatives to put the host into performance mode (requires root):",
        "",
        f"  1. {_tool_label('cpupower', present=has_cpupower, install_hint='install linux-tools / cpupower')}",
        f"       sudo cpupower frequency-set -g {PREFERRED_GOVERNOR}",
        f"       sudo cpupower frequency-set -g {restore}   # restore afterward",
        "",
        f"  2. {_tool_label('powerprofilesctl', present=has_ppd, install_hint='install power-profiles-daemon')}",
        "       powerprofilesctl set performance",
        "       powerprofilesctl set balanced   # typical restore",
        "",
        "  3. Direct sysfs write (works on any Linux with cpufreq; no extra package):",
        f"       echo {PREFERRED_GOVERNOR} | sudo tee "
        "/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor >/dev/null",
        f"       echo {restore} | sudo tee "
        "/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor >/dev/null"
        "   # restore",
        "",
        "Verify:",
        "",
        "  cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort -u",
        "",
        "  4. Run anyway on this suboptimal host (results may be noisy):",
        "",
        "       … run … --allow-suboptimal-cpu-power",
    ]
    return "\n".join(lines)


def evaluate_cpu_power_state(
    governors: list[str] | None,
    *,
    available: frozenset[str] | None = None,
) -> CpuPowerCheck:
    """Classify collected governors as ok / suboptimal / unavailable.

    When *available* is known and does **not** include ``performance``, the
    host cannot be asked to switch — treat current state as ``ok`` so ARM /
    embedded boards without that governor are not blocked.
    When *available* is ``None`` (sysfs absent), keep the strict default:
    require ``performance``.
    """
    if not governors:
        return CpuPowerCheck(
            status="unavailable",
            governors=frozenset(),
            available=available,
            message=(
                "CPU frequency governor check unavailable "
                "(no cpufreq scaling_governor sysfs); proceeding"
            ),
        )
    unique = frozenset(governors)
    if available is not None and PREFERRED_GOVERNOR not in available:
        return CpuPowerCheck(
            status="ok",
            governors=unique,
            available=available,
            message="",
        )
    if unique == {PREFERRED_GOVERNOR}:
        return CpuPowerCheck(
            status="ok",
            governors=unique,
            available=available,
            message="",
        )
    return CpuPowerCheck(
        status="suboptimal",
        governors=unique,
        available=available,
        message=format_power_state_guidance(unique),
    )


def check_host_cpu_power(*, cpu_root: Path = _DEFAULT_CPU_ROOT) -> CpuPowerCheck:
    """Inspect this host's CPU frequency governors and classify them."""
    return evaluate_cpu_power_state(
        collect_scaling_governors(cpu_root=cpu_root),
        available=collect_available_governors(cpu_root=cpu_root),
    )


def require_cpu_power_ok(
    check: CpuPowerCheck,
    *,
    allow_suboptimal: bool = False,
) -> str | None:
    """Return an error message when the run must abort, else ``None``.

    ``unavailable`` never blocks (no evidence of a powersave host).
    ``suboptimal`` blocks unless *allow_suboptimal* is true.
    """
    if check.status != "suboptimal":
        return None
    if allow_suboptimal:
        return None
    return check.message
