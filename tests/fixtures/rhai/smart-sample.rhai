if txn.sample_percent(5.0) {
    txn.set_tag("sampled", true);
}
