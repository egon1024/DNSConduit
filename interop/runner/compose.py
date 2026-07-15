"""Compose orchestration and dig-based querying."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

from .catalog import Peer
from .conduit_merge import merge_conduit_profile
from .oracles import QueryResult
from .paths import COMPOSE_CELL, FIXTURES, INTEROP, PROFILES, ROOT
from .peer_packs import materialize_peer_config
from .peer_query_count import count_dnsmasq_queries
from .metrics_scrape import MetricSamples, scrape_metrics
from .setup_ir import SetupIR

# Conduit address on the cell bridge (see compose/cell.compose.yml).
CONDUIT_PEER_CLIENT_IP = "172.30.97.20"


def resolve_conduitctl() -> Path:
    """Locate host ``conduitctl`` for mid-case health control actions.

    Order: ``CONDUITCTL`` env, ``PATH``, then workspace ``target/{release,debug}``.
    """
    env = os.environ.get("CONDUITCTL", "").strip()
    if env:
        path = Path(env)
        if path.is_file() and os.access(path, os.X_OK):
            return path
        raise RuntimeError(f"CONDUITCTL={env!r} is not an executable file")
    which = shutil.which("conduitctl")
    if which:
        return Path(which)
    for rel in ("target/release/conduitctl", "target/debug/conduitctl"):
        candidate = ROOT / rel
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    raise RuntimeError(
        "conduitctl not found (set CONDUITCTL, install on PATH, or "
        "`cargo build -p conduitctl` so target/debug/conduitctl exists)"
    )


def docker_available() -> bool:
    return shutil.which("docker") is not None


def image_digest(image: str) -> str:
    """Best-effort digest for a local or remote image reference."""
    if not docker_available():
        return "unavailable"
    try:
        out = subprocess.check_output(
            [
                "docker",
                "image",
                "inspect",
                "--format",
                "{{index .RepoDigests 0}}",
                image,
            ],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        if out and out != "<no value>":
            # repo@sha256:...
            if "@" in out:
                return out.split("@", 1)[1]
            return out
        out = subprocess.check_output(
            [
                "docker",
                "image",
                "inspect",
                "--format",
                "{{.Id}}",
                image,
            ],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        return out or "unknown"
    except subprocess.CalledProcessError:
        return "unknown"


_FLAG_NAMES = ("qr", "aa", "tc", "rd", "ra", "ad", "cd")


def parse_dig(output: str) -> QueryResult:
    rcode = "UNKNOWN"
    answers: list[dict] = []
    flags: dict[str, bool] = {name: False for name in _FLAG_NAMES}
    nscount = 0
    arcount = 0
    edns_udp_size: int | None = None
    in_answer = False
    for line in output.splitlines():
        if "status:" in line:
            m = re.search(r"status:\s*([A-Z0-9]+)", line)
            if m:
                rcode = m.group(1)
        # ;; flags: qr aa rd ra; QUERY: 1, ANSWER: 1, AUTHORITY: 0, ADDITIONAL: 1
        if "flags:" in line.lower():
            m = re.search(r"flags:\s*([^;]+)", line, re.IGNORECASE)
            if m:
                present = {tok.lower() for tok in m.group(1).split()}
                for name in _FLAG_NAMES:
                    flags[name] = name in present
            m_ns = re.search(r"AUTHORITY:\s*(\d+)", line, re.IGNORECASE)
            if m_ns:
                nscount = int(m_ns.group(1))
            m_ar = re.search(r"ADDITIONAL:\s*(\d+)", line, re.IGNORECASE)
            if m_ar:
                arcount = int(m_ar.group(1))
        # ; EDNS: version: 0, flags:; udp: 1232
        if "EDNS:" in line.upper() or "edns:" in line.lower():
            m = re.search(r"udp:\s*(\d+)", line, re.IGNORECASE)
            if m:
                edns_udp_size = int(m.group(1))
        if line.strip() == ";; ANSWER SECTION:":
            in_answer = True
            continue
        if in_answer:
            if line.startswith(";;") or not line.strip():
                in_answer = False
                continue
            parts = line.split()
            if len(parts) >= 5:
                answers.append(
                    {
                        "name": parts[0],
                        "ttl": parts[1],
                        "class": parts[2],
                        "type": parts[3],
                        "rdata": " ".join(parts[4:]),
                    }
                )
    ancount = len(answers)
    m = re.search(r"ANSWER:\s*(\d+)", output)
    if m:
        ancount = int(m.group(1))
    return QueryResult(
        rcode=rcode,
        ancount=ancount,
        answers=answers,
        raw=output,
        flags=flags,
        nscount=nscount,
        arcount=arcount,
        edns_udp_size=edns_udp_size,
    )


def dig_query(
    server: str,
    port: int,
    qname: str,
    qtype: str = "A",
    timeout: float = 3.0,
    *,
    bufsize: int | None = None,
    dnssec: bool = False,
    notcp: bool = False,
    ignore_tc: bool = False,
    norecurse: bool = False,
) -> QueryResult:
    if not shutil.which("dig"):
        raise RuntimeError("dig is required for interop queries (install bind9-dnsutils)")
    cmd = [
        "dig",
        f"@{server}",
        "-p",
        str(port),
        qname,
        qtype,
        "+time=2",
        "+tries=1",
        "+noall",
        "+answer",
        "+comments",
        "+additional",
    ]
    if bufsize is not None:
        cmd.append(f"+bufsize={int(bufsize)}")
    if dnssec:
        cmd.append("+dnssec")
    if notcp:
        # Prefer UDP only; do not open TCP for the query.
        cmd.append("+notcp")
    if ignore_tc:
        # Do not retry over TCP when TC is set — observe the UDP reply as-is.
        cmd.append("+ignore")
    if norecurse:
        # Clear RD in the query (dig +nord) — needed for RD=0 peer quirks.
        cmd.append("+nord")
    try:
        out = subprocess.check_output(cmd, text=True, stderr=subprocess.STDOUT, timeout=timeout)
    except subprocess.CalledProcessError as exc:
        out = exc.output or str(exc)
    except subprocess.TimeoutExpired:
        return QueryResult(rcode="TIMEOUT", ancount=0, answers=[], raw="")
    return parse_dig(out)


class CellStack:
    """Bring up Conduit + one configured peer for a single matrix cell.

    The peer's product family (``peer.family``) resolves an
    ``interop/peers/<family>/`` pack; that pack's rendered ``compose.override.yml``
    (plus any ``prepare.py`` output) fully determines how the peer container
    answers queries. There is no role-based hardcoding here — packs own their
    daemon invocation.
    """

    def __init__(
        self,
        *,
        conduit_image: str,
        peer: Peer,
        profile_id: str,
        setup_ir: SetupIR,
        conduit_delta: dict[str, Any] | None = None,
        conduit_assets: list[dict[str, str]] | None = None,
        host_port: int = 15553,
        project: str = "conduit-interop",
    ):
        self.conduit_image = conduit_image
        self.peer = peer
        self.profile_id = profile_id
        self.setup_ir = setup_ir
        self.conduit_delta = conduit_delta or {}
        self.conduit_assets = list(conduit_assets or [])
        self.host_port = host_port
        self.peer_host_port = host_port + 1
        self.metrics_host_port = host_port + 2
        self.control_host_port = host_port + 3
        self.project = project
        self._tmpdir: tempfile.TemporaryDirectory[str] | None = None
        self._override: Path | None = None
        self._env: dict[str, str] | None = None

    @property
    def peer_query_addr(self) -> tuple[str, int]:
        return ("127.0.0.1", self.peer_host_port)

    @property
    def metrics_url(self) -> str:
        return f"http://127.0.0.1:{self.metrics_host_port}/metrics"

    @property
    def control_endpoint(self) -> str:
        return f"http://127.0.0.1:{self.control_host_port}"

    def peer_logs(self) -> str:
        """Return ``docker compose logs`` for the peer service (query log source)."""
        if self._env is None:
            raise RuntimeError("cell stack is not started")
        out = subprocess.check_output(
            [
                "docker",
                "compose",
                "-p",
                self.project,
                *self._compose_files(),
                "logs",
                "--no-color",
                "peer",
            ],
            cwd=ROOT,
            env=self._env,
            text=True,
            stderr=subprocess.STDOUT,
        )
        return out

    def count_peer_queries(
        self,
        qname: str,
        qtype: str = "A",
        *,
        from_conduit: bool = True,
    ) -> int:
        """Count matching query lines in the peer log (stub-peer cache-hit proof).

        Implemented only for the **dnsmasq** family (``--log-queries``). Conduit
        behavior cache cases pin that stub peer; do not treat this as a general
        peer-count API for other packs.

        When ``from_conduit`` is true (default), only count queries sourced from
        the Conduit container IP so readiness digs on the published port do not
        pollute the baseline used for cache hit proofs.
        """
        from_ip = CONDUIT_PEER_CLIENT_IP if from_conduit else None
        if self.peer.family == "dnsmasq":
            return count_dnsmasq_queries(
                self.peer_logs(), qname, qtype, from_ip=from_ip
            )
        raise RuntimeError(
            f"peer-query-count is a dnsmasq stub-peer cache proof only; "
            f"not implemented for family {self.peer.family!r}"
        )

    def scrape_conduit_metrics(self) -> MetricSamples:
        """HTTP scrape Conduit's Prometheus endpoint published on the host."""
        return scrape_metrics(self.metrics_url)

    def wait_for_metrics(self, attempts: int = 20, delay: float = 0.25) -> None:
        """Poll until /metrics responds (cache-forward profiles expose scrape)."""
        last_err: Exception | None = None
        for _ in range(attempts):
            try:
                self.scrape_conduit_metrics()
                return
            except RuntimeError as exc:
                last_err = exc
                time.sleep(delay)
        raise RuntimeError(
            f"metrics endpoint not ready at {self.metrics_url}: {last_err}"
        )

    def run_conduitctl(self, args: list[str], *, timeout: float = 10.0) -> None:
        """Run host ``conduitctl`` against this cell's published control port."""
        ctl = resolve_conduitctl()
        cmd = [str(ctl), "--endpoint", self.control_endpoint, *args]
        try:
            subprocess.check_call(
                cmd,
                cwd=ROOT,
                timeout=timeout,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
            )
        except subprocess.CalledProcessError as exc:
            err = (exc.stderr or b"").decode("utf-8", errors="replace").strip()
            raise RuntimeError(
                f"conduitctl failed ({exc.returncode}): {' '.join(args)}"
                + (f": {err}" if err else "")
            ) from exc
        except subprocess.TimeoutExpired as exc:
            raise RuntimeError(
                f"conduitctl timed out after {timeout}s: {' '.join(args)}"
            ) from exc

    def wait_for_control(self, attempts: int = 40, delay: float = 0.25) -> None:
        """Poll until ``conduitctl health show`` succeeds (control published)."""
        last_err: Exception | None = None
        for _ in range(attempts):
            try:
                self.run_conduitctl(["health", "show"])
                return
            except RuntimeError as exc:
                last_err = exc
                time.sleep(delay)
        raise RuntimeError(
            f"control endpoint not ready at {self.control_endpoint}: {last_err}"
        )

    def _compose_files(self) -> list[str]:
        files = ["-f", str(COMPOSE_CELL)]
        if self._override is not None:
            files += ["-f", str(self._override)]
        return files

    def _build_env(self, tmp: Path) -> dict[str, str]:
        env = os.environ.copy()
        env.update(
            {
                "PEER_IMAGE": self.peer.image,
                "CONDUIT_IMAGE": self.conduit_image,
                "CONDUIT_CONFIG": str((tmp / "conduit.yaml").resolve()),
                "CONDUIT_ASSETS_DIR": str((tmp / "assets").resolve()),
                "CONDUIT_HOST_PORT": str(self.host_port),
                "CONDUIT_METRICS_HOST_PORT": str(self.metrics_host_port),
                "CONDUIT_CONTROL_HOST_PORT": str(self.control_host_port),
                "PEER_CONFIG_DIR": str((tmp / "peer").resolve()),
                "PEER_HOST_PORT": str(self.peer_host_port),
            }
        )
        return env

    def start(self) -> None:
        if not docker_available():
            raise RuntimeError("docker is required to run interop cells")
        profile = PROFILES / f"{self.profile_id}.yml"
        if not profile.is_file():
            raise FileNotFoundError(profile)
        self._tmpdir = tempfile.TemporaryDirectory(prefix="conduit-interop-")
        tmp = Path(self._tmpdir.name)
        assets_dir = tmp / "assets"
        assets_dir.mkdir(parents=True, exist_ok=True)
        self._materialize_conduit_assets(assets_dir)
        merge_conduit_profile(profile, self.conduit_delta, tmp / "conduit.yaml")
        self._override = materialize_peer_config(
            family=self.peer.family,
            ir=self.setup_ir,
            out_dir=tmp / "peer",
            peer=self.peer,
        )
        self._env = self._build_env(tmp)
        cmd = [
            "docker",
            "compose",
            "-p",
            self.project,
            *self._compose_files(),
            "up",
            "-d",
            "--remove-orphans",
        ]
        try:
            subprocess.check_call(cmd, cwd=ROOT, env=self._env)
        except subprocess.CalledProcessError as exc:
            raise RuntimeError(
                f"docker compose up failed for peer image {self.peer.image!r} "
                f"(project {self.project}): exit {exc.returncode}"
            ) from exc
        self._wait_for_peer_ready()

    def _materialize_conduit_assets(self, assets_dir: Path) -> None:
        """Copy case-declared files into the cell assets dir (mounted at /etc/conduit/assets)."""
        for item in self.conduit_assets:
            src_rel = item.get("src", "")
            dest_rel = item.get("dest", "")
            if not src_rel or not dest_rel:
                raise ValueError("conduit_assets entries require src and dest")
            src = INTEROP / src_rel
            if not src.is_file():
                # Allow interop/fixtures/... or fixtures/... interchangeably.
                alt = FIXTURES / src_rel.removeprefix("fixtures/")
                src = alt if alt.is_file() else src
            if not src.is_file():
                raise FileNotFoundError(f"conduit asset missing: {src_rel}")
            dest = assets_dir / dest_rel
            if dest.resolve() != assets_dir.resolve() and assets_dir.resolve() not in dest.resolve().parents:
                raise ValueError(f"conduit asset dest escapes assets dir: {dest_rel}")
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dest)

    def _readiness_qname(self) -> tuple[str, bool]:
        """Return (probe qname, expect_answer).

        When the case supplies ``local_rr`` we know a name the peer must answer,
        so we can wait for a real ``NOERROR``. Otherwise we only probe that the
        peer is listening (any DNS response, including NXDOMAIN/REFUSED).
        """
        if self.setup_ir.local_rr:
            return self.setup_ir.local_rr[0].name.rstrip("."), True
        return "readiness-probe.invalid", False

    def _wait_for_peer_ready(self, attempts: int = 20, delay: float = 0.5) -> None:
        host, port = self.peer_query_addr
        qname, expect_answer = self._readiness_qname()
        responded = False
        last: QueryResult | None = None
        for _ in range(attempts):
            result = dig_query(host, port, qname)
            last = result
            if result.rcode not in ("UNKNOWN", "TIMEOUT"):
                responded = True
                # If we know the peer should answer this name, keep waiting for
                # a real answer; otherwise a response alone proves it is up.
                if not expect_answer or (result.rcode == "NOERROR" and result.ancount >= 1):
                    return
            time.sleep(delay)
        if not responded:
            # The peer never produced any DNS response — fail loudly rather than
            # letting the oracle report a misleading UNKNOWN/TIMEOUT.
            raise RuntimeError(
                f"peer {self.peer.id} ({self.peer.image}) did not answer on "
                f"{host}:{port} after {attempts} attempts "
                f"(last rcode={last.rcode if last else 'n/a'})"
            )
        # Peer is listening but did not return the expected answer within the
        # window; proceed so the oracle can record the real mismatch detail.

    def stop(self) -> None:
        if not docker_available():
            return
        env = self._env or os.environ.copy()
        subprocess.call(
            ["docker", "compose", "-p", self.project, *self._compose_files(), "down", "-v", "--remove-orphans"],
            cwd=ROOT,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if self._tmpdir:
            self._tmpdir.cleanup()
            self._tmpdir = None

    def __enter__(self) -> CellStack:
        self.start()
        return self

    def __exit__(self, *args) -> None:
        self.stop()
