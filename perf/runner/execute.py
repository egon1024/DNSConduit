"""Execute catalog scenarios against a Conduit binary."""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import time
from pathlib import Path
from typing import Any

from .. import __version__
from .catalog import Scenario
from .companions import (
    CompanionProcess,
    ScrapeHammer,
    fetch_otlp_stats,
    resolve_conduitctl,
    resolve_dnstap_tracer,
    resolve_otlp_tracer,
    start_dnstap_tracer,
    start_otlp_tracer,
    start_scrape_hammer,
)
from .conduit import conduit_version, probe_dns_answer, start_conduit
from .cpuaffinity import detect_hybrid_cpusets
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


def _resolve_overlay(recipe: dict[str, Any]) -> Path:
    rel = recipe.get("overlay")
    if not rel:
        raise ValueError("scenario recipe missing overlay")
    path = CONFIGS / rel
    if not path.is_file():
        raise FileNotFoundError(f"fixture overlay not found: {path}")
    return path


def _start_upstream(kind: str | None, *, cpuset: str | None = None) -> StubUpstream | None:
    if not kind or kind == "none":
        return None
    if kind == "fast":
        return start_fast_upstream(cpuset=cpuset)
    if kind == "slow":
        return start_slow_upstream(delay_ms=50.0, cpuset=cpuset)
    raise ValueError(f"unknown upstream recipe: {kind}")


def _skip_reason(
    scenario: Scenario,
    *,
    otlp_tracer: Path | None,
    dnstap_tracer: Path | None,
    zdu: bool,
) -> str | None:
    recipe = scenario.recipe or {}
    gate = recipe.get("skip_unless")
    if gate == "otlp_tracer":
        if otlp_tracer is None or not Path(otlp_tracer).is_file():
            return "conduit-otlp-metrics-tracer not available"
    if gate == "zdu":
        if not zdu:
            return "zero-downtime upgrade not available in this binary"
    if scenario.suite == "lossless_upgrade" and not zdu:
        return "zero-downtime upgrade not available in this binary"
    if recipe.get("otlp_receiver"):
        if otlp_tracer is None or not Path(otlp_tracer).is_file():
            return "conduit-otlp-metrics-tracer not available"
    if recipe.get("dnstap_receiver"):
        if dnstap_tracer is None or not Path(dnstap_tracer).is_file():
            return "conduit-dnstap-tracer not available"
    return None


# A forward-path scenario that cannot forward still answers — with SERVFAIL, in
# microseconds — and the loadgen counts those as completed queries. Publishing
# such a cell would report rejection speed as throughput, so the harness treats a
# low successful-answer share as a failed measurement rather than a slow one.
DEFAULT_MIN_ANSWER_OK_PERCENT = 99.0
DEFAULT_EXPECTED_RCODE = "NOERROR"
DEFAULT_LOAD_SECONDS = 10


def _effective_time_s(recipe: dict[str, Any], time_s: int | None) -> int:
    """Load duration: explicit CLI value wins, else the recipe, else the default.

    Cells whose achieved QPS is tiny (a high-latency upstream against a runtime
    that blocks on it) need a longer window to complete enough queries for the
    number to mean anything.
    """
    if time_s is not None:
        return int(time_s)
    return int(recipe.get("duration_s") or DEFAULT_LOAD_SECONDS)


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
    if dp.response_codes:
        metrics["response_codes"] = dict(dp.response_codes)
    if dp.latency_ms:
        metrics["latency_ms"] = dict(dp.latency_ms)
    return metrics


def answer_ok_percent(
    response_codes: dict[str, int] | None,
    *,
    expected_rcode: str = DEFAULT_EXPECTED_RCODE,
) -> float | None:
    """Share of responses carrying the rcode the scenario is meant to measure."""
    if not response_codes:
        return None
    total = sum(response_codes.values())
    if total <= 0:
        return None
    return 100.0 * response_codes.get(expected_rcode, 0) / total


def answer_gate_settings(recipe: dict[str, Any]) -> tuple[str, float | None]:
    """Resolve (expected rcode, minimum ok share) for a scenario recipe.

    ``min_answer_ok_percent: 0`` disables the gate for scenarios where failed
    answers are the subject of the measurement (shutdown drain, policy drops).
    """
    expected = str(recipe.get("expect_rcode") or DEFAULT_EXPECTED_RCODE).upper()
    if "min_answer_ok_percent" in recipe:
        raw = recipe.get("min_answer_ok_percent")
        threshold = None if raw is None else float(raw)
        if threshold is not None and threshold <= 0:
            threshold = None
    else:
        threshold = DEFAULT_MIN_ANSWER_OK_PERCENT
    return expected, threshold


def _apply_answer_gate(
    result: dict[str, Any],
    *,
    recipe: dict[str, Any],
    metrics: dict[str, Any],
) -> None:
    """Mark the scenario invalid when too few responses are real answers."""
    expected, threshold = answer_gate_settings(recipe)
    share = answer_ok_percent(metrics.get("response_codes"), expected_rcode=expected)
    if share is None:
        return
    metrics["answer_ok_percent"] = round(share, 4)
    if threshold is None or share >= threshold:
        return
    codes = metrics.get("response_codes") or {}
    breakdown = ", ".join(f"{name} {count}" for name, count in sorted(codes.items()))
    result["status"] = "invalid"
    result["error"] = (
        f"answer gate: {share:.2f}% {expected} responses (need {threshold:.2f}%); "
        f"achieved_qps measures rejection, not throughput [{breakdown}]"
    )


def _effective_loadgen_knobs(
    recipe: dict[str, Any],
    *,
    clients: int,
    dnsperf_threads: int,
    max_outstanding: int | None,
) -> tuple[int, int, int | None]:
    """Resolve dnsperf concurrency: recipe knobs when set, else CLI args.

    CLI ``--max-outstanding`` still wins when provided (non-None) so lab
    overrides remain possible on recipe-pinned scenarios.
    """
    eff_clients = int(recipe["clients"]) if "clients" in recipe else clients
    eff_threads = (
        int(recipe["dnsperf_threads"])
        if "dnsperf_threads" in recipe
        else dnsperf_threads
    )
    if max_outstanding is not None:
        eff_max = max_outstanding
    elif "max_outstanding" in recipe:
        eff_max = int(recipe["max_outstanding"])
    else:
        eff_max = None
    return eff_clients, eff_threads, eff_max


def run_scenario(
    scenario: Scenario,
    *,
    conduit: Path,
    loadgen_mode: str = "docker",
    loadgen_image: str = DEFAULT_IMAGE,
    time_s: int | None = None,
    warmup_s: float = 2.0,
    clients: int = 4,
    dnsperf_threads: int = 2,
    max_outstanding: int | None = None,
    otlp_tracer: Path | None = None,
    dnstap_tracer: Path | None = None,
    conduitctl: Path | None = None,
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

    otlp = resolve_otlp_tracer(otlp_tracer, conduit=conduit)
    dnstap = resolve_dnstap_tracer(dnstap_tracer, conduit=conduit)
    ctl = resolve_conduitctl(conduitctl, conduit=conduit)
    hybrid = detect_hybrid_cpusets()
    p_cpus, e_cpus = hybrid if hybrid else (None, None)

    skip = _skip_reason(
        scenario, otlp_tracer=otlp, dnstap_tracer=dnstap, zdu=zdu
    )
    if skip:
        result["status"] = "skip"
        result["skip_reason"] = skip
        return result

    recipe = scenario.recipe
    clients, dnsperf_threads, max_outstanding = _effective_loadgen_knobs(
        recipe,
        clients=clients,
        dnsperf_threads=dnsperf_threads,
        max_outstanding=max_outstanding,
    )
    time_s = _effective_time_s(recipe, time_s)
    if recipe.get("loadgen") == "none" and scenario.suite == "lifecycle":
        return _run_lifecycle(
            scenario,
            conduit=conduit,
            conduitctl=ctl,
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
            clients=clients,
            dnsperf_threads=dnsperf_threads,
            max_outstanding=max_outstanding,
            result=result,
        )

    upstream = None
    cp = None
    companions: list[CompanionProcess] = []
    scrape_hammer: ScrapeHammer | None = None
    try:
        if recipe.get("otlp_receiver"):
            assert otlp is not None
            companions.append(start_otlp_tracer(otlp, cpuset=e_cpus))
        if recipe.get("dnstap_receiver"):
            assert dnstap is not None
            companions.append(start_dnstap_tracer(dnstap, cpuset=e_cpus))

        upstream = _start_upstream(recipe.get("upstream"), cpuset=e_cpus)
        config = _resolve_config(recipe)
        cp = start_conduit(conduit, config, cpuset=p_cpus)

        if recipe.get("cache_warm"):
            # Warm lookup cache with a few successful answers before load.
            for _ in range(20):
                probe_dns_answer(cp.listen_host, cp.listen_port)
                time.sleep(0.05)

        if recipe.get("scrape_hammer"):
            interval_ms = int(recipe.get("scrape_interval_ms") or 100)
            scrape_url = str(
                recipe.get("scrape_url") or "http://127.0.2.1:19090/metrics"
            )
            scrape_hammer = start_scrape_hammer(
                url=scrape_url, interval_ms=interval_ms
            )

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
                clients=clients,
                threads=dnsperf_threads,
                max_outstanding=max_outstanding,
                cpuset=e_cpus,
            )
            metrics = _metrics_from_dnsperf(dp)
            _apply_answer_gate(result, recipe=recipe, metrics=metrics)
            quality.update(
                _dnsperf_quality(
                    clients=clients,
                    dnsperf_threads=dnsperf_threads,
                    max_outstanding=max_outstanding,
                )
            )
            if upstream is not None:
                quality["upstream_workers"] = upstream.workers

        secondary: dict[str, Any] = {}
        for companion in companions:
            if companion.kind == "otlp_tracer" and companion.listen:
                # Allow one more push interval after load before sampling.
                time.sleep(2.5)
                stats = fetch_otlp_stats(companion.listen)
                if stats:
                    secondary.update(stats)
        if scrape_hammer is not None:
            secondary.update(scrape_hammer.stats())

        result["metrics"] = metrics
        result["quality"] = quality
        if secondary:
            result["secondary"] = secondary
        return result
    except Exception as exc:  # noqa: BLE001 — record per-scenario error
        result["status"] = "error"
        result["error"] = str(exc)
        return result
    finally:
        if scrape_hammer is not None:
            try:
                scrape_hammer.stop()
            except Exception:
                pass
        if cp is not None:
            try:
                cp.stop(sig=signal.SIGTERM, wait_s=15.0)
            except Exception:
                pass
        if upstream is not None:
            upstream.stop()
        for companion in companions:
            try:
                companion.stop()
            except Exception:
                pass


def _dnsperf_quality(
    *,
    clients: int,
    dnsperf_threads: int,
    max_outstanding: int | None,
) -> dict[str, Any]:
    """Loadgen knobs recorded on scenario/run quality for reproduce and docs."""
    out: dict[str, Any] = {
        "dnsperf_clients": clients,
        "dnsperf_threads": dnsperf_threads,
    }
    if max_outstanding is not None:
        out["dnsperf_max_outstanding"] = max_outstanding
    return out


def _run_shutdown_drain(
    scenario: Scenario,
    *,
    conduit: Path,
    loadgen_mode: str,
    loadgen_image: str,
    time_s: int,
    warmup_s: float,
    clients: int = 4,
    dnsperf_threads: int = 2,
    max_outstanding: int | None = None,
    result: dict[str, Any],
) -> dict[str, Any]:
    """Stop Conduit under concurrent load; record drain duration and client loss.

    Flow: start stub + Conduit → establish dnsperf load → SIGTERM Conduit while
    load continues → wait for process exit (drain_duration_ms) → collect dnsperf
    stats (queries_lost → secondary.client_failures_during_stop).
    """
    recipe = scenario.recipe
    hybrid = detect_hybrid_cpusets()
    p_cpus, e_cpus = hybrid if hybrid else (None, None)
    upstream = None
    cp = None
    load_handle = None
    try:
        upstream = _start_upstream(recipe.get("upstream") or "slow", cpuset=e_cpus)
        config = _resolve_config(recipe)
        cp = start_conduit(conduit, config, cpuset=p_cpus)

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
            clients=clients,
            threads=dnsperf_threads,
            max_outstanding=max_outstanding,
            cpuset=e_cpus,
        )

        time.sleep(establish_s)

        drain_s = cp.stop(sig=signal.SIGTERM, wait_s=stop_wait_s)
        cp = None

        # Let dnsperf finish its -l window so it emits Queries lost / QPS.
        dp = load_handle.wait(timeout_s=float(load_time_s) + 15.0)
        load_handle = None

        metrics = _metrics_from_dnsperf(dp)
        metrics["drain_duration_ms"] = round(drain_s * 1000.0, 3)
        _apply_answer_gate(result, recipe=recipe, metrics=metrics)
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
            **_dnsperf_quality(
                clients=clients,
                dnsperf_threads=dnsperf_threads,
                max_outstanding=max_outstanding,
            ),
        }
        if upstream is not None:
            result["quality"]["upstream_workers"] = upstream.workers
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
    conduitctl: Path | None,
    warmup_s: float,
    result: dict[str, Any],
) -> dict[str, Any]:
    upstream = None
    cp = None
    hybrid = detect_hybrid_cpusets()
    p_cpus, e_cpus = hybrid if hybrid else (None, None)
    try:
        recipe = scenario.recipe
        kind = recipe.get("lifecycle") or "cold_start"
        upstream = _start_upstream(recipe.get("upstream") or "fast", cpuset=e_cpus)
        config = _resolve_config(recipe)

        if kind == "cold_start":
            t0 = time.monotonic()
            cp = start_conduit(conduit, config, cpuset=p_cpus)
            cold_ms = (time.monotonic() - t0) * 1000.0
            result["metrics"] = {"cold_start_ms": round(cold_ms, 3)}
            result["quality"] = {"warmup_seconds": warmup_s}
            return result

        if kind == "config_apply":
            if conduitctl is None or not Path(conduitctl).is_file():
                result["status"] = "skip"
                result["skip_reason"] = "conduitctl not available"
                return result
            overlay = _resolve_overlay(recipe)
            cp = start_conduit(conduit, config, cpuset=p_cpus)
            if warmup_s > 0:
                time.sleep(min(warmup_s, 1.0))
            env = os.environ.copy()
            env["CONDUIT_CONTROL"] = "http://127.0.2.1:5199"
            t0 = time.monotonic()
            subprocess.check_call(
                [str(conduitctl), "apply", "--file", str(overlay)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                timeout=30,
                env=env,
            )
            apply_ms = (time.monotonic() - t0) * 1000.0
            # Confirm dataplane still answers after apply.
            if not probe_dns_answer(cp.listen_host, cp.listen_port):
                raise RuntimeError("DNS probe failed after config apply")
            result["metrics"] = {"apply_latency_ms": round(apply_ms, 3)}
            result["quality"] = {"warmup_seconds": warmup_s}
            return result

        raise ValueError(f"unknown lifecycle recipe: {kind!r}")
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
    time_s: int | None = None,
    clients: int = 4,
    dnsperf_threads: int = 2,
    max_outstanding: int | None = None,
    run_annotation_ids: list[str] | None = None,
) -> dict[str, Any]:
    lab = detect_lab_profile_runtime(profile_id=profile_id)
    loadgen: dict[str, Any] = {
        "tool": "dnsperf",
        "mode": loadgen_mode,
        "clients": clients,
        "threads": dnsperf_threads,
    }
    if max_outstanding is not None:
        loadgen["max_outstanding"] = max_outstanding
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
        # Run-level duration is the default for cells that do not declare one;
        # each scenario records the window it actually ran under.
        "quality": {
            "warmup_seconds": warmup_s,
            "duration_seconds": float(
                time_s if time_s is not None else DEFAULT_LOAD_SECONDS
            ),
            **_dnsperf_quality(
                clients=clients,
                dnsperf_threads=dnsperf_threads,
                max_outstanding=max_outstanding,
            ),
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
