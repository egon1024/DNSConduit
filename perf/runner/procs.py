"""Child-process hygiene for measurement runs.

A lab process that outlives the run that started it is not a harmless stray.
It keeps its CPU affinity, so an orphaned Conduit sits on the very cores the
next run pins its own Conduit to and quietly taxes every cell measured after
it. That tax is invisible in the run document: throughput is simply lower,
uniformly, for reasons nothing in the record explains.

Defenses:

* ``die_with_parent`` asks the kernel to kill each child we spawn when the
  runner dies (Ctrl-C, editor killing a background job) — the case ``finally``
  blocks cannot cover.
* A on-disk **PID ledger** records every child this runner starts, with the
  kernel starttime so a recycled PID cannot be mistaken for our process.
  SIGKILL is issued only against ledger entries that still match that identity.
  There is no ``/proc`` marker scan that kills strangers.
"""

from __future__ import annotations

import atexit
import ctypes
import json
import os
import signal
import threading
from dataclasses import asdict, dataclass
from pathlib import Path

_PR_SET_PDEATHSIG = 1

# Per-runner ledgers live under /tmp so a crashed runner leaves a trail without
# writing into the results tree. Filename is the runner PID.
LEDGER_DIR = Path("/tmp/conduit-perf-ledger")

_lock = threading.Lock()
_entries: dict[int, "TrackedChild"] = {}
_atexit_registered = False


@dataclass(frozen=True)
class TrackedChild:
    """A process this harness started and is allowed to SIGKILL."""

    pid: int
    starttime: int
    kind: str
    cmdline: str


def die_with_parent() -> None:
    """``preexec_fn`` that asks for SIGKILL when the runner process exits.

    Linux-only and best effort: on any other platform, or if the call fails,
    the child simply keeps the default behavior and cleanup falls back to the
    caller's teardown path. Must only be used on children this harness spawns.
    """
    try:
        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        if libc.prctl(_PR_SET_PDEATHSIG, int(signal.SIGKILL), 0, 0, 0) != 0:
            return
    except (OSError, AttributeError):
        return


def process_starttime(pid: int) -> int | None:
    """Kernel starttime ticks from ``/proc/<pid>/stat``, or ``None`` if gone."""
    try:
        raw = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    except OSError:
        return None
    # ``pid (comm) state ppid ... starttime ...`` — comm may contain spaces/parens.
    rparen = raw.rfind(")")
    if rparen < 0:
        return None
    fields = raw[rparen + 2 :].split()
    try:
        return int(fields[19])
    except (IndexError, ValueError):
        return None


def process_cmdline(pid: int) -> str:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return ""
    return raw.replace(b"\x00", b" ").decode("utf-8", "replace").strip()


def _pid_alive(pid: int) -> bool:
    if pid <= 1:
        return False
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _ledger_path(runner_pid: int | None = None) -> Path:
    return LEDGER_DIR / f"{runner_pid if runner_pid is not None else os.getpid()}.json"


def _persist_locked() -> None:
    LEDGER_DIR.mkdir(parents=True, exist_ok=True)
    path = _ledger_path()
    payload = [asdict(e) for e in _entries.values()]
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    tmp.replace(path)


def _clear_ledger_file() -> None:
    try:
        _ledger_path().unlink()
    except OSError:
        pass


def _ensure_atexit() -> None:
    global _atexit_registered
    if _atexit_registered:
        return
    atexit.register(_atexit_cleanup)
    _atexit_registered = True


def _atexit_cleanup() -> None:
    """Best-effort: kill anything still in our ledger, then drop the file."""
    with _lock:
        leftovers = list(_entries.values())
        _entries.clear()
    for child in leftovers:
        kill_tracked(child)
    _clear_ledger_file()


def register_child(pid: int, *, kind: str) -> TrackedChild | None:
    """Record a child this runner just spawned. Returns ``None`` if it already exited."""
    if pid <= 1 or pid == os.getpid():
        return None
    starttime = process_starttime(pid)
    if starttime is None:
        return None
    child = TrackedChild(
        pid=pid,
        starttime=starttime,
        kind=kind,
        cmdline=process_cmdline(pid)[:500],
    )
    with _lock:
        _ensure_atexit()
        _entries[pid] = child
        _persist_locked()
    return child


def unregister_child(pid: int) -> None:
    with _lock:
        _entries.pop(pid, None)
        if _entries:
            _persist_locked()
        else:
            _clear_ledger_file()


def verify_tracked(child: TrackedChild) -> bool:
    """True only if *pid* is still the same process we recorded."""
    if child.pid <= 1 or child.pid == os.getpid():
        return False
    starttime = process_starttime(child.pid)
    return starttime is not None and starttime == child.starttime


def kill_tracked(child: TrackedChild) -> bool:
    """SIGKILL *child* only if its starttime still matches. Returns whether signalled."""
    if not verify_tracked(child):
        unregister_child(child.pid)
        return False
    try:
        os.kill(child.pid, signal.SIGKILL)
    except OSError:
        unregister_child(child.pid)
        return False
    unregister_child(child.pid)
    return True


def tracked_children() -> list[TrackedChild]:
    with _lock:
        return list(_entries.values())


def load_ledger_file(path: Path) -> list[TrackedChild]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return []
    out: list[TrackedChild] = []
    if not isinstance(data, list):
        return out
    for item in data:
        if not isinstance(item, dict):
            continue
        try:
            out.append(
                TrackedChild(
                    pid=int(item["pid"]),
                    starttime=int(item["starttime"]),
                    kind=str(item.get("kind") or "unknown"),
                    cmdline=str(item.get("cmdline") or ""),
                )
            )
        except (KeyError, TypeError, ValueError):
            continue
    return out


def find_orphaned_tracked() -> list[TrackedChild]:
    """Children recorded by a dead runner that are still alive and verified."""
    if not LEDGER_DIR.is_dir():
        return []
    self_pid = os.getpid()
    orphans: list[TrackedChild] = []
    for path in sorted(LEDGER_DIR.glob("*.json")):
        try:
            runner_pid = int(path.stem)
        except ValueError:
            continue
        # Never treat this runner's own ledger as orphans (those are still in use).
        if runner_pid == self_pid:
            continue
        # Another live runner still owns this ledger — leave it alone.
        if _pid_alive(runner_pid):
            continue
        for child in load_ledger_file(path):
            if verify_tracked(child):
                orphans.append(child)
        # Drop ledger files for dead runners that have no living children left.
        living = [c for c in load_ledger_file(path) if verify_tracked(c)]
        if not living:
            try:
                path.unlink()
            except OSError:
                pass
    return orphans


def kill_orphaned_tracked(orphans: list[TrackedChild]) -> int:
    """SIGKILL verified orphans only. Returns how many signals were sent."""
    killed = 0
    for child in orphans:
        if kill_tracked(child):
            killed += 1
        # Also remove the dead runner's ledger file if empty.
    if LEDGER_DIR.is_dir():
        for path in list(LEDGER_DIR.glob("*.json")):
            try:
                runner_pid = int(path.stem)
            except ValueError:
                continue
            if _pid_alive(runner_pid):
                continue
            living = [c for c in load_ledger_file(path) if verify_tracked(c)]
            if not living:
                try:
                    path.unlink()
                except OSError:
                    pass
    return killed


# Back-compat names used by __main__ / tests during the transition.
StrayProcess = TrackedChild


def find_stray_lab_processes() -> list[TrackedChild]:
    """Return verified orphans from dead runners' ledgers (never a /proc guess)."""
    return find_orphaned_tracked()


def kill_stray_lab_processes(strays: list[TrackedChild]) -> None:
    """Kill only the verified ledger orphans passed in."""
    kill_orphaned_tracked(strays)
