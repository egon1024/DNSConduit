if lookup("blocklist", txn.question().qname) == "block" {
    metrics.inc("block_hits", 1);
    txn.drop_query();
}
