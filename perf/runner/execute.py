"""Execute catalog scenarios against a Conduit binary."""

from __future__ import annotations

import shutil
import signal
import time
from pathlib import Path
from typing import Any

from .. import __version__
from .catalog import Scenario
from .conduit import conduit_version, probe_dns_answer, start_conduit
from .loadgen import DEFAULT_IMAGE, DnsperfResult, run_dnsperf, start_dnsperf
from .paths import CONFIGS, QUERIES
from .run_record import detect_lab_profile_runtime, utc_now_iso
from .upstream import StubUpstream, start_fast_upstream, start_slow_upstream


def _resolve_config(recipe: dict[str, Any]) -> Path:
    rel = recipe.get("config")
    if not rel:
        raise ValueError("scenario recipe missing config")
    path = CONFIGS / rel
    if not path.is_file():
        raise FileNotFoundError(f"fixture config not found: {path}")
    return path


def _start_upstream(kind: str | None) -> StubUpstream | None:
    if not kind or kind == "none":
        return None
    if kind == "fast":
        return start_fast_upstream()
    if kind == "slow":
        return start_slow_upstream(delay_ms=50.0)
    raise ValueError(f"unknown upstream recipe: {kind}")


def _skip_reason(scenario: Scenario, *, otlp_tracer: Path | None, zdu: bool) -> str | None:
    gate = (scenario.recipe or {}).get("skip_unless")
    if gate == "otlp_tracer":
        if otlp_tracer is None or not Path(otlp_tracer).is_file():
            return "conduit-otlp-metrics-tracer not available"
    if gate == "zdu":
        if not zdu:
            return "zero-downtime upgrade not available in this binary"
    if scenario.suite == "lossless_upgrade" and not zdu:
        return "zero-downtime upgrade not available in this binary"
    return None


def _metrics_from_dnsperf(dp: DnsperfResult) -> dict[str, Any]:
    metrics: dict[str, Any] = {}
    if dp.achieved_qps is not None:
        metrics["achieved_qps"] = dp.achieved_qps
    if dp.offered_qps is not None:
        metrics["offered_qps"] = dp.offered_qps
    if dp.queries_sent is not None:
        metrics["queries_sent"] = dp.queries_sent
    if dp.queries_completed is not None:
        metrics["queries_completed"] = dp.queries_completed
    if dp.queries_lost is not None:
        metrics["queries_lost"] = dp.queries_lost
    if dp.latency_ms:
        metrics["latency_ms"] = dict(dp.latency_ms)
    return metrics


def run_scenario(
    scenario: Scenario,
    *,
    conduit: Path,
    loadgen_mode: str = "docker",
    loadgen_image: str = DEFAULT_IMAGE,
    time_s: int = 10,
    warmup_s: float = 2.0,
    otlp_tracer: Path | None = None,
    zdu: bool = False,
    annotation_ids: list[str] | None = None,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "id": scenario.id,
        "suite": scenario.suite,
        "intent": scenario.intent,
        "status": "ok",
        "axes": dict(scenario.axes),
    }
    if annotation_ids:
        result["annotation_ids"] = list(annotation_ids)

    skip = _skip_reason(scenario, otlp_tracer=otlp_tracer, zdu=zdu)
    if skip:
        result["status"] = "skip"
        result["skip_reason"] = skip
        return result

    recipe = scenario.recipe
    if recipe.get("loadgen") == "none" and scenario.suite == "lifecycle":
        return _run_lifecycle(
            scenario,
            conduit=conduit,
            warmup_s=warmup_s,
            result=result,
        )

    if recipe.get("shutdown"):
        return _run_shutdown_drain(
            scenario,
            conduit=conduit,
            loadgen_mode=loadgen_mode,
            loadgen_image=loadgen_image,
            time_s=time_s,
            warmup_s=warmup_s,
            result=result,
        )

    upstream = None
    cp = None
    try:
        upstream = _start_upstream(recipe.get("upstream"))
        config = _resolve_config(recipe)
        cp = start_conduit(conduit, config)

        if recipe.get("cache_warm"):
            # Warm lookup cache with a few successful answers before load.
            for _ in range(20):
                probe_dns_answer(cp.listen_host, cp.listen_port)
                time.sleep(0.05)

        if warmup_s > 0 and recipe.get("loadgen") == "dnsperf":
            time.sleep(warmup_s)

        metrics: dict[str, Any] = {}
        quality = {
            "warmup_seconds": warmup_s,
            "duration_seconds": float(time_s),
        }

        if recipe.get("loadgen") == "dnsperf":
            qfile_name = recipe.get("query_file") or "perf-a.txt"
            qfile = QUERIES / qfile_name
            dp = run_dnsperf(
                server=cp.listen_host,
                port=cp.listen_port,
                query_file=qfile,
                time_s=time_s,
                mode=loadgen_mode,
                image=loadgen_image,
            )
            metrics = _metrics_from_dnsperf(dp)

        result["metrics"] = metrics
        result["quality"] = quality
        return result
    except Exception as exc:  # noqa: BLE001 — record per-scenario error
        result["status"] = "error"
        result["error"] = str(exc)
        return result
    finally:
        if cp is not None:
            try:
                cp.stop(sig=signal.SIGTERM, wait_s=15.0)
            except Exception:
                pass
        if upstream is not None:
            upstream.stop()


def _run_shutdown_drain(
    scenario: Scenario,
    *,
    conduit: Path,
    loadgen_mode: str,
    loadgen_image: str,
    time_s: int,
    warmup_s: float,
    result: dict[str, Any],
) -> dict[str, Any]:
    """Stop Conduit under concurrent load; record drain duration and client loss.

    Flow: start stub + Conduit → establish dnsperf load → SIGTERM Conduit while
    load continues → wait for process exit (drain_duration_ms) → collect dnsperf
    stats (queries_lost → secondary.client_failures_during_stop).
    """
    recipe = scenario.recipe
    upstream = None
    cp = None
    load_handle = None
    try:
        upstream = _start_upstream(recipe.get("upstream") or "slow")
        config = _resolve_config(recipe)
        cp = start_conduit(conduit, config)

        if warmup_s > 0:
            time.sleep(warmup_s)

        establish_s = float(recipe.get("establish_load_s", 2.0))
        stop_wait_s = float(recipe.get("stop_wait_s", 20.0))
        # Keep -l short enough for smoke labs but long enough to cover
        # establish + drain wait; let dnsperf finish naturally so it emits stats.
        default_load = int(establish_s + min(stop_wait_s, 8.0) + 2)
        load_time_s = int(recipe.get("load_time_s", max(time_s, default_load)))

        qfile_name = recipe.get("query_file") or "perf-a.txt"
        qfile = QUERIES / qfile_name
        load_handle = start_dnsperf(
            server=cp.listen_host,
            port=cp.listen_port,
            query_file=qfile,
            time_s=load_time_s,
            mode=loadgen_mode,
            image=loadgen_image,
        )

        time.sleep(establish_s)

        drain_s = cp.stop(sig=signal.SIGTERM, wait_s=stop_wait_s)
        cp = None

        # Let dnsperf finish its -l window so it emits Queries lost / QPS.
        dp = load_handle.wait(timeout_s=float(load_time_s) + 15.0)
        load_handle = None

        metrics = _metrics_from_dnsperf(dp)
        metrics["drain_duration_ms"] = round(drain_s * 1000.0, 3)
        secondary: dict[str, Any] = {}
        if dp.queries_lost is not None:
            secondary["client_failures_during_stop"] = dp.queries_lost

        result["metrics"] = metrics
        result["quality"] = {
            "warmup_seconds": warmup_s,
            "duration_seconds": float(load_time_s),
            "notes": (
                f"shutdown under load: establish_load_s={establish_s}, "
                f"stop_wait_s={stop_wait_s}"
            ),
        }
        if secondary:
            result["secondary"] = secondary
        return result
    except Exception as exc:  # noqa: BLE001
        result["status"] = "error"
        result["error"] = str(exc)
        return result
    finally:
        if load_handle is not None:
            try:
                load_handle.kill()
                load_handle.wait(timeout_s=10.0)
            except Exception:
                pass
        if cp is not None:
            try:
                cp.stop(sig=signal.SIGTERM, wait_s=15.0)
            except Exception:
                pass
        if upstream is not None:
            upstream.stop()


def _run_lifecycle(
    scenario: Scenario,
    *,
    conduit: Path,
    warmup_s: float,
    result: dict[str, Any],
) -> dict[str, Any]:
    upstream = None
    cp = None
    try:
        recipe = scenario.recipe
        upstream = _start_upstream(recipe.get("upstream") or "fast")
        config = _resolve_config(recipe)
        t0 = time.monotonic()
        cp = start_conduit(conduit, config)
        cold_ms = (time.monotonic() - t0) * 1000.0
        result["metrics"] = {"cold_start_ms": round(cold_ms, 3)}
        result["quality"] = {"warmup_seconds": warmup_s}
        return result
    except Exception as exc:  # noqa: BLE001
        result["status"] = "error"
        result["error"] = str(exc)
        return result
    finally:
        if cp is not None:
            try:
                cp.stop()
            except Exception:
                pass
        if upstream is not None:
            upstream.stop()


def public_conduit_path(conduit: Path, *, cwd: Path | None = None) -> str:
    """Return a provenance path that avoids absolute home-directory leakage.

    Prefer a path relative to *cwd* when the binary lives under the working
    tree (e.g. ``target/release/conduit``). Otherwise store only the basename.
    """
    base = (cwd or Path.cwd()).resolve()
    try:
        resolved = conduit.expanduser().resolve()
    except OSError:
        return conduit.name or str(conduit)
    try:
        rel = resolved.relative_to(base)
    except ValueError:
        return resolved.name
    text = rel.as_posix()
    return text if text else resolved.name


def build_run_document(
    scenario_results: list[dict[str, Any]],
    *,
    conduit: Path,
    profile_id: str = "local",
    loadgen_mode: str = "docker",
    loadgen_image: str = DEFAULT_IMAGE,
    warmup_s: float = 2.0,
    time_s: int = 10,
    run_annotation_ids: list[str] | None = None,
) -> dict[str, Any]:
    lab = detect_lab_profile_runtime(profile_id=profile_id)
    loadgen: dict[str, Any] = {"tool": "dnsperf", "mode": loadgen_mode}
    if loadgen_mode == "docker":
        loadgen["image"] = loadgen_image
    doc: dict[str, Any] = {
        "schema_version": 1,
        "generated_at": utc_now_iso(),
        "lab_profile": lab,
        "provenance": {
            "conduit_path": public_conduit_path(conduit),
            "conduit_version": conduit_version(conduit),
            "runner": f"perf.runner/{__version__}",
            "loadgen": loadgen,
        },
        "quality": {
            "warmup_seconds": warmup_s,
            "duration_seconds": float(time_s),
        },
        "scenarios": scenario_results,
    }
    if run_annotation_ids:
        doc["annotation_ids"] = list(run_annotation_ids)
    git = shutil.which("git")
    if git:
        try:
            import subprocess

            sha = subprocess.check_output(
                [git, "rev-parse", "HEAD"],
                cwd=str(Path.cwd()),
                text=True,
                timeout=5,
            ).strip()
            doc["provenance"]["git_sha"] = sha
        except Exception:
            pass
    return doc
