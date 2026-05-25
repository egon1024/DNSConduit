if txn.sample_include(0.05) {
    txn.set_tag("sampled", true);
}
