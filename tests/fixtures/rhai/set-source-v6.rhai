// Pin egress to loopback IPv6 (must be in forward.sources_v6 or pool override).
txn.set_source_v6("::1");
