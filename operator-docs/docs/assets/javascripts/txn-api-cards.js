/* Transaction API cards — reference toggles on the title row and per-section expand/collapse. */
(function () {
  function referencePanel(entry) {
    return entry.querySelector(":scope > .txn-api-reference-panel");
  }

  function entryHeading(entry) {
    return (
      entry.querySelector(":scope > .txn-api-entry-header > h3") ||
      entry.querySelector(":scope > h3")
    );
  }

  function makeChevron(className) {
    const chevron = document.createElement("span");
    chevron.className = className;
    chevron.setAttribute("aria-hidden", "true");
    return chevron;
  }

  function isEntryOpen(entry) {
    const panel = referencePanel(entry);
    return panel ? !panel.hidden : false;
  }

  function setEntryOpen(entry, open) {
    const panel = referencePanel(entry);
    const button = entry.querySelector(".txn-api-reference-toggle");
    if (!panel) return;

    panel.hidden = !open;
    entry.classList.toggle("txn-api-entry--open", open);

    if (button) {
      button.setAttribute("aria-label", open ? "Hide reference" : "Show reference");
      button.setAttribute("aria-expanded", open ? "true" : "false");
      button.setAttribute("title", open ? "Hide reference" : "Show reference");
    }

    entry.dispatchEvent(new CustomEvent("txn-api-toggle", { bubbles: false }));
  }

  function ensureReferencePanel(entry) {
    let panel = referencePanel(entry);
    const details = entry.querySelector(":scope > details");
    if (panel || !details) return panel;

    panel = document.createElement("div");
    panel.className = "txn-api-reference-panel";
    panel.hidden = true;
    while (details.firstChild) {
      panel.appendChild(details.firstChild);
    }
    panel.querySelector(":scope > summary")?.remove();
    details.replaceWith(panel);
    return panel;
  }

  function initReferenceToggle(entry) {
    const h3 = entryHeading(entry);
    if (!h3 || entry.dataset.txnApiInit === "true") return;

    const panel = ensureReferencePanel(entry);
    if (!panel) return;

    entry.dataset.txnApiInit = "true";
    panel.hidden = !isEntryOpen(entry);

    let header = entry.querySelector(":scope > .txn-api-entry-header");
    if (!header) {
      header = document.createElement("div");
      header.className = "txn-api-entry-header";
      entry.insertBefore(header, h3);
      header.appendChild(h3);

      const button = document.createElement("button");
      button.type = "button";
      button.className = "txn-api-reference-toggle";
      button.appendChild(makeChevron("txn-api-reference-chevron"));
      button.setAttribute("aria-label", "Show reference");
      button.setAttribute("aria-expanded", "false");
      button.setAttribute("title", "Show reference");
      header.appendChild(button);

      button.addEventListener("click", (event) => {
        event.preventDefault();
        setEntryOpen(entry, !isEntryOpen(entry));
      });
    }

    entry.classList.add("txn-api-entry--ready");
  }

  function sectionEntries(h2) {
    const entries = [];
    let el = h2.nextElementSibling;
    while (el && el.tagName !== "H2") {
      if (el.classList.contains("txn-api-entry")) entries.push(el);
      el = el.nextElementSibling;
    }
    return entries;
  }

  function updateSectionButton(button, entries) {
    const openCount = entries.filter(isEntryOpen).length;
    const allOpen = openCount === entries.length;

    button.dataset.state = allOpen ? "open" : "closed";
    button.setAttribute("aria-expanded", allOpen ? "true" : "false");
    button.setAttribute(
      "aria-label",
      allOpen ? "Collapse all method references in this section" : "Expand all method references in this section"
    );
    button.setAttribute("title", allOpen ? "Collapse all" : "Expand all");
    button.classList.toggle("txn-api-section-toggle--open", allOpen);
  }

  function initSectionToggle(h2) {
    if (h2.dataset.txnSectionInit === "true") return;

    const entries = sectionEntries(h2);
    if (!entries.length) return;
    h2.dataset.txnSectionInit = "true";

    const insertBefore = entries[0];
    const bar = document.createElement("div");
    bar.className = "txn-api-section-bar";

    const button = document.createElement("button");
    button.type = "button";
    button.className = "txn-api-section-toggle";
    button.appendChild(makeChevron("txn-api-section-chevron"));

    updateSectionButton(button, entries);

    button.addEventListener("click", () => {
      const expand = entries.filter(isEntryOpen).length < entries.length;
      for (const entry of entries) {
        setEntryOpen(entry, expand);
      }
      updateSectionButton(button, entries);
    });

    for (const entry of entries) {
      entry.addEventListener("txn-api-toggle", () => {
        updateSectionButton(button, entries);
      });
    }

    bar.appendChild(button);
    insertBefore.parentElement.insertBefore(bar, insertBefore);
  }

  function init() {
    document.querySelectorAll(".txn-api-entry").forEach(initReferenceToggle);
    document.querySelectorAll(".md-content__inner > h2").forEach(initSectionToggle);
  }

  function resetAndInit() {
    document.querySelectorAll(".txn-api-entry-header").forEach((header) => {
      const h3 = header.querySelector("h3");
      const entry = header.parentElement;
      if (h3 && entry) entry.insertBefore(h3, header);
      header.remove();
    });
    document.querySelectorAll(".txn-api-reference-panel").forEach((panel) => {
      const entry = panel.closest(".txn-api-entry");
      if (!entry) return;
      const details = document.createElement("details");
      const summary = document.createElement("summary");
      summary.textContent = "Reference";
      details.appendChild(summary);
      while (panel.firstChild) details.appendChild(panel.firstChild);
      panel.replaceWith(details);
    });
    document.querySelectorAll("[data-txn-api-init]").forEach((el) => {
      delete el.dataset.txnApiInit;
    });
    document.querySelectorAll("[data-txn-section-init]").forEach((el) => {
      delete el.dataset.txnSectionInit;
    });
    document.querySelectorAll(".txn-api-section-bar").forEach((bar) => bar.remove());
    document.querySelectorAll(".txn-api-entry").forEach((entry) => {
      entry.classList.remove("txn-api-entry--open", "txn-api-entry--ready");
    });
    init();
  }

  window.__txnApiCards = {
    setEntryOpen,
    isEntryOpen,
    openEntryForTarget(target) {
      const entry = target && target.closest(".txn-api-entry");
      if (!entry) return;
      initReferenceToggle(entry);
      setEntryOpen(entry, true);
    },
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }

  if (typeof document$ !== "undefined" && document$.subscribe) {
    document$.subscribe(resetAndInit);
  }
})();
