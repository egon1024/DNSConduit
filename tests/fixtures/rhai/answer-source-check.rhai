if txn.answer_source() == "cache" {
    metrics.inc("obs_cache_hit", 1);
} else if txn.answer_source() == "forward" {
    metrics.inc("obs_cache_miss", 1);
}
