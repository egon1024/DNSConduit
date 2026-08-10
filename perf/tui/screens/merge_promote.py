"""Merge & Promote stage — two panels."""

from __future__ import annotations

from pathlib import Path

from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, Checkbox, Input, Log, Static

from perf.runner.api import FacadeError, merge_median, promote
from perf.tui.help import FlagWithHelp, LabeledField
from perf.tui.pipeline_state import PipelineState
from perf.tui.scope import (
    default_merge_output,
    default_merge_sources,
    default_promote_source,
)


class MergePromoteScreen(Vertical):
    def __init__(self, pipeline: PipelineState, **kwargs) -> None:
        super().__init__(**kwargs)
        self.pipeline = pipeline

    def compose(self) -> ComposeResult:
        yield Static("Merge & Promote", classes="stage-title")
        yield Static(
            "Combine per-cycle run JSON files with a median merge, then promote "
            "into the reference warehouse.",
            classes="stage-lead",
        )
        with VerticalScroll(classes="stage-body"):
            with Vertical(classes="card"):
                yield Static("1 · Merge (median)", classes="card-title")
                yield LabeledField("Per-cycle run JSON paths", help_id="merge-sources")
                yield Input(
                    value=default_merge_sources(),
                    id="merge-sources",
                    placeholder="perf/results/runs/…/r1.json r2.json r3.json",
                )
                yield LabeledField("Merge output path", help_id="merge-output")
                yield Input(value=default_merge_output(), id="merge-output")
                yield Button("Merge median", variant="primary", id="merge-go")

            with Vertical(classes="card"):
                yield Static("2 · Promote", classes="card-title")
                yield LabeledField("Source run JSON", help_id="promote-source")
                yield Input(value=default_promote_source(), id="promote-source")
                with Horizontal(classes="field-row"):
                    with Vertical(classes="field"):
                        yield LabeledField("Reference name", help_id="promote-name")
                        yield Input(value="thin-spine", id="promote-name")
                    with Vertical(classes="field"):
                        yield LabeledField("Profile id", help_id="promote-profile")
                        yield Input(value="maintainer-ws-1", id="promote-profile")
                with Horizontal(classes="flag-row"):
                    yield FlagWithHelp(
                        "Publish-set filter",
                        help_id="promote-publish-set",
                        checkbox_id="promote-publish-set",
                        value=True,
                    )
                    yield FlagWithHelp(
                        "Thin-spine filter",
                        help_id="promote-thin-spine",
                        checkbox_id="promote-thin-spine",
                    )
                yield Button("Promote", variant="success", id="promote-go")

            yield Static("Log", classes="card-title")
            yield Log(id="merge-log", max_lines=500, classes="panel-log")

    def on_mount(self) -> None:
        self._refresh_defaults()

    def on_show(self) -> None:
        self._refresh_defaults()

    def _refresh_defaults(self) -> None:
        """Keep paths aligned with the default Run → merge → promote click-path.

        Prefer live session outputs when present; otherwise keep the conventional
        publish-set-median layout so the fields are never blank.
        """
        merge_in = self.query_one("#merge-sources", Input)
        if self.pipeline.last_run_paths:
            merge_in.value = " ".join(str(p) for p in self.pipeline.last_run_paths)
        elif not merge_in.value.strip():
            merge_in.value = default_merge_sources()

        promo = self.query_one("#promote-source", Input)
        if self.pipeline.last_merge_path:
            promo.value = str(self.pipeline.last_merge_path)
        elif not promo.value.strip():
            promo.value = default_promote_source()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "merge-go":
            self._do_merge()
        elif event.button.id == "promote-go":
            self._do_promote()

    def _parse_paths(self, raw: str) -> list[Path]:
        parts = raw.replace(",", " ").split()
        return [Path(p) for p in parts if p]

    @work(thread=True)
    def _do_merge(self) -> None:
        log = self.query_one("#merge-log", Log)
        raw = self.query_one("#merge-sources", Input).value
        paths = self._parse_paths(raw)
        out_raw = self.query_one("#merge-output", Input).value.strip()
        output = Path(out_raw) if out_raw else None
        try:
            dest = merge_median(paths, output=output)
        except FacadeError as exc:
            self.app.call_from_thread(log.write_line, f"Merge ERROR: {exc}")
            return
        self.app.call_from_thread(self._on_merge_done, dest)

    def _on_merge_done(self, dest: Path) -> None:
        self.query_one("#merge-log", Log).write_line(f"merged → {dest}")
        self.pipeline.record_merge(dest)
        self.query_one("#promote-source", Input).value = str(dest)
        if hasattr(self.app, "refresh_sync_badges"):
            self.app.refresh_sync_badges()  # type: ignore[attr-defined]

    @work(thread=True)
    def _do_promote(self) -> None:
        log = self.query_one("#merge-log", Log)
        raw = self.query_one("#promote-source", Input).value.strip()
        if not raw and self.pipeline.last_merge_path:
            raw = str(self.pipeline.last_merge_path)
        sources = self._parse_paths(raw)
        name = self.query_one("#promote-name", Input).value.strip() or "thin-spine"
        profile = (
            self.query_one("#promote-profile", Input).value.strip() or "maintainer-ws-1"
        )
        publish_set = self.query_one("#promote-publish-set", Checkbox).value
        thin_spine = self.query_one("#promote-thin-spine", Checkbox).value
        try:
            dest = promote(
                sources,
                name=name,
                profile_id=profile,
                publish_set=publish_set,
                thin_spine=thin_spine,
            )
        except FacadeError as exc:
            self.app.call_from_thread(log.write_line, f"Promote ERROR: {exc}")
            return
        except Exception as exc:
            self.app.call_from_thread(log.write_line, f"Promote ERROR: {exc}")
            return
        self.app.call_from_thread(self._on_promote_done, dest)

    def _on_promote_done(self, dest: Path) -> None:
        self.query_one("#merge-log", Log).write_line(f"promoted → {dest}")
        self.pipeline.record_promote(dest)
        if hasattr(self.app, "refresh_sync_badges"):
            self.app.refresh_sync_badges()  # type: ignore[attr-defined]
