if txn.question().qname == "login.suspicious.example." {
    txn.set_tag("suspicious", true);
}
