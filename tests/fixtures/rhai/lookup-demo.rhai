let region = lookup("geo", txn.question().qname);
if region == "eu" {
    txn.set_tag("region", "eu");
}
