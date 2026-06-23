if table_lookup("blocklist", question_qname(txn)) == "block" {
    txn.metric_inc("block_hits", 1);
    txn.drop_query();
}
