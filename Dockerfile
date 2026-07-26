# DNSConduit server image (Conduit-native). Not derived from third-party DNS packaging.
# Build: docker build -t conduit:local -f Dockerfile .
# Release tags: ghcr.io/<owner>/dnsconduit:<version>
#
# Uses current stable Rust (not MSRV pin alone): lockfile deps may need edition2024 Cargo.

FROM rust:bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY proto ./proto
RUN cargo build --release -p conduit \
    && strip target/release/conduit

FROM debian:bookworm-slim AS runtime
# Upgrade base packages at build time so release images pick up Debian
# security fixes that landed after the bookworm-slim snapshot was published.
RUN apt-get update \
    && apt-get upgrade -y --no-install-recommends \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --uid 10001 conduit
COPY --from=build /src/target/release/conduit /usr/local/bin/conduit
USER conduit
EXPOSE 53/udp 53/tcp
ENTRYPOINT ["/usr/local/bin/conduit"]
CMD ["/etc/conduit/conduit.yaml"]
