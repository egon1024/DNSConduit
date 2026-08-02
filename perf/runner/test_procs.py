"""Unit tests for child-process hygiene (PID ledger + die_with_parent)."""

from __future__ import annotations

import os
import select
import subprocess
import sys
import time
import unittest

from perf.runner.procs import (
    LEDGER_DIR,
    TrackedChild,
    die_with_parent,
    find_orphaned_tracked,
    kill_orphaned_tracked,
    kill_tracked,
    process_starttime,
    register_child,
    tracked_children,
    unregister_child,
    verify_tracked,
)


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _readline_with_timeout(stream, timeout_s: float) -> str:
    ready, _, _ = select.select([stream], [], [], timeout_s)
    if not ready:
        raise TimeoutError(f"no line within {timeout_s}s")
    return stream.readline()


class DieWithParentTests(unittest.TestCase):
    def test_child_dies_when_runner_is_killed(self):
        """A killed runner must not leave a grandchild holding CPU."""
        launcher = (
            "import subprocess, sys, time\n"
            "sys.path.insert(0, %r)\n"
            "from perf.runner.procs import die_with_parent\n"
            "p = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],"
            " preexec_fn=die_with_parent)\n"
            "print(p.pid, flush=True)\n"
            "time.sleep(60)\n"
        ) % os.getcwd()
        parent = subprocess.Popen(
            [sys.executable, "-c", launcher],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        child_pid: int | None = None
        try:
            assert parent.stdout is not None
            line = _readline_with_timeout(parent.stdout, 5.0)
            self.assertTrue(line.strip(), parent.stderr.read() if parent.stderr else "")
            child_pid = int(line.strip())
            self.assertTrue(_pid_alive(child_pid))
            parent.kill()
            parent.wait(timeout=5)
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and _pid_alive(child_pid):
                time.sleep(0.05)
            self.assertFalse(_pid_alive(child_pid), "child outlived its runner")
        finally:
            if parent.poll() is None:
                parent.kill()
                parent.wait(timeout=5)
            if child_pid is not None and _pid_alive(child_pid):
                try:
                    os.kill(child_pid, 9)
                except OSError:
                    pass


class LedgerTests(unittest.TestCase):
    def tearDown(self):
        for child in list(tracked_children()):
            unregister_child(child.pid)

    def test_register_and_verify_round_trip(self):
        proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            child = register_child(proc.pid, kind="test-sleep")
            self.assertIsNotNone(child)
            assert child is not None
            self.assertTrue(verify_tracked(child))
            self.assertEqual(child.starttime, process_starttime(proc.pid))
        finally:
            proc.kill()
            proc.wait(timeout=5)
            unregister_child(proc.pid)

    def test_kill_tracked_refuses_recycled_or_wrong_starttime(self):
        fake = TrackedChild(
            pid=os.getpid(),
            starttime=0,
            kind="must-not-kill",
            cmdline="fake",
        )
        self.assertFalse(kill_tracked(fake))
        self.assertTrue(_pid_alive(os.getpid()))

    def test_kill_tracked_only_signals_verified_child(self):
        proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            child = register_child(proc.pid, kind="test-sleep")
            self.assertIsNotNone(child)
            assert child is not None
            self.assertTrue(kill_tracked(child))
            proc.wait(timeout=5)
            self.assertFalse(_pid_alive(proc.pid))
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait(timeout=5)
            unregister_child(proc.pid)

    def test_orphan_finder_sees_dead_runner_ledger(self):
        proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        # Pick a pid that is almost certainly dead and not 1.
        dead_runner = 2_000_000_000
        while _pid_alive(dead_runner):
            dead_runner -= 1
        path = LEDGER_DIR / f"{dead_runner}.json"
        try:
            child = register_child(proc.pid, kind="orphan-test")
            self.assertIsNotNone(child)
            assert child is not None
            # Move our live entry into a dead-runner ledger file.
            unregister_child(proc.pid)
            LEDGER_DIR.mkdir(parents=True, exist_ok=True)
            path.write_text(
                (
                    '[{"pid": %d, "starttime": %d, "kind": "orphan-test", '
                    '"cmdline": "test"}]\n'
                )
                % (child.pid, child.starttime),
                encoding="utf-8",
            )
            orphans = find_orphaned_tracked()
            matched = [o for o in orphans if o.pid == child.pid]
            self.assertTrue(matched)
            killed = kill_orphaned_tracked(matched)
            self.assertEqual(killed, 1)
            proc.wait(timeout=5)
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait(timeout=5)
            try:
                path.unlink()
            except OSError:
                pass


if __name__ == "__main__":
    unittest.main()
