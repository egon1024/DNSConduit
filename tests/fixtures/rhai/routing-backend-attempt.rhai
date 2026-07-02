let backend = runtime.routing().backend_for_attempt(
    txn.selected_pool(),
    txn.selected_backend_name()
);
if backend.configured() && backend.applied() == "down" {
    txn.request_retry();
}
