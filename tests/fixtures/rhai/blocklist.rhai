if table_lookup("blocklist", question_qname(txn)) == "block" {
    txn.set_tag("blocked", true);
    txn.drop_query();
}
