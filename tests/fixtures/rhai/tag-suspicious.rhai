if question_qname(txn) == "login.suspicious.example." {
    txn.set_tag("suspicious", true);
}
