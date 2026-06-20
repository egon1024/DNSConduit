if txn.has_tag("suspicious") {
    if txn.last_forward_ms() > 500 {
        txn.metric_inc("slow_login", 1);
    }
}
