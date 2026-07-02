// Keyed sampling: 10% of queries by qname
if txn.sample_percent(10.0, txn.question().qname) {
    txn.set_tag("audit_sample", true);
}
