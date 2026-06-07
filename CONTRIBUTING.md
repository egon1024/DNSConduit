# Contributing to DNS Conduit

Thank you for your interest in contributing.

## License

By contributing to this repository, you agree that your contributions are
licensed under the [Apache License, Version 2.0](LICENSE), the same license
that covers the project.

## Developer Certificate of Origin (DCO)

We require a [Developer Certificate of Origin](https://developercertificate.org/)
(DCO) sign-off on every commit.

Each commit message must include a line in this form:

```
Signed-off-by: Your Name <your.email@example.com>
```

The sign-off certifies that you wrote the contribution or have the right to
pass it on as an open-source contribution under the project's license.

### Adding a sign-off

With Git 2.14 or later, use `-s` when committing:

```bash
git commit -s -m "your message"
```

To add a sign-off to the most recent commit:

```bash
git commit -s --amend
```

Use `--amend` only on commits you have not pushed, or when explicitly
rebasing a pull request before merge.

## Pull requests

1. Open an issue or comment on an existing one if you are unsure about scope.
2. Fork the repository and work on a branch.
3. Run `make test` before opening the pull request.
4. Ensure every commit in the PR has a valid DCO sign-off.
5. Describe what changed and how you tested it.

Pull requests without DCO sign-offs on all commits will not be merged.

## What is covered by the project license

The Apache 2.0 license applies to DNS Conduit source code in this repository
(daemon, libraries, CLI, and project documentation).

Operator configuration you write for your own deployment (YAML, Rhai scripts,
and future WASM plugins or sidecars) is your own work and is not automatically
licensed under Apache 2.0 unless it includes or is derived from project
source code.
