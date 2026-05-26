// Intentionally calls response() during request hook — phase guard, fail-open.
txn.response();
