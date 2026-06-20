/* Collapsible right-hand TOC (opt-in via page front matter `toc_collapsible: true`). */
(function () {
  let scrollListenerBound = false;
  let headingObserver = null;

  function tocRoots() {
    return document.querySelectorAll(".md-nav.md-nav--secondary.md-nav--collapsible");
  }

  function branchItem(el) {
    return el && el.closest("li.md-nav__item--nested");
  }

  function branchToggle(item) {
    return item && item.querySelector(":scope > input.md-nav__toggle");
  }

  function setBranchOpen(item, open, reason) {
    const toggle = branchToggle(item);
    if (!toggle) return;

    toggle.dataset.tocProgrammatic = "true";
    toggle.checked = open;
    queueMicrotask(() => {
      delete toggle.dataset.tocProgrammatic;
    });

    if (!open) {
      delete item.dataset.tocScrollOpen;
      if (reason === "user") delete item.dataset.tocPinned;
      return;
    }

    if (reason === "user") {
      item.dataset.tocPinned = "true";
      delete item.dataset.tocScrollOpen;
    } else if (reason === "scroll") {
      if (item.dataset.tocPinned !== "true") {
        item.dataset.tocScrollOpen = "true";
      }
    } else if (item.dataset.tocPinned !== "true") {
      item.dataset.tocScrollOpen = "true";
    }
  }

  function ancestorBranches(start) {
    const branches = [];
    let item = branchItem(start);
    while (item) {
      branches.push(item);
      item = branchItem(item.parentElement && item.parentElement.closest("li.md-nav__item"));
    }
    return branches;
  }

  function expandAncestors(start, reason) {
    for (const item of ancestorBranches(start)) {
      setBranchOpen(item, true, reason);
    }
  }

  function collapseScrollOpened(root, keepItems) {
    root.querySelectorAll("li.md-nav__item--nested").forEach((item) => {
      if (keepItems.has(item)) return;
      if (item.dataset.tocPinned === "true") return;
      if (item.dataset.tocScrollOpen !== "true") return;
      setBranchOpen(item, false, "scroll");
    });
  }

  function uniquifyToggleIds(root) {
    const prefix = root.dataset.tocInstance || "0";
    root.querySelectorAll("input.md-nav__toggle[id]").forEach((input, index) => {
      const slug = input.id.replace(/^toc-/, "") || String(index);
      const newId = `toc-${prefix}-${slug}`;
      const oldId = input.id;
      if (oldId === newId) return;
      input.id = newId;
      root.querySelectorAll(`label[for="${CSS.escape(oldId)}"]`).forEach((label) => {
        label.setAttribute("for", newId);
      });
    });
  }

  function clearActive(root) {
    root.querySelectorAll("a.md-nav__link--active").forEach((link) => {
      link.classList.remove("md-nav__link--active");
    });
    root.querySelectorAll("li.md-nav__item--active").forEach((item) => {
      item.classList.remove("md-nav__item--active");
    });
  }

  function markActive(link) {
    if (!link) return;
    link.classList.add("md-nav__link--active");
    const item = link.closest("li.md-nav__item");
    if (item) item.classList.add("md-nav__item--active");
  }

  function viewportMarker() {
    return window.innerHeight * 0.28;
  }

  function pageHeadings() {
    const article = document.querySelector(".md-content__inner");
    if (!article) return { h2s: [], all: [] };
    return {
      h2s: [...article.querySelectorAll(":scope > h2[id]")],
      all: [...article.querySelectorAll("h2[id], h3[id]")],
    };
  }

  function headingAtMarker(headings) {
    const marker = viewportMarker();
    let best = null;
    for (const heading of headings) {
      if (heading.getBoundingClientRect().top <= marker) best = heading;
    }
    return best;
  }

  function sectionHeadingFor(heading) {
    if (!heading) return null;
    const { h2s } = pageHeadings();
    if (!h2s.length) return null;
    if (heading.tagName === "H2") return heading;

    let sectionH2 = h2s[0];
    for (const h2 of h2s) {
      if (h2.compareDocumentPosition(heading) & Node.DOCUMENT_POSITION_FOLLOWING) {
        sectionH2 = h2;
      }
    }
    return sectionH2;
  }

  function linkHash(link) {
    const href = link && link.getAttribute("href");
    if (!href) return "";
    if (href.startsWith("#")) return href;
    try {
      return new URL(href, location.href).hash;
    } catch {
      const hashIndex = href.indexOf("#");
      return hashIndex >= 0 ? href.slice(hashIndex) : "";
    }
  }

  function tocLinkForHeading(root, heading) {
    if (!heading || !heading.id) return null;
    const targetHash = `#${heading.id}`;
    for (const link of root.querySelectorAll("a.md-nav__link")) {
      if (linkHash(link) === targetHash) return link;
    }
    return null;
  }

  function tocLinkForHash(root, hash) {
    if (!hash) return null;
    for (const link of root.querySelectorAll("a.md-nav__link")) {
      if (linkHash(link) === hash) return link;
    }
    return null;
  }

  function updateScrollState(root) {
    const { all } = pageHeadings();
    if (!all.length) return;

    const highlightHeading = headingAtMarker(all) || all[0];
    const highlightLink = tocLinkForHeading(root, highlightHeading);
    const sectionHeading = sectionHeadingFor(highlightHeading);
    const sectionLink = tocLinkForHeading(root, sectionHeading);

    if (sectionLink) {
      const keepOpen = new Set(ancestorBranches(sectionLink));
      collapseScrollOpened(root, keepOpen);
      expandAncestors(sectionLink, "scroll");
    }

    clearActive(root);
    if (highlightLink) markActive(highlightLink);
  }

  function applyHash(root, hash) {
    if (!hash) return;
    const link = tocLinkForHash(root, hash);
    if (!link) return;

    const keepOpen = new Set(ancestorBranches(link));
    collapseScrollOpened(root, keepOpen);
    expandAncestors(link, "user");
    clearActive(root);
    markActive(link);
  }

  function openReferenceForHash(hash) {
    if (!hash || hash.length < 2) return;
    const id = decodeURIComponent(hash.slice(1));
    const target =
      document.getElementById(id) ||
      document.querySelector(`[id="${CSS.escape(id)}"]`);
    if (!target) return;
    if (window.__txnApiCards) {
      window.__txnApiCards.openEntryForTarget(target);
    }
  }

  function scheduleScrollSync() {
    window.requestAnimationFrame(() => {
      tocRoots().forEach((root) => {
        if (location.hash) {
          applyHash(root, location.hash);
        }
        updateScrollState(root);
      });
    });
  }

  function bindScrollListener() {
    if (scrollListenerBound) return;
    scrollListenerBound = true;

    let ticking = false;
    const onScroll = () => {
      if (ticking) return;
      ticking = true;
      window.requestAnimationFrame(() => {
        tocRoots().forEach((root) => updateScrollState(root));
        ticking = false;
      });
    };

    window.addEventListener("scroll", onScroll, { passive: true });
    document.addEventListener("scroll", onScroll, { passive: true, capture: true });
    window.addEventListener("resize", onScroll, { passive: true });
  }

  function bindHeadingObserver() {
    if (headingObserver) {
      headingObserver.disconnect();
      headingObserver = null;
    }

    const { all } = pageHeadings();
    if (!all.length || typeof IntersectionObserver === "undefined") return;

    headingObserver = new IntersectionObserver(
      () => {
        tocRoots().forEach((root) => updateScrollState(root));
      },
      {
        root: null,
        rootMargin: "-20% 0px -55% 0px",
        threshold: [0, 1],
      }
    );

    all.forEach((heading) => headingObserver.observe(heading));
  }

  function initRoot(root, index) {
    if (root.dataset.tocCollapseInit === "true") return;
    root.dataset.tocCollapseInit = "true";
    root.dataset.tocInstance = String(index);

    uniquifyToggleIds(root);

    root.addEventListener("change", (event) => {
      const toggle = event.target;
      if (!toggle.matches("input.md-nav__toggle") || !root.contains(toggle)) return;

      const item = toggle.closest("li.md-nav__item--nested");
      if (!item) return;

      if (toggle.dataset.tocProgrammatic === "true") {
        delete toggle.dataset.tocProgrammatic;
        if (!toggle.checked) {
          delete item.dataset.tocScrollOpen;
        }
        return;
      }

      if (toggle.checked) {
        item.dataset.tocPinned = "true";
        delete item.dataset.tocScrollOpen;
      } else {
        delete item.dataset.tocPinned;
        delete item.dataset.tocScrollOpen;
      }
    });

    root.addEventListener("click", (event) => {
      const chevron = event.target.closest("label.md-toc__branch-toggle");
      if (chevron && root.contains(chevron)) {
        const item = chevron.closest("li.md-nav__item--nested");
        const toggle = branchToggle(item);
        queueMicrotask(() => {
          if (!toggle || !item) return;
          delete toggle.dataset.tocProgrammatic;
          if (toggle.checked) {
            item.dataset.tocPinned = "true";
            delete item.dataset.tocScrollOpen;
          } else {
            delete item.dataset.tocPinned;
            delete item.dataset.tocScrollOpen;
          }
        });
        return;
      }

      const branchLink = event.target.closest("a.md-toc__branch-link");
      if (branchLink && root.contains(branchLink)) {
        expandAncestors(branchLink, "user");
        return;
      }

      const leafLink = event.target.closest("a.md-nav__link:not(.md-toc__branch-link)");
      if (leafLink && root.contains(leafLink)) {
        expandAncestors(leafLink, "user");
      }
    });
  }

  function syncAll() {
    openReferenceForHash(location.hash);
    scheduleScrollSync();
  }

  function init() {
    bindScrollListener();
    bindHeadingObserver();
    tocRoots().forEach((root, index) => initRoot(root, index));
    syncAll();
  }

  function resetAndInit() {
    if (headingObserver) {
      headingObserver.disconnect();
      headingObserver = null;
    }
    tocRoots().forEach((root) => {
      delete root.dataset.tocCollapseInit;
      root.querySelectorAll("li.md-nav__item--nested").forEach((item) => {
        delete item.dataset.tocPinned;
        delete item.dataset.tocScrollOpen;
      });
    });
    init();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }

  if (typeof document$ !== "undefined" && document$.subscribe) {
    document$.subscribe(resetAndInit);
  }

  window.addEventListener("hashchange", syncAll);
})();
