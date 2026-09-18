# Argon2id calibration (spec §2.3, Phase B gate deliverable)

Date: 2026-09-18. Author: Phase B gate run on the development machine.

## Rule being satisfied

§2.3 pins the v1 tuple `m = 64 MiB, t = 3, p = 1` (RFC 9106 §7.4 second
profile with a parallelism reduction, attribution corrected in v0.3) and
requires a calibration gate before parameters freeze: benchmark on
supported hardware, keep interactive cost ≤ 1 s on a 2020 MacBook Air
(M1) and ≤ 2 s on the oldest supported iPhone, and commit the table
alongside the chosen tuple. Recovery security must not be weakened for
speed.

## Measurements

Machine: Mac14,2 (MacBook Air, Apple M2), macOS 26.6.2. Build: release
(`cargo run -p source-vault-helper --release --bin kdf_bench`). 3 samples
per candidate after 1 warmup, minimum latency reported.

| memory (KiB) | time | lanes | min latency (ms) | ~peak working set (KiB) | note |
|---|---|---|---|---|---|
| 65536 | 3 | 1 | 115 | 65552 | **spec v1 tuple** |
| 65536 | 4 | 1 | 140 | 65536 | time-up neighbor |
| 131072 | 1 | 1 | 68 | 131072 | memory-up neighbor |
| 131072 | 2 | 1 | 231 | 131072 | 128 MiB candidate |
| 131072 | 3 | 1 | 305 | 131072 | §2.3 upgrade candidate |

## Interpretation

- The v1 tuple costs **115 ms** on this 2022 M2 Air. The slowest
  supported Mac class per §2.3 is the 2020 M1 Air; single-thread Argon2id
  scales roughly with per-core performance, so the same tuple lands at an
  estimated ~130–150 ms there — under 15% of the 1 s budget.
- The 128 MiB/t=3 upgrade candidate costs 305 ms here (~350–400 ms
  estimated M1), also inside the budget, but it doubles peak memory.
  On-memory-budget devices (iPhone enrollment and recovery flows arrive
  in Phase E) need measured evidence before adopting it.

## Chosen tuple and justification

**`m = 65536 KiB (64 MiB), t = 3, p = 1, outlen = 32`** — the spec-pinned
v1 tuple — is committed unchanged, because:

1. It is RFC 9106 §7.4's second recommended profile in the two dominant
   hardness parameters (memory cost and iteration count); the only
   deviation is `p = 1`, the spec's deliberate application-specific
   parallelism reduction for predictable latency on memory-constrained
   devices (§2.3, attribution paragraph).
2. It satisfies the §2.3 latency gate with wide margin on the slowest
   supported Mac class (measured here at 115 ms on an M2; estimated
   ≤ 150 ms on an M1).
3. Freezing at v1 keeps every Phase B artifact (wrap files, tests,
   vectors, CR-01…04) on one parameter set; parameter changes are a
   `kdf_version` bump with re-wrap on next unlock (§2.3 upgrade path).
4. The 128 MiB/t=3 candidate is recorded with evidence (305 ms) as the
   v2 upgrade option. Adopting it is a spec decision for a later phase,
   gated on iPhone-class measurements that Phase B does not have —
   Phase B ships Mac-only, and weakening memory cost for speed is
   explicitly forbidden, so no downward adjustment was considered.

Memory-pressure note: peak working set tracks the configured memory
parameter within measurement noise (65,552 KiB observed for the 64 MiB
tuple), confirming no hidden overhead in the `argon2` crate's allocator
behavior for our usage.

## Reproduce

```bash
cd src-tauri && cargo run -p source-vault-helper --release --bin kdf_bench
```
