"""Field help copy and a small click-to-open help modal."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.screen import ModalScreen
from textual.widgets import Button, Checkbox, Label, Static

# Short help copy (used when a field has no longer FIELD_DETAILS entry).
FIELD_BLURBS: dict[str, str] = {
    "run-conduit": "Path to the Conduit binary under test (not built by the harness).",
    "run-profile": (
        "Label stored on the run JSON for which lab host produced these numbers. "
        "Use local for smoke; maintainer-ws-1 for publish-quality reference runs."
    ),
    "run-cycles": "How many full passes to run. Publish-quality medians usually use 3.",
    "run-time": (
        "dnsperf -l seconds per scenario. 5 is a smoke default; leave empty for the "
        "harness/scenario default duration used in published results."
    ),
    "run-output": "Where to write run JSON. With cycles>1, sibling -rN files are used.",
    "run-kill-strays": "SIGKILL orphans left by a crashed prior runner (ledger-tracked only).",
    "run-allow-cpu": "Skip the CPU governor=performance preflight (results may be noisy).",
    "run-allow-udp": "Skip the UDP rmem_max preflight (may see Queries lost).",
    "merge-sources": "Two or more same-shape round JSON files to median-merge.",
    "merge-output": "Destination for the merged median document.",
    "promote-source": "Run JSON to bless into results/references/ (often the median output).",
    "promote-name": "Basename under perf/results/references/ (default thin-spine).",
    "promote-profile": (
        "Lab profile id written onto the promoted reference (usually maintainer-ws-1)."
    ),
    "promote-publish-set": "Keep only members of studies marked published: true.",
    "promote-thin-spine": "Legacy filter: keep only the thin curated spine scenario ids.",
    "gen-from": "Reference JSON to render from; empty uses latest-reference.json.",
    "gen-render-from": "Any run or reference JSON to render to a file.",
    "gen-format": "plain / rich / yaml / json / html — same formats as perf.runner render.",
    "gen-render-out": "Filesystem path for the rendered output.",
}

# Longer body for the "?" modal (falls back to the blurb).
FIELD_DETAILS: dict[str, str] = {
    "run-profile": (
        "Each run document records lab_profile.id so later readers know which "
        "machine shape produced the QPS numbers.\n\n"
        "• local — ad-hoc / smoke runs on whatever host you are on.\n"
        "• maintainer-ws-1 — the named reference workstation profile used for "
        "published operator-docs numbers (see perf/catalog/lab_profiles/).\n\n"
        "Promoting a run also retargets this id (Merge & Promote defaults to "
        "maintainer-ws-1). It does not change how the load is generated; it is "
        "provenance metadata."
    ),
    "run-cycles": (
        "One cycle = one full pass over the selected scenarios, writing one run "
        "JSON. For publish-set refresh, run N cycles then median-merge the round "
        "files (methodology default N=3) before promote."
    ),
    "run-time": (
        "Overrides each scenario's configured duration_s for this invocation. "
        "Short values (e.g. 5) are for wiring checks only — absolute QPS will not "
        "match published figures. Omit the override for publish-quality timing."
    ),
    "promote-publish-set": (
        "When enabled, promote keeps the union of scenario ids from studies with "
        "published: true in the catalog. Prefer this over the legacy thin-spine "
        "filter for curated reference refreshes."
    ),
}


class HelpModal(ModalScreen[None]):
    """Simple title + body help dialog."""

    BINDINGS = [Binding("escape", "dismiss_help", "Close", show=True)]

    CSS = """
    HelpModal {
        align: center middle;
    }
    #help-dialog {
        width: 72;
        max-width: 90%;
        height: auto;
        max-height: 80%;
        background: $surface;
        border: thick $accent;
        padding: 1 2;
    }
    #help-dialog .help-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 1;
    }
    #help-body {
        height: auto;
        max-height: 24;
        margin-bottom: 1;
    }
    #help-actions {
        height: auto;
        align: right middle;
    }
    """

    def __init__(self, title: str, body: str) -> None:
        super().__init__()
        self._title = title
        self._body = body

    def compose(self) -> ComposeResult:
        with Vertical(id="help-dialog"):
            yield Static(self._title, classes="help-title")
            with VerticalScroll(id="help-body"):
                yield Static(self._body)
            with Horizontal(id="help-actions"):
                yield Button("Close", variant="primary", id="help-close")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "help-close":
            self.dismiss(None)

    def action_dismiss_help(self) -> None:
        self.dismiss(None)


def field_detail(field_id: str) -> str:
    return FIELD_DETAILS.get(field_id) or FIELD_BLURBS.get(field_id, "")


class FlagWithHelp(Horizontal):
    """Checkbox with a trailing ? help button."""

    DEFAULT_CSS = """
    FlagWithHelp {
        height: auto;
        width: auto;
        margin-right: 2;
    }
    FlagWithHelp Checkbox {
        width: auto;
    }
    FlagWithHelp .help-btn {
        width: 5;
        min-width: 5;
        margin: 0;
    }
    """

    def __init__(
        self,
        label: str,
        *,
        help_id: str,
        checkbox_id: str,
        value: bool = False,
    ) -> None:
        super().__init__()
        self._label = label
        self._help_id = help_id
        self._checkbox_id = checkbox_id
        self._value = value

    def compose(self) -> ComposeResult:
        yield Checkbox(self._label, value=self._value, id=self._checkbox_id)
        yield Button("?", flat=True, classes="help-btn", id=f"help-{self._help_id}")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id != f"help-{self._help_id}":
            return
        event.stop()
        self.app.push_screen(HelpModal(self._label, field_detail(self._help_id)))


class LabeledField(Vertical):
    """Label + optional ? help button + blurb, for consistent form rows."""

    DEFAULT_CSS = """
    LabeledField {
        height: auto;
        margin-bottom: 0;
    }
    LabeledField .label-row {
        height: auto;
    }
    LabeledField .field-label {
        width: 1fr;
        color: #a0aab4;
    }
    LabeledField .help-btn {
        width: 5;
        min-width: 5;
        margin: 0 0 0 1;
    }
    """

    def __init__(
        self,
        label: str,
        *,
        help_id: str,
        classes: str | None = None,
    ) -> None:
        super().__init__(classes=classes)
        self._label = label
        self._help_id = help_id

    def compose(self) -> ComposeResult:
        with Horizontal(classes="label-row"):
            yield Label(self._label, classes="field-label")
            yield Button("?", flat=True, classes="help-btn", id=f"help-{self._help_id}")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id != f"help-{self._help_id}":
            return
        event.stop()
        title = self._label.rstrip(":")
        body = field_detail(self._help_id)
        self.app.push_screen(HelpModal(title, body))
