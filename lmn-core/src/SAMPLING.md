# Sampling in lmn-core

At high concurrency a load test can produce millions of individual request results.
Storing every result in memory is impractical and unnecessary — aggregate statistics
(p50, p99, error rate) can be computed from a representative sample.

`lmn-core` uses a **two-stage sampling pipeline** implemented in `sampling.rs`:

```
completed request
       │
       ▼
┌─────────────────────┐
│  Stage 1            │  VU-threshold gate
│  should_collect()   │  — probabilistic drop when VU count is high
└────────┬────────────┘
         │ pass
         ▼
┌─────────────────────┐
│  Stage 2            │  Reservoir gate
│  reservoir_slot()   │  — Vitter's Algorithm R
└────────┬────────────┘
         │
    Push / Replace / Discard
```

---

## Stage 1 — VU-threshold gate

**Purpose:** prevent the reservoir from becoming the bottleneck when hundreds of
VUs are firing simultaneously.

**Rule:** while `active_vus <= vu_threshold` (default 50), every result passes.
Above the threshold the acceptance probability is capped:

```
sample_rate = vu_threshold / active_vus
```

| Active VUs | Threshold | Rate  |
|------------|-----------|-------|
| 30         | 50        | 1.0   |
| 50         | 50        | 1.0   |
| 100        | 50        | 0.50  |
| 200        | 50        | 0.25  |
| 1 000      | 50        | 0.05  |

Each request draws `random::<f64>() < sample_rate` — a fair Bernoulli trial.

Setting `vu_threshold = 0` disables this gate entirely (rate is always 1.0).

---

## Stage 2 — Vitter's Algorithm R

Results that pass Stage 1 enter a fixed-size reservoir of capacity `k`
(default 100 000).

### The problem it solves

We want a **uniform random sample of size k** from a stream of unknown length `n`,
using O(k) memory regardless of how large `n` grows.

A naive approach — "collect the first k, then replace randomly" — introduces
selection bias. Algorithm R avoids this with a clean mathematical guarantee:
after processing `n` items, every item has an equal `k/n` probability of being
in the reservoir.

### The algorithm

Let `k` = reservoir capacity, `t` = number of items seen so far (including this one).

```
for each incoming item (the t-th item seen):
    if t <= k:
        push item into reservoir          # fill phase
    else:
        j = random integer in [0, t)
        if j < k:
            reservoir[j] = item           # replace phase
        else:
            discard item
```

**Correctness sketch:**

- Fill phase (t <= k): every item is kept, inclusion probability = 1.0 >= k/t. ok
- Replace phase (t > k): the new item is accepted with probability k/t.
  Any previously stored item at slot j survives with probability `1 - 1/t = (t-1)/t`.
  By induction, if each stored item had probability k/(t-1) before this step,
  after this step it has `k/(t-1) * (t-1)/t = k/t`. ok

All items end up with exactly `k/t` probability — a uniform sample.

### Mapping to the code

```rust
pub fn reservoir_slot(&mut self, results_len: usize) -> ReservoirAction {
    self.total_seen_for_reservoir += 1;           // t = total seen so far
    if results_len < self.reservoir_size {
        ReservoirAction::Push                     // fill phase: t <= k
    } else {
        let j = self.rng.random_range(0..self.total_seen_for_reservoir); // j in [0, t)
        if j < self.reservoir_size {
            ReservoirAction::Replace(j)           // accepted: replace slot j
        } else {
            ReservoirAction::Discard              // rejected
        }
    }
}
```

`total_seen_for_reservoir` is the denominator `t`. It counts only the items that
reached Stage 2 (i.e. passed Stage 1), not the raw request total.

### Worked example

`k = 3`, processing 6 results that all pass Stage 1:

| t | Action                               | Reservoir after      |
|---|--------------------------------------|----------------------|
| 1 | Push                                 | [R1]                 |
| 2 | Push                                 | [R1, R2]             |
| 3 | Push                                 | [R1, R2, R3]         |
| 4 | j=random[0,4): j=1 < 3 -> Replace(1) | [R1, R4, R3]        |
| 5 | j=random[0,5): j=4 >= 3 -> Discard  | [R1, R4, R3]         |
| 6 | j=random[0,6): j=0 < 3 -> Replace(0) | [R6, R4, R3]        |

After 6 items, each item has probability 3/6 = 0.5 of being in the reservoir.

---

## min_sample_rate

`SamplingState` tracks the lowest `sample_rate` ever observed across all
`set_active_vus()` calls:

```rust
self.min_sample_rate = self.min_sample_rate.min(self.sample_rate);
```

This is recorded even if VUs later drop back below the threshold. It is exposed
on the output report so consumers can understand the worst-case data density:

- `min_sample_rate = 1.0` — every request was stored (or eligible for storage)
- `min_sample_rate = 0.05` — at peak load, only 1 in 20 requests entered the reservoir

Statistics computed from the reservoir (latency percentiles, error rate) remain
unbiased by construction (Algorithm R guarantees uniform selection), but
`min_sample_rate` lets operators judge whether the sample was large enough for
their confidence requirements.
