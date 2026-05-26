// Pin egress to loopback for upstream forward (must be in forward.sources_v4).
txn.set_source_v4("127.0.0.1");
