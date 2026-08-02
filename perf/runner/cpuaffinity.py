"""Best-effort CPU affinity pinning for hybrid P-core/E-core hosts.

On Intel hybrid CPUs (Alder Lake and newer), the OS scheduler is free to move
threads between performance (P) and efficient (E) cores from one process
launch to the next. Left unpinned, that alone can swing a single-shot
throughput measurement by 2-3x independent of anything Conduit, the loadgen,
or the scenario config actually does — noise that easily dwarfs the feature
deltas the harness is trying to measure.

When the kernel exposes the hybrid core topology (``/sys/devices/cpu_core``
and ``/sys/devices/cpu_atom``), pin the process under test (Conduit) to the
P-cores and the load generator / companion receivers to the E-cores, so a
run stays on a consistent core class throughout. Hosts without this
topology (most machines, including non-hybrid Intel/AMD CPUs) are
unaffected: detection returns ``None`` and callers skip pinning entirely.
"""

from __future__ import annotations

import shutil
from pathlib import Path

_CPU_CORE = Path("/sys/devices/cpu_core/cpus")
_CPU_ATOM = Path("/sys/devices/cpu_atom/cpus")


def detect_hybrid_cpusets() -> tuple[str, str] | None:
    """Return ``(performance_cpus, efficiency_cpus)`` as cpuset range strings
    (e.g. ``"0-15"``), or ``None`` when the host has no detected hybrid
    P-core/E-core split."""
    try:
        p = _CPU_CORE.read_text(encoding="utf-8").strip()
        e = _CPU_ATOM.read_text(encoding="utf-8").strip()
    except OSError:
        return None
    if not p or not e:
        return None
    return p, e


def parse_cpuset(cpuset: str | None) -> set[int]:
    """Expand a cpuset range string (``"0-7,16"``) into CPU indices.

    Returns an empty set for ``None``, empty, or unparseable input so callers
    can treat "no usable cpuset" as "do not pin".
    """
    if not cpuset:
        return set()
    cpus: set[int] = set()
    for part in cpuset.split(","):
        part = part.strip()
        if not part:
            continue
        try:
            if "-" in part:
                lo, hi = part.split("-", 1)
                cpus.update(range(int(lo), int(hi) + 1))
            else:
                cpus.add(int(part))
        except ValueError:
            return set()
    return cpus


def taskset_prefix(cpuset: str | None) -> list[str]:
    """Argv prefix that pins a subprocess to *cpuset* via ``taskset -c``.

    Returns an empty list when *cpuset* is ``None`` or ``taskset`` is not on
    ``$PATH`` — callers just run unpinned in that case.
    """
    if not cpuset:
        return []
    taskset = shutil.which("taskset")
    if not taskset:
        return []
    return [taskset, "-c", cpuset]
