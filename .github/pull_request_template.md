## Summary

<!-- What changed and why (operator impact). -->

## Bump

<!-- #patch (default for fixes), #minor (features), #major (breaking) — release automation reads this. -->

## Documentation

- [ ] Updated **operator-docs/** or root **README.md** for operator-visible changes
- [ ] Or added **`#no-docs`** below with a short reason

<!-- If skipping docs intentionally: -->

## Interop matrix

- [ ] Refreshed **`interop/results/latest.json`** (and regenerated matrix) when interop-relevant paths changed
- [ ] Or added **`#no-interop-matrix`** below with a short reason

<!-- If skipping interop matrix intentionally: -->

## Release notes

- [ ] Added bullet(s) under **`operator-docs/docs/release-notes/unreleased.md`** for operator-visible changes
- [ ] Linked to canonical docs pages (not full duplicate explanations)
- [ ] Noted upgrade or migration steps when relevant
- [ ] Skipped when **`#no-docs`** and no operator impact (release automation writes a maintenance stub)

## Test plan

- [ ] `make test`
- [ ] `make docs-build` (when operator-docs/ or surfaced code changed)
- [ ] Interop matrix refreshed or `#no-interop-matrix` (when interop-relevant paths changed)
