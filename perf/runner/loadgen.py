"""DNS-OARC dnsperf loadgen (Docker default, native override)."""

from __future__ import annotations

import re
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Sequence

from .paths import DNSPERF_DIR, QUERIES

# Thin in-repo image built from fixtures/dnsperf/Dockerfile (pinned upstream tag).
DEFAULT_IMAGE = "dnsconduit-dnsperf:2.14.0"
DEFAULT_QUERY_FILE = QUERIES / "perf-a.txt"


@dataclass
class DnsperfResult:
    achieved_qps: float | None = None
    offered_qps: float | None = None
    queries_sent: int | None = None
    queries_completed: int | None = None
    queries_lost: int | None = None
    latency_ms: dict[str, float] = field(default_factory=dict)
    raw_stdout: str = ""
    raw_stderr: str = ""
    mode: str = "docker"
    image: str | None = None
    native_version: str | None = None
    flags: list[str] = field(default_factory=list)


@dataclass
class DnsperfHandle:
    """Background dnsperf process (for overlapping load during shutdown)."""

    proc: subprocess.Popen[str]
    mode: str
    image: str | None
    flags: list[str]
    offered_qps: float | None = None
    native_version: str | None = None

    def wait(self, timeout_s: float | None = None) -> DnsperfResult:
        try:
            stdout, stderr = self.proc.communicate(timeout=timeout_s)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            stdout, stderr = self.proc.communicate(timeout=10)
        text = (stdout or "") + "\n" + (stderr or "")
        parsed = parse_dnsperf_output(text)
        parsed.mode = self.mode
        parsed.image = self.image
        parsed.flags = list(self.flags)
        parsed.raw_stderr = stderr or ""
        parsed.offered_qps = self.offered_qps
        parsed.native_version = self.native_version
        if self.proc.returncode not in (0, None) and parsed.achieved_qps is None:
            # Under shutdown, non-zero exit with partial stats is expected.
            if parsed.queries_sent is None and parsed.queries_lost is None:
                raise RuntimeError(
                    f"dnsperf failed (exit {self.proc.returncode}): "
                    f"{(stderr or stdout or '')[:2000]}"
                )
        return parsed

    def terminate(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()

    def kill(self) -> None:
        if self.proc.poll() is None:
            self.proc.kill()


_QPS_RE = re.compile(
    r"Queries per second:\s+([0-9.]+)",
    re.IGNORECASE,
)
_SENT_RE = re.compile(r"Queries sent:\s+(\d+)", re.IGNORECASE)
_COMPLETED_RE = re.compile(r"Queries completed:\s+(\d+)", re.IGNORECASE)
_LOST_RE = re.compile(r"Queries lost:\s+(\d+)", re.IGNORECASE)
# dnsperf latency lines vary by version; tolerate common patterns.
_LAT_AVG_RE = re.compile(r"Average Latency \(s\):\s+([0-9.]+)", re.IGNORECASE)
_LAT_MIN_RE = re.compile(r"Latency Min/Max \(s\):\s+([0-9.]+)/([0-9.]+)", re.IGNORECASE)


def parse_dnsperf_output(text: str) -> DnsperfResult:
    result = DnsperfResult(raw_stdout=text)
    if m := _QPS_RE.search(text):
        result.achieved_qps = float(m.group(1))
    if m := _SENT_RE.search(text):
        result.queries_sent = int(m.group(1))
    if m := _COMPLETED_RE.search(text):
        result.queries_completed = int(m.group(1))
    if m := _LOST_RE.search(text):
        result.queries_lost = int(m.group(1))
    lat: dict[str, float] = {}
    if m := _LAT_AVG_RE.search(text):
        lat["avg"] = float(m.group(1)) * 1000.0
    if m := _LAT_MIN_RE.search(text):
        lat["min"] = float(m.group(1)) * 1000.0
        lat["max"] = float(m.group(2)) * 1000.0
    result.latency_ms = lat
    return result


def docker_available() -> bool:
    return shutil.which("docker") is not None


def docker_dnsperf_cmd(
    *,
    image: str,
    query_dir: Path,
    flags: Sequence[str],
) -> list[str]:
    """Build docker run argv for the pinned dnsperf image.

    The image ENTRYPOINT is already ``dnsperf``, so *flags* are appended as
    arguments only — do not pass a second ``dnsperf`` token.
    """
    return [
        "docker",
        "run",
        "--rm",
        "--network=host",
        "-v",
        f"{query_dir}:/queries:ro",
        image,
        *flags,
    ]


def build_dnsperf_image(*, image: str = DEFAULT_IMAGE) -> None:
    dockerfile = DNSPERF_DIR / "Dockerfile"
    if not dockerfile.is_file():
        raise FileNotFoundError(f"missing dnsperf Dockerfile: {dockerfile}")
    subprocess.check_call(
        ["docker", "build", "-t", image, "-f", str(dockerfile), str(DNSPERF_DIR)],
    )


def ensure_dnsperf_image(*, image: str = DEFAULT_IMAGE) -> None:
    """Build the pinned image if ``docker image inspect`` fails."""
    probe = subprocess.run(
        ["docker", "image", "inspect", image],
        capture_output=True,
        text=True,
        check=False,
    )
    if probe.returncode != 0:
        build_dnsperf_image(image=image)


def _dnsperf_flags(
    *,
    server: str,
    port: int,
    query_file: Path,
    clients: int,
    threads: int,
    limit_qps: int | None,
    time_s: int,
    mode: str,
    extra_flags: Sequence[str],
) -> list[str]:
    flags = [
        "-s",
        server,
        "-p",
        str(port),
        "-d",
        "/queries/perf-a.txt" if mode == "docker" else str(query_file),
        "-c",
        str(clients),
        "-T",
        str(threads),
        "-l",
        str(time_s),
    ]
    if limit_qps is not None:
        flags.extend(["-Q", str(limit_qps)])
    flags.extend(extra_flags)
    return flags


def _native_version() -> str:
    binary = shutil.which("dnsperf")
    if not binary:
        return "unknown"
    try:
        ver = subprocess.check_output(
            [binary, "-v"], stderr=subprocess.STDOUT, text=True, timeout=5
        )
        return ver.strip().splitlines()[0]
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def start_dnsperf(
    *,
    server: str = "127.0.2.1",
    port: int = 15353,
    query_file: Path = DEFAULT_QUERY_FILE,
    clients: int = 4,
    threads: int = 2,
    limit_qps: int | None = None,
    time_s: int = 10,
    mode: str = "docker",
    image: str = DEFAULT_IMAGE,
    extra_flags: Sequence[str] = (),
) -> DnsperfHandle:
    """Start dnsperf in the background (does not wait for completion)."""
    if not query_file.is_file():
        raise FileNotFoundError(f"query file not found: {query_file}")

    base_flags = _dnsperf_flags(
        server=server,
        port=port,
        query_file=query_file,
        clients=clients,
        threads=threads,
        limit_qps=limit_qps,
        time_s=time_s,
        mode=mode,
        extra_flags=extra_flags,
    )
    offered = float(limit_qps) if limit_qps is not None else None

    if mode == "native":
        binary = shutil.which("dnsperf")
        if not binary:
            raise FileNotFoundError(
                "native dnsperf not found on PATH; use Docker default or install dnsperf"
            )
        proc = subprocess.Popen(
            [binary, *base_flags],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        return DnsperfHandle(
            proc=proc,
            mode="native",
            image=None,
            flags=list(base_flags),
            offered_qps=offered,
            native_version=_native_version(),
        )

    if not docker_available():
        raise RuntimeError("Docker is required for the default dnsperf loadgen path")

    ensure_dnsperf_image(image=image)
    cmd = docker_dnsperf_cmd(
        image=image,
        query_dir=query_file.parent.resolve(),
        flags=base_flags,
    )
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return DnsperfHandle(
        proc=proc,
        mode="docker",
        image=image,
        flags=list(base_flags),
        offered_qps=offered,
    )


def run_dnsperf(
    *,
    server: str = "127.0.2.1",
    port: int = 15353,
    query_file: Path = DEFAULT_QUERY_FILE,
    clients: int = 4,
    threads: int = 2,
    limit_qps: int | None = None,
    time_s: int = 10,
    mode: str = "docker",
    image: str = DEFAULT_IMAGE,
    extra_flags: Sequence[str] = (),
) -> DnsperfResult:
    handle = start_dnsperf(
        server=server,
        port=port,
        query_file=query_file,
        clients=clients,
        threads=threads,
        limit_qps=limit_qps,
        time_s=time_s,
        mode=mode,
        image=image,
        extra_flags=extra_flags,
    )
    return handle.wait(timeout_s=float(time_s) + 60.0)
