let region = table_lookup("geo", question_qname(txn));
if region != "" {
    txn.set_tag("region", region);
}
