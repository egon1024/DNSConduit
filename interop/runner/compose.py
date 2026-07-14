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
from .paths import COMPOSE_CELL, PROFILES, ROOT
from .peer_packs import materialize_peer_config
from .setup_ir import SetupIR


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


def parse_dig(output: str) -> QueryResult:
    rcode = "UNKNOWN"
    answers: list[dict] = []
    in_answer = False
    for line in output.splitlines():
        if "status:" in line:
            m = re.search(r"status:\s*([A-Z0-9]+)", line)
            if m:
                rcode = m.group(1)
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
    return QueryResult(rcode=rcode, ancount=ancount, answers=answers, raw=output)


def dig_query(server: str, port: int, qname: str, qtype: str = "A", timeout: float = 3.0) -> QueryResult:
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
    ]
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
        host_port: int = 15553,
        project: str = "conduit-interop",
    ):
        self.conduit_image = conduit_image
        self.peer = peer
        self.profile_id = profile_id
        self.setup_ir = setup_ir
        self.conduit_delta = conduit_delta or {}
        self.host_port = host_port
        self.peer_host_port = host_port + 1
        self.project = project
        self._tmpdir: tempfile.TemporaryDirectory[str] | None = None
        self._override: Path | None = None
        self._env: dict[str, str] | None = None

    @property
    def peer_query_addr(self) -> tuple[str, int]:
        return ("127.0.0.1", self.peer_host_port)

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
                "CONDUIT_HOST_PORT": str(self.host_port),
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
