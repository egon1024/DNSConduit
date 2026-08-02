# Ingress concurrency (sync)

<div class="study-question" markdown="1">

How much throughput do you buy by raising UDP ingress thread count under
`dataplane.runtime:` [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default),
and does that answer change when the upstream is slow?

</div>

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

On **[sync](/concepts/runtime-and-concurrency.md#sync-runtime-default)**,
listener [ingress `threads`](/reference/config-schema/listeners.md) are the
main concurrency setting: the same workers receive the query and wait on the
upstream. Two questions follow from that, and this study runs the same ladder
twice to answer them separately. Against a fast upstream, where each worker's
wait is short, the ladder is a **sizing curve** — add workers until achieved QPS
flattens, then stop. Against a slow upstream, where each worker is parked for
the whole round trip, the ladder shows whether thread count is the binding limit
at all. Pair with [`listeners.reuse_port`](/reference/config-schema/listeners.md)
when `threads > 1`. See
[worker counts](/concepts/runtime-and-concurrency.md#worker-counts-and-limits)
and [dataplane runtime tuning](/guides/dataplane-runtime-tuning.md).

## What we varied

- **Varied:** UDP listener `threads` (`1`, `2`, `4`, `8`) with
  `dataplane.runtime:` [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)
- **Varied:** load shape —
  [`forward_fast`](/performance/methodology.md#load-shapes) for the sizing curve,
  [`forward_slow`](/performance/methodology.md#load-shapes) for the
  slow-backend illustration
- **Held fixed:** observability off fixtures, one dnsperf recipe per shape on the
  named lab profile
- **Note:** the `ingress=2` rung at each shape reuses that shape's sync scale cell

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress ladder (forward_fast)

![Achieved QPS — sync ingress ladder (forward_fast)](../generated/ingress-concurrency-sync-forward-fast.svg)

[Download CSV](../generated/ingress-concurrency-sync-forward-fast.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-fast) | sync | 38584.0 | 4.3 | 389667 | 386003 | 3664 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-fast) | sync | 74932.5 | 3.6 | 753275 | 749881 | 3465 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-fast) | sync | 101956.1 | 2.8 | 1023433 | 1020013 | 3420 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-fast) | sync | 198042.2 | 2.8 | 1984315 | 1981323 | 2992 | ingress=8 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress ladder (forward_slow)

![Achieved QPS — sync ingress ladder (forward_slow)](../generated/ingress-concurrency-sync-forward-slow.svg)

[Download CSV](../generated/ingress-concurrency-sync-forward-slow.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-slow) | sync | 2.8 | 2514.6 | 12093 | 99 | 11994 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-slow) | sync | 5.7 | 2509.5 | 12188 | 198 | 11990 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-slow) | sync | 11.4 | 2512.9 | 12380 | 398 | 11978 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-slow) | sync | 21.7 | 2591.5 | 12692 | 757 | 11950 | ingress=8 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **sync ingress ladder (forward_fast):** `2` is about **1.9×** `1` (~75k vs ~39k); `4` is about **2.6×** `1` (~102k vs ~39k); `8` is about **5.1×** `1` (~198k vs ~39k).
- **sync ingress ladder (forward_slow):** `2` is about **2.0×** `1` (~6 QPS vs ~3 QPS); `4` is about **4.0×** `1` (~11 QPS vs ~3 QPS); `8` is about **7.6×** `1` (~22 QPS vs ~3 QPS).
<!-- perf-study-deltas:end -->

## Takeaway

**More sync ingress threads raise throughput against a fast upstream.** On this
lab, under [`forward_fast`](/performance/methodology.md#load-shapes), achieved
QPS climbs about **39k → 75k → 102k → 198k** as you go from 1 to 2 to 4 to 8
workers. Gains are large from 1→2 and 4→8 (~2× each); 2→4 is smaller. Eight
workers is still climbing here — keep adding threads only while *your*
remeasure still buys QPS.

**Against a slow upstream, more threads help a little and do not fix the
model.** Under [`forward_slow`](/performance/methodology.md#load-shapes),
completed QPS scales with thread count (~3 → 6 → 11 → 22), but absolute
throughput stays tiny, latency stays ~2.5 s, and most queries are lost. Extra
workers buy more completions; they do not make
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) a good fit
when the backend owns the wait.

**What to do:** choose `listeners.threads` from the fast ladder (enable
[`reuse_port`](/reference/config-schema/listeners.md) when `threads > 1`). If
upstreams are slow or variable, prefer
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) instead of
stacking sync threads — see
[sync vs split_io](/performance/studies/sync-vs-split-io.md) and
[I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md).

## Related guides

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — sync model and worker roles
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [Reference: listeners](/reference/config-schema/listeners.md) — `threads`, `reuse_port`, `rcvbuf`
- [Reference: dataplane](/reference/config-schema/dataplane.md)
- [I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md)
- [Sync vs split_io](/performance/studies/sync-vs-split-io.md)

## Member scenarios

- [scale-sync-ingress-1-forward-fast](/performance/scenarios.md#scale-sync-ingress-1-forward-fast)
- [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast)
- [scale-sync-ingress-4-forward-fast](/performance/scenarios.md#scale-sync-ingress-4-forward-fast)
- [scale-sync-ingress-8-forward-fast](/performance/scenarios.md#scale-sync-ingress-8-forward-fast)
- [scale-sync-ingress-1-forward-slow](/performance/scenarios.md#scale-sync-ingress-1-forward-slow)
- [scale-sync-forward-slow](/performance/scenarios.md#scale-sync-forward-slow)
- [scale-sync-ingress-4-forward-slow](/performance/scenarios.md#scale-sync-ingress-4-forward-slow)
- [scale-sync-ingress-8-forward-slow](/performance/scenarios.md#scale-sync-ingress-8-forward-slow)
