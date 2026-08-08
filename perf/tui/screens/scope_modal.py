"""Modal scope picker — studies / suites / scenarios with group toggles."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.screen import ModalScreen
from textual.widgets import Button, Checkbox, Label, Rule, Static

from perf.tui.scope import (
    ScopeSelection,
    catalog_scenarios,
    catalog_studies,
    catalog_suites,
)


class ScopePickerModal(ModalScreen[ScopeSelection | None]):
    """Pick run scope from catalog checkboxes."""

    BINDINGS = [
        Binding("escape", "cancel", "Cancel", show=True),
    ]

    CSS = """
    ScopePickerModal {
        align: center middle;
    }
    #scope-dialog {
        width: 90;
        height: 85%;
        max-height: 42;
        background: $surface;
        border: thick $accent;
        padding: 1 2;
    }
    #scope-dialog .dialog-title {
        text-style: bold;
        color: $accent;
        margin-bottom: 0;
    }
    #scope-dialog .section-head {
        layout: horizontal;
        height: auto;
        margin-top: 1;
    }
    #scope-dialog .section-label {
        text-style: bold;
        width: 1fr;
        color: $text;
    }
    #scope-dialog .hint {
        color: $text-muted;
        margin-bottom: 1;
    }
    #scope-scroll {
        height: 1fr;
        border: solid $panel;
        padding: 0 1;
        margin: 1 0;
    }
    #scope-dialog Checkbox {
        width: 100%;
    }
    #scope-dialog .indent {
        margin-left: 2;
    }
    #scope-actions {
        height: auto;
        align: right middle;
    }
    """

    def __init__(self, initial: ScopeSelection | None = None) -> None:
        super().__init__()
        self._initial = initial or ScopeSelection()

    def compose(self) -> ComposeResult:
        with Vertical(id="scope-dialog"):
            yield Static("Select run scope", classes="dialog-title")
            yield Static(
                "Check one or more items. Publish set selects all published "
                "study members. Group buttons toggle an entire section.",
                classes="hint",
            )
            with VerticalScroll(id="scope-scroll"):
                yield from self._section(
                    "Presets",
                    "preset",
                    [
                        ("publish_set", "Publish set (union of published studies)"),
                        ("curated", "Curated scenarios only"),
                    ],
                )
                yield Rule()
                study_opts = [
                    (
                        sid,
                        f"{sid}"
                        + ("  · published" if pub else "")
                        + (f"  — {q.splitlines()[0][:60]}" if q else ""),
                    )
                    for sid, pub, q in catalog_studies()
                ]
                yield from self._section("Studies", "study", study_opts)
                yield Rule()
                suite_opts = [(s, s) for s in catalog_suites()]
                yield from self._section("Suites", "suite", suite_opts)
                yield Rule()
                sc_opts = [
                    (
                        sid,
                        f"{sid}  · {suite}" + ("  · curated" if curated else ""),
                    )
                    for sid, suite, curated in catalog_scenarios()
                ]
                yield from self._section("Scenarios", "scenario", sc_opts)
            with Horizontal(id="scope-actions"):
                yield Button("Cancel", id="scope-cancel")
                yield Button("Apply", variant="primary", id="scope-apply")

    def _section(
        self,
        title: str,
        group: str,
        options: list[tuple[str, str]],
    ) -> ComposeResult:
        with Horizontal(classes="section-head"):
            yield Label(title, classes="section-label")
            yield Button("All", flat=True, id=f"all-{group}", classes="tiny")
            yield Button("None", flat=True, id=f"none-{group}", classes="tiny")
        for key, label in options:
            yield Checkbox(
                label,
                id=f"cb-{group}-{key}",
                classes="indent",
                value=self._initial_checked(group, key),
            )

    def _initial_checked(self, group: str, key: str) -> bool:
        sel = self._initial
        if group == "preset":
            if key == "publish_set":
                return sel.publish_set
            if key == "curated":
                return sel.curated_only
        if group == "study":
            return key in sel.study_ids
        if group == "suite":
            return key in sel.suites
        if group == "scenario":
            return key in sel.scenario_ids
        return False

    def on_mount(self) -> None:
        self.query_one("#scope-apply", Button).focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        bid = event.button.id or ""
        if bid == "scope-cancel":
            self.action_cancel()
            return
        if bid == "scope-apply":
            self.dismiss(self._collect())
            return
        if bid.startswith("all-"):
            self._set_group(bid.removeprefix("all-"), True)
        elif bid.startswith("none-"):
            self._set_group(bid.removeprefix("none-"), False)

    def action_cancel(self) -> None:
        self.dismiss(None)

    def _set_group(self, group: str, value: bool) -> None:
        prefix = f"cb-{group}-"
        for cb in self.query(Checkbox):
            if cb.id and cb.id.startswith(prefix):
                cb.value = value

    def _collect(self) -> ScopeSelection:
        publish = False
        curated = False
        studies: list[str] = []
        suites: list[str] = []
        scenarios: list[str] = []
        for cb in self.query(Checkbox):
            if not cb.id or not cb.value:
                continue
            # cb-{group}-{key} — key may contain hyphens
            rest = cb.id.removeprefix("cb-")
            group, _, key = rest.partition("-")
            if group == "preset":
                if key == "publish_set":
                    publish = True
                elif key == "curated":
                    curated = True
            elif group == "study":
                studies.append(key)
            elif group == "suite":
                suites.append(key)
            elif group == "scenario":
                scenarios.append(key)
        return ScopeSelection(
            publish_set=publish,
            curated_only=curated,
            study_ids=studies,
            suites=suites,
            scenario_ids=scenarios,
        )
