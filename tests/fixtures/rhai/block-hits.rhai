let cat = lookup("geo", txn.question().qname);
if cat == "eu" {
    metrics.inc_labels("block_hits", 1, #{ category: "eu" });
} else if cat == "us" {
    metrics.inc_labels("block_hits", 1, #{ category: "us" });
}
