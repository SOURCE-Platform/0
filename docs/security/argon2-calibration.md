# Argon2id calibration (spec §2.3, Phase B gate deliverable)

Date: 2026-09-18. Author: Phase B gate run on the development machine.

> **Status (2026-09-20, Phase D.1):** `m = 64 MiB, t = 3, p = 1` remains
> the **current/provisional** v1 tuple. iPhone evidence now exists (below)
> and is comfortably inside the §2.3 budget, but the *oldest supported*
> iPhone class (A12 / iPhone XS–XR, the iOS 17 floor) is still unmeasured,
> so the owner decision is: **do not freeze yet** — freeze only once an
> A12-class device has been measured (release-gate item 30). The tuple must
> not be weakened in the meantime. This open item does not block Phase E.

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
v1 tuple — is committed unchanged as the **provisional** v1 default,
because:

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

**Open item before parameter freeze:** the table above is single-class
evidence (M2 Air). Per §2.3, freeze requires measured unlock/recovery
latency and memory pressure on the supported iPhone floor and any other
supported hardware class; those measurements must be appended here before
the tuple is declared final. The provisional tuple stays in force, and
stays at full strength, until then.

Memory-pressure note: peak working set tracks the configured memory
parameter within measurement noise (65,552 KiB observed for the 64 MiB
tuple), confirming no hidden overhead in the `argon2` crate's allocator
behavior for our usage.

## Reproduce

```bash
cd src-tauri && cargo run -p source-vault-helper --release --bin kdf_bench
```

## iPhone measurement (2026-09-20, Phase D.1)

Device: **iPhone 13 mini (iPhone14,4, A15 Bionic)**, iOS 27.0, 3674 MB RAM,
on charge, screen on, no other foreground app.

Code: the **production crate and parameters** (`argon2` 0.6, Argon2id,
`Version::V0x13`, 32-byte output — the same call `crypto::kdf::derive_pk`
makes), cross-compiled to `aarch64-apple-ios` (release, LTO) and linked
into a throwaway SwiftUI harness. Synthetic password and a fixed salt; no
credential material. Harness kept outside the repos; not committed.

| Run | Latency (ms) |
|---|---|
| 1 (cold) | 124 |
| 2 | 91 |
| 3–7 | 89, 89, 89, 89, 89 |

- **Median: 89 ms. Worst observed: 124 ms** (first run, cold allocation).
- Budget (§2.3): ≤ 2 s on the oldest supported iPhone. The measured device
  is ~22× inside that budget at the median.
- **Memory pressure:** the 64 MiB block allocated and freed on every run
  with no `didReceiveMemoryWarning` and no jetsam kill across 7 runs on a
  4 GB device. Note the vault on iOS is the **app** (§1.7), not an
  extension, so the tighter extension memory limits do not apply.
- **Extrapolation is not evidence:** an A12 (iPhone XS/XR) is roughly
  2–3× slower than an A15 on memory-hard work, which would still land far
  inside 2 s — but that is an estimate, not a measurement, and the freeze
  decision waits for a real A12-class run.

### Remaining for the §2.3 freeze

| Class | Status |
|---|---|
| MacBook Air M2 (Mac14,2) | measured 2026-09-18 |
| iPhone 13 mini (A15) | measured 2026-09-20 |
| iPhone XS/XR class (A12, iOS 17 floor) | **not measured** — blocks the freeze |
| Other supported Mac classes (Intel? older Apple silicon) | not enumerated/measured |

