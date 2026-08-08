"""Generate stage — integrity, docs, alternate render formats."""

from __future__ import annotations

from pathlib import Path

from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, Input, Log, Select, Static

from perf.runner.api import (
    FORMATS,
    FacadeError,
    TakeawayIntegrityError,
    check_takeaway_integrity,
    generate_docs,
    render_run,
    resolve_latest_reference_path,
)
from perf.tui.help import LabeledField
from perf.tui.pipeline_state import PipelineState
from perf.tui.scope import default_render_output


class GenerateScreen(Vertical):
    def __init__(self, pipeline: PipelineState, **kwargs) -> None:
        super().__init__(**kwargs)
        self.pipeline = pipeline

    def compose(self) -> ComposeResult:
        yield Static("Generate", classes="stage-title")
        yield Static(
            "Regenerate operator-docs fragments, check takeaway integrity, "
            "or export an alternate render.",
            classes="stage-lead",
        )
        with VerticalScroll(classes="stage-body"):
            with Vertical(classes="card"):
                yield Static("Operator docs", classes="card-title")
                yield LabeledField("Reference JSON", help_id="gen-from")
                yield Input(
                    id="gen-from",
                    placeholder="perf/results/references/thin-spine.json",
                )
                with Horizontal(classes="btn-row"):
                    yield Button("Check integrity", id="gen-integrity")
                    yield Button("Generate docs", variant="success", id="gen-docs")

            with Vertical(classes="card"):
                yield Static("Render / save", classes="card-title")
                yield LabeledField("Source run or reference JSON", help_id="gen-render-from")
                yield Input(id="gen-render-from")
                with Horizontal(classes="field-row"):
                    with Vertical(classes="field"):
                        yield LabeledField("Format", help_id="gen-format")
                        yield Select(
                            ((f, f) for f in sorted(FORMATS)),
                            value="html",
                            id="gen-format",
                        )
                    with Vertical(classes="field"):
                        yield LabeledField("Output file", help_id="gen-render-out")
                        yield Input(
                            value=default_render_output(),
                            id="gen-render-out",
                        )
                yield Button("Render to file", variant="primary", id="gen-render")

            yield Static("Log", classes="card-title")
            yield Log(id="gen-log", max_lines=500, classes="panel-log")

    def on_mount(self) -> None:
        latest = resolve_latest_reference_path()
        if latest is not None:
            self.query_one("#gen-from", Input).value = str(latest)
            self.query_one("#gen-render-from", Input).value = str(latest)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "gen-integrity":
            self._check_integrity()
        elif event.button.id == "gen-docs":
            self._generate_docs()
        elif event.button.id == "gen-render":
            self._render_file()

    def _from_path(self) -> Path | None:
        raw = self.query_one("#gen-from", Input).value.strip()
        return Path(raw) if raw else None

    @work(thread=True)
    def _check_integrity(self) -> None:
        log = self.query_one("#gen-log", Log)
        try:
            check_takeaway_integrity(self._from_path())
        except TakeawayIntegrityError as exc:
            self.app.call_from_thread(log.write_line, f"Integrity FAIL:\n{exc}")
            return
        except FacadeError as exc:
            self.app.call_from_thread(log.write_line, f"Integrity ERROR: {exc}")
            return
        self.app.call_from_thread(
            log.write_line, "Integrity OK — takeaways match evidence."
        )

    @work(thread=True)
    def _generate_docs(self) -> None:
        log = self.query_one("#gen-log", Log)
        try:
            written = generate_docs(self._from_path())
        except TakeawayIntegrityError as exc:
            self.app.call_from_thread(
                log.write_line, f"Generate FAIL (integrity):\n{exc}"
            )
            return
        except FacadeError as exc:
            self.app.call_from_thread(log.write_line, f"Generate ERROR: {exc}")
            return
        self.app.call_from_thread(self._on_docs_done, written)

    def _on_docs_done(self, written: list[Path]) -> None:
        log = self.query_one("#gen-log", Log)
        log.write_line(f"Generated {len(written)} path(s):")
        for p in written[:40]:
            log.write_line(f"  {p}")
        if len(written) > 40:
            log.write_line(f"  … and {len(written) - 40} more")
        self.pipeline.record_generate()
        if hasattr(self.app, "refresh_sync_badges"):
            self.app.refresh_sync_badges()  # type: ignore[attr-defined]

    @work(thread=True)
    def _render_file(self) -> None:
        log = self.query_one("#gen-log", Log)
        src = self.query_one("#gen-render-from", Input).value.strip()
        out = self.query_one("#gen-render-out", Input).value.strip()
        fmt = self.query_one("#gen-format", Select).value
        if not src or not out:
            self.app.call_from_thread(
                log.write_line, "Render needs source JSON and output path."
            )
            return
        if not isinstance(fmt, str):
            fmt = "html"
        try:
            render_run(Path(src), fmt, output=Path(out))
        except FacadeError as exc:
            self.app.call_from_thread(log.write_line, f"Render ERROR: {exc}")
            return
        except Exception as exc:
            self.app.call_from_thread(log.write_line, f"Render ERROR: {exc}")
            return
        self.app.call_from_thread(log.write_line, f"Wrote {out} ({fmt})")
