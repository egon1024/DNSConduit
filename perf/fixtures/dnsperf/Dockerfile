# DNS-OARC dnsperf — pinned thin image for the performance harness (D13).
# Build: docker build -t dnsconduit-dnsperf:2.14.0 -f perf/fixtures/dnsperf/Dockerfile perf/fixtures/dnsperf
FROM debian:bookworm-slim

ARG DNSPERF_VERSION=2.14.0

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        build-essential \
        pkg-config \
        libssl-dev \
        libck-dev \
        libldns-dev \
        libnghttp2-dev \
    && curl -fsSL \
        "https://www.dns-oarc.net/files/dnsperf/dnsperf-${DNSPERF_VERSION}.tar.gz" \
        -o /tmp/dnsperf.tar.gz \
    && mkdir -p /tmp/dnsperf-src \
    && tar -xzf /tmp/dnsperf.tar.gz -C /tmp/dnsperf-src --strip-components=1 \
    && cd /tmp/dnsperf-src \
    && ./configure --prefix=/usr/local \
    && make -j"$(nproc)" \
    && make install \
    && cd / \
    && rm -rf /tmp/dnsperf.tar.gz /tmp/dnsperf-src \
    && apt-get purge -y build-essential curl \
    && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*

ENTRYPOINT ["dnsperf"]
