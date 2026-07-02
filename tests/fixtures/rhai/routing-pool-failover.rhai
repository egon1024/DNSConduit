let primary = runtime.routing().pool("primary");
if primary.configured() && primary.eligible_count() < primary.configured_count() {
    txn.set_pool("secondary");
}
