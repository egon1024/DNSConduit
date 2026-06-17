// Rhai keyed sampling: per-qname salt via second argument.
if txn.sample_percent(10.0, question_qname()) {
    txn.set_tag("rhai_sampled", true);
}
