# Contributing to the operator documentation

Operator-facing documentation lives in **`operator-docs/`** in the DNS Conduit repository. The published site is built with **MkDocs Material** and deployed to GitHub Pages on release tags.

## When to update docs

Pull requests that change **operator-visible** behavior should update **`operator-docs/`** or the root **`README.md`** — for example:

- Config schema, validation, or defaults
- `conduitctl` or control-plane gRPC behavior
- Metrics, logging, event export, or tracing
- Rhai scripting surfaces, limits, or examples

Purely internal refactors, test-only changes, or dependency bumps with no operator impact do not require doc edits.

## Release notes

Operator-facing changes should add bullet(s) under **`operator-docs/docs/release-notes/unreleased.md`** in the same pull request. At release cut, automation promotes that content to a versioned page (with an ISO **Released** date), updates **`operator-docs/site-root/versions/release-dates.json`**, and resets **Unreleased**.

- Use **`## New features`**, **`## Fixes`**, and **`## Upgrade notes`** sections when helpful.
- Link to canonical docs pages; do not duplicate full explanations.
- Internal-only changes (`#no-docs`) need no unreleased entry; the release gets a short maintenance stub.

## Documentation override

If a PR changes operator-surface code but documentation is genuinely unnecessary, add **`#no-docs`** in the **pull request description** with a one-line reason. CI enforces this policy on protected branches.

## Local build

From the repository root:

```bash
make docs-build
```

Use **`make docs-serve`** for live preview while editing. The build runs **`mkdocs build --strict`** — broken internal links fail the build.

To preview the global **Versions** page (outside MkDocs), run **`make docs-versions-preview`** and open the printed URL. Edit **`operator-docs/site-root/versions/release-dates.json`** and refresh the browser; nothing is published until docs deploy runs on a tag.

## Contributing code

Source code contribution, DCO sign-off, and `make test` expectations are in the repository root [**CONTRIBUTING.md**](https://github.com/egon1024/DNSConduit/blob/main/CONTRIBUTING.md) on GitHub.
