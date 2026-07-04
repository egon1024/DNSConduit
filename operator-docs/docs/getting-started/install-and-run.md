# Install and run

This page covers installing Conduit from [GitHub release](https://github.com/egon1024/DNSConduit/releases) assets on **Ubuntu 22.04 or 24.04** (amd64). For the smallest config after install, see [Minimal configuration](/getting-started/minimal-configuration.md).

## Release artifacts

Each stable release publishes:

| Asset | Purpose |
|-------|---------|
| `conduit-<version>-amd64.tar.gz` | Production: **stripped** binaries |
| `conduit-<version>-amd64-debug.tar.gz` | Debug: **unstripped** binaries |
| `conduit_<version>_amd64.deb` | Production Debian package (stripped) |
| `conduit-dbg_<version>_amd64.deb` | Debug Debian package (unstripped) |
| `SHA256SUMS` | Checksums for the files above |
| `conduit-<version>.spdx.json` | Software bill of materials (SBOM) |

Every tarball and package includes three binaries:

- **`conduit`** — DNS [dataplane](/glossary/index.md#dataplane) (the service)
- **`conduitctl`** — [control plane](/glossary/index.md#control-plane) CLI (`validate` offline; `apply`, `export`, `reload`, `trace`, and `health` when control is enabled)
- **`conduit-dnstap-tracer`** — development/troubleshooting dnstap listener (decodes export to stdout). **Not** part of the production systemd service; use only for debugging dnstap sinks.

Production vs debug differs only by **stripped vs unstripped** binaries. Use production artifacts on servers; use debug artifacts when you need symbols for `gdb` or postmortem analysis.

Verify downloads:

```bash
sha256sum -c SHA256SUMS
```

If a rebuild is published, delete old assets from the release page before maintainers re-run the artifact workflow (uploads fail when assets already exist).

## Install from tarball

```bash
VERSION=0.13.0   # replace with the release you downloaded
tar xzf "conduit-${VERSION}-amd64.tar.gz"
cd "conduit-${VERSION}"
ls -l
```

The directory contains `conduit`, `conduitctl`, `conduit-dnstap-tracer`, `LICENSE`, `conduit.minimal.yaml`, and `conduit.reference.yaml`.

Run with a config file (first argument is the YAML path only):

```bash
./conduit conduit.minimal.yaml
```

Edit the pool backend address in the minimal file before expecting successful forwarding. For a full field reference, see `conduit.reference.yaml` and [Reference: config schema](/reference/config-schema/index.md).

## Install from .deb (production)

```bash
VERSION=0.13.0
sudo dpkg -i "conduit_${VERSION}_amd64.deb"
# if dependencies are missing:
sudo apt-get -f install -y
```

The package:

- Installs binaries to `/usr/bin/`
- Creates system user and group **`conduit`**
- Installs **`/etc/conduit/conduit.yaml`** (conffile — preserved on upgrade)
- Installs examples under **`/usr/share/doc/conduit/examples/`**
- **Enables** `conduit.service` but does **not** start it

Edit the config, then start the service:

```bash
sudo editor /etc/conduit/conduit.yaml
sudo systemctl start conduit
sudo systemctl status conduit
```

The unit runs `/usr/bin/conduit /etc/conduit/conduit.yaml` as user `conduit` with **`CAP_NET_BIND_SERVICE`** so non-root processes can bind to privileged ports (for example port 53).

## Debug .deb (optional)

For unstripped binaries on a troubleshooting host:

```bash
sudo dpkg -i "conduit-dbg_${VERSION}_amd64.deb"
```

This overwrites `/usr/bin/conduit`, `conduitctl`, and `conduit-dnstap-tracer` with unstripped builds. Install the production package first on servers that use systemd; use `-dbg` only when you need debug symbols.

## Build from source

Requires Rust 1.78+ (see workspace `rust-version` in `Cargo.toml`):

```bash
git clone https://github.com/egon1024/DNSConduit.git
cd DNSConduit
cargo build --release -p conduit -p conduitctl -p conduit-dnstap-tracer
```

Binaries are in `target/release/`. Example configs for packaging live in `packaging/examples/`.

## Validate before run

```bash
conduitctl validate --file /etc/conduit/conduit.yaml
```

On success the command prints `ok`.

## Related

- [Minimal configuration](/getting-started/minimal-configuration.md) — smallest runnable YAML
- [First query](/getting-started/first-query.md) — send a test query
- [Config file](/control-plane/config-file.md) — load, validation, reload
