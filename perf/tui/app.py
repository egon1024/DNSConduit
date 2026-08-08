"""Perf lifecycle Textual application (side-rail shell)."""

from __future__ import annotations

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.widgets import Footer, Header, Label, ListItem, ListView, Static

from perf.tui.pipeline_state import PipelineState, sync_label
from perf.tui.screens.generate import GenerateScreen
from perf.tui.screens.merge_promote import MergePromoteScreen
from perf.tui.screens.run import RunScreen

STAGES = (
    ("run", "Run"),
    ("merge_promote", "Merge & Promote"),
    ("generate", "Generate"),
)


class StageRailItem(ListItem):
    def __init__(self, stage_id: str, title: str) -> None:
        super().__init__()
        self.stage_id = stage_id
        self._title = title

    def compose(self) -> ComposeResult:
        yield Label(self._title, classes="rail-title")
        yield Label("· unknown", classes="rail-badge", id=f"badge-{self.stage_id}")


class PerfLifecycleApp(App[None]):
    """Side-rail workflow shell; stage screens are swappable without chrome rewrite."""

    TITLE = "DNSConduit perf lifecycle"
    CSS = """
    Screen {
        background: #1b1f24;
    }
    Header {
        background: #12151a;
        color: #e8eef2;
        text-style: bold;
    }
    Footer {
        background: #12151a;
    }
    #rail {
        width: 30;
        background: #14181e;
        border-right: tall #2f9e8f;
        padding: 1 0;
    }
    #rail .rail-heading {
        padding: 0 2 1 2;
        color: #2f9e8f;
        text-style: bold;
    }
    #rail ListView {
        height: 1fr;
        background: transparent;
    }
    #rail ListItem {
        padding: 1 1;
        margin: 0 1;
        background: transparent;
    }
    #rail ListItem.-highlight {
        background: #243038;
        border-left: thick #2f9e8f;
    }
    .rail-title {
        padding: 0 1;
        color: #e8eef2;
        text-style: bold;
    }
    .rail-badge {
        padding: 0 1;
        color: #7a8490;
    }
    .rail-badge.in_sync {
        color: #5dca8a;
    }
    .rail-badge.stale {
        color: #e0a85c;
    }
    .rail-badge.unknown {
        color: #7a8490;
    }
    #session-hint {
        height: auto;
        min-height: 3;
        color: #8b949e;
        padding: 1 2;
        border-top: solid #2a3139;
    }
    #content {
        width: 1fr;
        padding: 1 2;
        background: #1b1f24;
    }
    .stage-title {
        text-style: bold;
        color: #e8eef2;
        text-align: left;
        margin-bottom: 0;
    }
    .stage-lead {
        color: #8b949e;
        margin-bottom: 1;
    }
    .stage-body {
        height: 1fr;
    }
    .card {
        background: #222830;
        border: solid #2a3139;
        padding: 1 2;
        margin-bottom: 1;
        height: auto;
    }
    .card-title {
        text-style: bold;
        color: #2f9e8f;
        margin-bottom: 1;
    }
    .scope-summary {
        color: #c8d0d8;
        padding: 1;
        background: #1b1f24;
        border: solid #2f9e8f;
        margin-bottom: 1;
        height: auto;
    }
    .status-card {
        margin-bottom: 1;
        border: tall #2a3139;
    }
    .status-card-idle {
        border: tall #2a3139;
    }
    .status-card-running {
        border: tall #e0a85c;
        background: #2a261c;
    }
    .status-card-complete {
        border: tall #5dca8a;
        background: #1c2a22;
    }
    .status-card-failed {
        border: tall #e06c75;
        background: #2a1c1e;
    }
    .status-card-cancelled {
        border: tall #7a8490;
    }
    .status-headline {
        text-style: bold;
        margin-bottom: 1;
    }
    .status-idle {
        color: #8b949e;
    }
    .status-running {
        color: #e0a85c;
    }
    .status-complete {
        color: #5dca8a;
    }
    .status-failed {
        color: #e06c75;
    }
    .status-cancelled {
        color: #a0aab4;
    }
    .status-times {
        color: #c8d0d8;
        margin-top: 1;
    }
    .status-detail {
        color: #8b949e;
        margin-top: 0;
    }
    #run-progress {
        margin: 0 0 0 0;
        width: 100%;
    }
    .btn-row {
        height: auto;
        margin-top: 1;
    }
    .btn-row Button {
        margin-right: 1;
    }
    .field-row {
        height: auto;
    }
    .field {
        width: 1fr;
        margin-right: 1;
        height: auto;
    }
    .flag-row {
        height: auto;
        margin-top: 1;
    }
    .flag-row Checkbox {
        margin-right: 2;
        width: auto;
    }
    Input {
        margin-bottom: 1;
    }
    Label {
        color: #a0aab4;
        margin-bottom: 0;
    }
    Log.run-log, Log.panel-log {
        height: 16;
        min-height: 10;
        border: solid #2a3139;
        background: #12151a;
    }
    Button {
        margin-right: 1;
    }
    """

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("r", "refresh_sync", "Refresh sync"),
    ]

    def __init__(self) -> None:
        super().__init__()
        self.pipeline = PipelineState()
        self._active = "run"

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Horizontal():
            with Vertical(id="rail"):
                yield Label("WORKFLOW", classes="rail-heading")
                yield ListView(
                    *[StageRailItem(sid, title) for sid, title in STAGES],
                    id="stage-list",
                )
                yield Static("", id="session-hint")
            with Vertical(id="content"):
                yield RunScreen(self.pipeline, id="stage-run")
                yield MergePromoteScreen(self.pipeline, id="stage-merge_promote")
                yield GenerateScreen(self.pipeline, id="stage-generate")
        yield Footer()

    def on_mount(self) -> None:
        for sid, _title in STAGES:
            widget = self.query_one(f"#stage-{sid}")
            widget.display = sid == self._active
        self.query_one("#stage-list", ListView).index = 0
        self.refresh_sync_badges()

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item = event.item
        if not isinstance(item, StageRailItem):
            return
        self._show_stage(item.stage_id)

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        item = event.item
        if item is None or not isinstance(item, StageRailItem):
            return
        self._show_stage(item.stage_id)

    def _show_stage(self, stage_id: str) -> None:
        self._active = stage_id
        for sid, _title in STAGES:
            self.query_one(f"#stage-{sid}").display = sid == stage_id
        self.refresh_sync_badges()

    def action_refresh_sync(self) -> None:
        self.refresh_sync_badges()
        self.notify("Sync badges refreshed")

    def refresh_sync_badges(self) -> None:
        for sid, _title in STAGES:
            status = self.pipeline.badge(sid)
            badge = self.query_one(f"#badge-{sid}", Label)
            badge.update(f"· {sync_label(status)}")
            badge.set_classes(f"rail-badge {status}")
        hint = self.pipeline.session_hint.get(self._active, "")
        if self._active == "merge_promote":
            hint = self.pipeline.session_hint.get(
                "promote"
            ) or self.pipeline.session_hint.get("merge", hint)
        self.query_one("#session-hint", Static).update(
            hint or "Session hints appear here after actions."
        )


def main() -> int:
    try:
        import textual  # noqa: F401
    except ImportError:
        print(
            "Textual is required for the perf TUI.\n"
            "  pip install -r perf/requirements-tui.txt",
            flush=True,
        )
        return 1
    PerfLifecycleApp().run()
    return 0
