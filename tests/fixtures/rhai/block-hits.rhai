let cat = table_lookup("geo", question_qname(txn));
if cat == "eu" {
    txn.metric_inc_labels("block_hits", 1, #{ category: "eu" });
} else if cat == "us" {
    txn.metric_inc_labels("block_hits", 1, #{ category: "us" });
}
