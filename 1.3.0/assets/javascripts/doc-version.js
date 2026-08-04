/* Inject documentation product version from hooks.py (config.version.default in __config). */
(function () {
  function injectDocVersion() {
    const configEl = document.getElementById("__config");
    if (!configEl) return;

    let version = "";
    try {
      version = JSON.parse(configEl.textContent).version?.default || "";
    } catch {
      return;
    }
    if (!version || version === "development") return;

    const topic = document.querySelector(".md-header__topic");
    if (!topic || topic.querySelector(".md-doc-version")) return;

    const span = document.createElement("span");
    span.className = "md-doc-version";
    span.title = "Documentation version";
    span.textContent = version;
    topic.appendChild(span);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", injectDocVersion);
  } else {
    injectDocVersion();
  }
})();
