"""Run stage — single parameterized screen."""

from __future__ import annotations

import threading
from pathlib import Path

from textual import on, work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import (
    Button,
    Checkbox,
    Input,
    Log,
    ProgressBar,
    Rule,
    Static,
)

from perf.runner.api import (
    FacadeError,
    PreflightError,
    RunParams,
    RunProgressEvent,
    list_scenario_summaries,
    run_benchmarks,
)
from perf.runner.execute import DEFAULT_LOAD_SECONDS
from perf.tui.pipeline_state import PipelineState
from perf.tui.help import FlagWithHelp, LabeledField
from perf.tui.run_status import RunStatusModel
from perf.tui.scope import (
    ScopeSelection,
    default_conduit_path,
    default_run_cycles,
    default_run_output,
    default_run_profile,
    default_run_time,
    default_scope,
)
from perf.tui.screens.scope_modal import ScopePickerModal


class RunScreen(Vertical):
    def __init__(self, pipeline: PipelineState, **kwargs) -> None:
        super().__init__(**kwargs)
        self.pipeline = pipeline
        self._cancel = threading.Event()
        self._bench_active = False
        self._scope = default_scope()
        self._status = RunStatusModel()
        self._tick_timer = None

    def compose(self) -> ComposeResult:
        yield Static("Run", classes="stage-title")
        yield Static(
            "Configure a lab measurement, then start. Long runs stream progress below.",
            classes="stage-lead",
        )
        with Vertical(classes="card status-card", id="run-status-card"):
            yield Static("Idle", id="run-status-headline", classes="status-headline status-idle")
            yield ProgressBar(total=100, show_eta=False, id="run-progress")
            yield Static(
                "Started —   Ended —   Duration —",
                id="run-status-times",
                classes="status-times",
            )
            yield Static("No run yet", id="run-status-detail", classes="status-detail")
        with VerticalScroll(classes="stage-body"):
            with Vertical(classes="card"):
                yield Static("Target", classes="card-title")
                yield LabeledField("Conduit binary", help_id="run-conduit")
                yield Input(
                    value=default_conduit_path(),
                    placeholder="target/release/conduit",
                    id="run-conduit",
                )
                yield LabeledField("Lab profile id", help_id="run-profile")
                yield Input(value=default_run_profile(), id="run-profile")

            with Vertical(classes="card"):
                yield Static("Scope", classes="card-title")
                yield Static(
                    self._scope.summary(),
                    id="run-scope-summary",
                    classes="scope-summary",
                )
                with Horizontal(classes="btn-row"):
                    yield Button("Choose scope…", variant="primary", id="run-scope-edit")
                    yield Button("Reset to publish set", id="run-scope-reset")

            with Vertical(classes="card"):
                yield Static("Load", classes="card-title")
                with Horizontal(classes="field-row"):
                    with Vertical(classes="field"):
                        yield LabeledField("Cycles", help_id="run-cycles")
                        yield Input(value=default_run_cycles(), id="run-cycles")
                    with Vertical(classes="field"):
                        yield LabeledField("Time override (s)", help_id="run-time")
                        yield Input(
                            value=default_run_time(),
                            placeholder="empty = harness default",
                            id="run-time",
                        )
                yield LabeledField("Output path", help_id="run-output")
                yield Input(value=default_run_output(), id="run-output")
                with Horizontal(classes="flag-row"):
                    yield FlagWithHelp(
                        "Kill stray orphans",
                        help_id="run-kill-strays",
                        checkbox_id="run-kill-strays",
                        value=True,
                    )
                    yield FlagWithHelp(
                        "Allow suboptimal CPU",
                        help_id="run-allow-cpu",
                        checkbox_id="run-allow-cpu",
                    )
                    yield FlagWithHelp(
                        "Allow suboptimal UDP",
                        help_id="run-allow-udp",
                        checkbox_id="run-allow-udp",
                    )

            with Horizontal(classes="btn-row"):
                yield Button("Start run", variant="success", id="run-start")
                yield Button("Cancel after scenario", id="run-cancel")

            yield Rule()
            yield Static("Progress log", classes="card-title")
            yield Log(id="run-log", max_lines=2000, classes="run-log")

    def on_mount(self) -> None:
        self._refresh_scope_summary()
        self._paint_status()

    def _refresh_scope_summary(self) -> None:
        self.query_one("#run-scope-summary", Static).update(self._scope.summary())

    def _set_start_enabled(self, enabled: bool) -> None:
        try:
            self.query_one("#run-start", Button).disabled = not enabled
        except Exception:
            pass

    def _count_scenarios(self, params: RunParams) -> int:
        try:
            rows = list_scenario_summaries(
                suites=params.suites,
                scenario_ids=params.scenario_ids,
                study_ids=params.study_ids,
                curated_only=params.curated_only,
                publish_set=params.publish_set,
            )
            return len(rows)
        except Exception:
            return 0

    def _planned_cell_seconds(self, params: RunParams) -> float:
        load = float(params.time_s if params.time_s is not None else DEFAULT_LOAD_SECONDS)
        # warmup + rough process/setup slack per cell
        return load + float(params.warmup_s) + 8.0

    def _paint_status(self) -> None:
        headline, times, detail = self._status.render_lines()
        phase = self._status.phase
        hl = self.query_one("#run-status-headline", Static)
        hl.update(headline)
        hl.set_classes(f"status-headline status-{phase}")
        self.query_one("#run-status-times", Static).update(times)
        self.query_one("#run-status-detail", Static).update(detail)
        bar = self.query_one("#run-progress", ProgressBar)
        bar.update(progress=int(round(self._status.fraction() * 100)))

        card = self.query_one("#run-status-card")
        card.set_classes(f"card status-card status-card-{phase}")

    def _start_ticker(self) -> None:
        self._stop_ticker()
        self._tick_timer = self.set_interval(1.0, self._on_tick)

    def _stop_ticker(self) -> None:
        if self._tick_timer is not None:
            self._tick_timer.stop()
            self._tick_timer = None

    def _on_tick(self) -> None:
        if self._status.phase == "running":
            self._paint_status()

    def _begin_run(self, params: RunParams) -> None:
        self._bench_active = True
        self._set_start_enabled(False)
        n = self._count_scenarios(params)
        self._status.mark_start(
            cycles=params.cycles,
            scenarios_per_cycle=n,
            planned_cell_seconds=self._planned_cell_seconds(params),
            detail=f"Starting · {n} scenario(s) × {params.cycles} cycle(s)",
        )
        self._paint_status()
        self._start_ticker()

    def _after_finish_ui(self) -> None:
        self._stop_ticker()
        self._set_start_enabled(True)
        self._paint_status()

    @on(Button.Pressed, "#run-scope-edit")
    def _open_scope_modal(self, event: Button.Pressed) -> None:
        event.stop()

        def apply(result: ScopeSelection | None) -> None:
            if result is None:
                return
            self._scope = result
            self._refresh_scope_summary()

        self.app.push_screen(ScopePickerModal(self._scope), apply)

    @on(Button.Pressed, "#run-scope-reset")
    def _reset_scope(self, event: Button.Pressed) -> None:
        event.stop()
        self._scope = default_scope()
        self._refresh_scope_summary()
        self.app.notify("Scope reset to publish set (operator-docs reference)")

    @on(Button.Pressed, "#run-cancel")
    def _on_cancel(self, event: Button.Pressed) -> None:
        event.stop()
        self._cancel.set()
        if self._status.phase == "running":
            self._status.detail = "Cancel requested — finishing current scenario…"
            self._paint_status()
        self.query_one("#run-log", Log).write_line(
            "Cancel requested — will stop between scenarios…"
        )

    @on(Button.Pressed, "#run-start")
    def _on_start(self, event: Button.Pressed) -> None:
        event.stop()
        if self._bench_active:
            self.app.notify("A run is already in progress", severity="warning")
            return
        if self._scope.is_empty():
            self.app.notify("Choose a scope first", severity="warning")
            return
        try:
            params = self._parse_params()
        except (ValueError, TypeError) as exc:
            self.query_one("#run-log", Log).write_line(f"Invalid params: {exc}")
            return
        self._cancel.clear()
        self._begin_run(params)
        self.query_one("#run-log", Log).write_line(
            f"Starting run… scope: {self._scope.summary()}"
        )
        self._run_worker(params)

    def _parse_params(self) -> RunParams:
        conduit = Path(self.query_one("#run-conduit", Input).value.strip())
        cycles_s = self.query_one("#run-cycles", Input).value.strip() or "1"
        cycles = int(cycles_s)
        time_s_raw = self.query_one("#run-time", Input).value.strip()
        time_s = int(time_s_raw) if time_s_raw else None
        out_raw = self.query_one("#run-output", Input).value.strip()
        output = Path(out_raw) if out_raw else None
        kwargs: dict = {
            "conduit": conduit,
            "profile_id": self.query_one("#run-profile", Input).value.strip()
            or "local",
            "cycles": cycles,
            "time_s": time_s,
            "output": output,
            "kill_strays": self.query_one("#run-kill-strays", Checkbox).value,
            "allow_suboptimal_cpu_power": self.query_one(
                "#run-allow-cpu", Checkbox
            ).value,
            "allow_suboptimal_udp_buffers": self.query_one(
                "#run-allow-udp", Checkbox
            ).value,
            "cancel_event": self._cancel,
            "on_progress": self._on_progress,
        }
        kwargs.update(self._scope.to_run_kwargs())
        return RunParams(**kwargs)

    def _on_progress(self, event: RunProgressEvent) -> None:
        msg = event.message or (
            f"{event.kind} {event.scenario_id or ''} "
            f"cycle={event.cycle}/{event.cycles} "
            f"[{event.index}/{event.total}]"
        )
        self.app.call_from_thread(self._apply_progress, event, msg.strip())

    def _apply_progress(self, event: RunProgressEvent, line: str) -> None:
        self._status.observe(event)
        self._paint_status()
        self._append_log(line)

    def _append_log(self, line: str) -> None:
        self.query_one("#run-log", Log).write_line(line)

    @work(thread=True, exclusive=True)
    def _run_worker(self, params: RunParams) -> None:
        ok = False
        detail = ""
        paths: list[Path] | None = None
        try:
            paths = run_benchmarks(params)
            ok = True
            detail = "Run complete"
        except (FacadeError, PreflightError) as exc:
            detail = f"ERROR: {exc}"
            self.app.call_from_thread(self._append_log, detail)
        except Exception as exc:  # pragma: no cover - lab faults
            detail = f"ERROR: {exc}"
            self.app.call_from_thread(self._append_log, detail)
        else:
            self.app.call_from_thread(self._on_run_done, paths)
        finally:
            # If cancel observed via progress, keep cancelled; else complete/fail.
            self._bench_active = False
            if self._status.phase == "running":
                self._status.mark_complete(ok=ok, detail=detail)
            try:
                self.app.call_from_thread(self._after_finish_ui)
            except Exception:
                pass

    def _on_run_done(self, paths: list[Path]) -> None:
        log = self.query_one("#run-log", Log)
        for p in paths:
            log.write_line(f"wrote {p}")
        if paths:
            self.pipeline.record_runs(paths)
            app = self.app
            if hasattr(app, "refresh_sync_badges"):
                app.refresh_sync_badges()  # type: ignore[attr-defined]
        log.write_line("Run finished.")
