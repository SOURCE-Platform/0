# Phase F design closure — credential vault

| | |
|---|---|
| **Date** | 2026-09-24 (revision 3, incorporating D-1…D-13, S-1…S-5, O-1/O-2 and the final review corrections) |
| **Status** | **APPROVED as the Phase F design baseline (owner, 2026-09-24; committed `cfc6b80`). Folded into spec v0.4, which is normative and supersedes this record where they differ — notably the v0.4 internal review amended S-5 (the shared throttle covers the MP class only; RK-class recovery is exempt) and added revocation re-keying of both recovery classes, inline `create` bootstrap blobs and `setup_retry_handle`** |
| **Phase F** | **NOT AUTHORIZED / NOT STARTED** |
| **Inputs** | `phase-f-readiness.md` (`72b15b0`); `credential-vault-implementation-spec.md` v0.3.1; `credential-vault-security-architecture.md`; `src-tauri/vault-helper/src/**`; `vault-apple-crypto`; SourceMobile `Vault/`; vendor documentation (§5) |
| **Repo state** | desktop `main` @ `72b15b0`, readiness file restored to `HEAD`; iPhone `main` @ `d6fe885` |

This document proposes how to close the Phase F readiness blockers. The
implementation specification and the architecture document stay normative
until the owner approves corrections. Nothing here authorizes
implementation.

**Labels used throughout:**

- **[Fact]** — verified in the repository or spec, with a location.
- **[Owner]** — an owner decision already taken (D-n). Its status is shown
  in §12.
- **[Rec]** — my technical design.
- **[Closed S-n / O-n]** — a security or owner choice that was open in
  revision 2 and is now decided (§12.2). No security decision remains open
  in this revision.

---

## 1. Executive summary

**Authentication (D-1):**

- The provider authenticates every request by a **P-256 signature** over a
  canonical, domain-separated request structure (`ov0/provider/request/v2`,
  §4.3).
- Devices sign with their existing **Secure Enclave signing key**. The
  provider authorizes them against the `sign_pub` of the currently accepted
  registry.
- MP/RK recovery requests are signed with keys derived deterministically
  inside the helper. The derivation is RFC 9180's `DeriveKeyPair` for
  DHKEM(P-256, HKDF-SHA256), using only the current crypto stack (§4.2).
- **No symmetric backup credential exists anywhere.** The provider stores
  only public keys.
- `device_backup_cred`, `creds.bin`, `device_register`, the revoke
  endpoint and finalize tag `0x0B` are all removed (D-13).

**Storage and state (D-2, D-3):**

- Amazon S3 holds immutable SHA-256-addressed blobs, written only with
  `If-None-Match: *`.
- Each vault has one small **vault-state object**. It changes only by an
  atomic **state transition** (formerly `manifest_cas`), written with
  `If-Match` on the previous ETag.
- The client-visible CAS token is a cryptographic **state commitment**;
  ETags are used only as concurrency tokens.
- Publication, recovery finalize and account creation are all kinds of one
  transition (§7).
- The v1 overwrite race (N1) is impossible. Vault data is only ever
  immutable blobs plus the CAS-only state object. The other mutable
  objects (handle claims, rate-limit slots) are provider operational
  state, also written only conditionally, and never a vault root of trust
  (§7.1).

**Revisions (D-5):**

- Each revision gets a **stable random 256-bit `revision_id`** when it is
  authored, and parents refer to these ids.
- Encrypted objects are stored under `blob_hash = SHA-256(bytes)`. The
  signed index maps `revision_id → blob_hash`.
- A VK rotation re-encrypts revisions and changes their blob hashes, but
  never renames the DAG. The v1 "rotation barrier/remap" problem therefore
  disappears.
- Counters use an exact ancestry-based algorithm that out-of-order delivery
  cannot trip (§10.4).
- Unseen revisions from a revoked author are **refused** under a precisely
  defined cutoff (§10.6).

**Transfer (§9):** ≤ 24 KiB raw chunks (≈ 33 KiB encoded frames),
pull-based for output and acknowledged writes for input. The same
mechanism carries the enrollment bundle.

**States:** BACKING_UP, SYNCING, ROTATING_KEYS, RECOVERING and COMPROMISED
become real states with defined transitions (§6.3). ROTATING_KEYS is
internal, with a status/progress event (O-2).

**Remote completion is explicit (§4.2.6):** MP changes, RK replacements,
revocations and enrollments go through LOCAL_COMMITTED →
REMOTE_UPDATE_PENDING → REMOTE_COMMITTED. The UI never claims a remote
cutoff before the provider transition commits.

**Mobile (D-6, narrowed):** Phase F adds only iPhone **envelope
catch-up**:
- fetch and verify the current state, the registry and the device's own
  envelope from the provider;
- the envelope v2 parser that D-13 makes mandatory anyway.

Full iPhone record sync is deferred.

**Recovery lookup (D-9):**

- A user-chosen, normalized, **public** recovery handle. Email is
  optional, never required.
- Rate-limited lookup that returns only public KDF metadata. The client
  enforces the exact frozen Argon2id policy before any MP derivation (no
  downgrade) and cross-checks against the authenticated header afterwards
  (§4.4).
- A fake-but-consistent response for unknown handles (the pepper).
- Handle claims are crash-safe and reclaimable (§7.4.1).
- Recovery-auth failures are throttled **provider-wide**, not per
  instance (§4.5).
- The handle is printed on the Recovery Key sheet.

**Security decisions:** S-1…S-5 and O-1/O-2 are **closed** (§12.2).
Total-loss recovery revokes every prior device in the finalize
transition (S-4).

**Keychain (D-10):** the cleanup matcher was dry-run read-only. It
selects exactly the 370 stale test items and cannot select production
entries (§12.3).

**Dependabot #108:** recorded, not dismissed, not a blocker (§15).

---

## 2. Confirmed contradictions and findings

Carried over from revision 1 and still valid.

**Contradictions:**

| ID | Summary | Resolved by |
|---|---|---|
| C1 | Backup credentials would cross IPC | §4 |
| C2 | Object addressing is inconsistent | §7 |
| C3 | The §11.1 trait doesn't match the architecture | §3 |
| C4 | Revocation order is inconsistent | §8 |
| C5 | CAS format undefined | §7.4 |
| C6 | 64 KiB frames vs 1 MiB objects | §9 |
| C7 | Zero author id | §10.1 |
| C8 | Conflict resolution impossible | §10.3 |
| C9 | Envelopes not backed up | §11 |

**Findings:**

| ID | Summary | Resolved by |
|---|---|---|
| N1 | Overwrite race | §7.1 |
| N2 | Rotation renames revisions | §10.2 — **dissolved** by stable ids |
| N3 | LOCKED-state signing impossible with symmetric credentials | §4.1 |
| N4 | Client deletion | D-4 |
| N5 | v1 MAC framing ambiguous | §4.3 |
| N6 | Recovery re-registration not atomic | §4.2.4 |
| N7 | Keychain PID reuse | §12.3 |
| N8 | SE keys have no ACL | §4.1 |

**New in this revision [Fact]:**

- **N9 — total-loss recovery leaves lost devices enrolled.**
  - §4.4 rule 6 installs the replacement device.
  - Nothing revokes the devices that were lost; §12 scenario 3 has no
    revoke step.
  - Their `sign_pub`s stay valid registry authorizers. Under D-1 they
    would also stay authenticated at the provider.
  - A thief holding a lost, unlockable Mac could sign `enroll` entries.
    Those devices get no VK, but the registry is polluted.
  - **Resolved by S-4 (approved):** `finalize` revokes every prior device
    (§7.4 step 6).
- **N10 — leftover Secure Enclave key blobs.** The login Keychain holds
  **542** `com.racker.zero.vault.se-signing` and **542** `…se-agreement`
  items, all tagged `dev.<hex>`.
  - Tests apparently create SE keys without deleting them.
  - These are *not* covered by the D-10 matcher. Production and test tags
    share the `dev.` pattern, so a pattern-based delete could hit a real
    key.
  - §12.3 proposes a distinct test tag prefix going forward. Cleanup of
    the existing 542 needs an allowlist, deferred.
- **N11 — the iPhone never checks the checkpoint against the manifest.**
  `VaultCheckpoint.swift` parses `manifest_core_hash` but never compares
  it. Closed by §11.3 as part of envelope catch-up.

**Found during the final review (revision 3):**

- **N12 — KDF downgrade via unauthenticated locate.** The locate response
  supplies Argon2 parameters before anything is authenticated. A
  malicious provider could lower them and obtain a cheap MP-derived
  signing verifier. **Closed by §4.4.2**: an exact frozen-tuple policy,
  plus a post-authentication cross-check.
- **N13 — `create` spans two S3 keys** (handle claim + state) with no
  transaction. **Closed by §7.4.1**: a claim lifecycle
  `pending → bound`, CAS on the claim, grace-period reclaim, rollback of an
  orphaned generation-1 state.
- **N14 — the remote cutoff lags the local change.** Between a local MP
  change or RK replacement and the provider commit, the *old* MP/RK still
  recovers the (old) remote state. **Closed by §4.2.6**: explicit
  REMOTE_UPDATE_PENDING, priority retry, and UI copy that never claims the
  cutoff early.

---

## 3. Recommended Phase F architecture

```text
 ┌──────────────── Mac ─────────────────┐            ┌──────── Provider (container) ────────┐
 │ vault-helper (signed, no network)    │            │ vault-provider (Rust, stateless)     │
 │  • builds/seals/verifies all state   │ typed IPC  │  axum routes (§7.3)                  │
 │  • signs provider requests (§4.3)    │◀──────────▶│  ProviderCore<StateStore,BlobStore,  │
 │  • staging dir: ciphertext only      │ ≤24 KiB    │               NonceStore>            │
 │          ▲ chunked streams (§9)      │ chunks     │  S3: blobs (If-None-Match: *)        │
 │ main app: ProviderTransport (HTTPS)  │── HTTPS ──▶│      state (If-Match: ETag)          │
 │           BackupCoordinator          │            │      nonces (If-None-Match: *)       │
 └──────────────────────────────────────┘            └──────────────────────────────────────┘
 iPhone (Phase F only): envelope catch-up over HTTPS, signed with its SE key (§11.3)
 shared pure-Rust crate `vault-proto`: TLV, object/index/manifest v2, checkpoint, registry
 decode + structural chain verify, request TLV, state commitment, handle normalization
```

| Crate / component | Responsibility |
|---|---|
| `vault-proto` (new) | Wire types and secret-free verification. Moved out of `vault-helper` without behaviour change; the existing tests gate the move. No rusqlite, objc2 or Swift |
| `vault-provider-core` (new) | All provider semantics over three traits: `StateStore` (load → `(state, etag)`, `create_if_absent`, `replace_if_match`), `BlobStore` (`put_if_absent`, `get`, `exists`, `gc_delete`), `NonceStore` (`insert_if_absent`). `FsStores` implements them for tests and rehearsals |
| `vault-provider` (new binary) | axum adapter + `S3Stores`; container image; no APNs (D-8), route module reserved |
| `vault-helper` | Builds, signs and verifies. The Phase D recovery engine becomes sans-IO step functions. `FsBackupStore` is **retired**; its semantics move into `ProviderCore` |
| Main (`zero_lib`) | `ProviderTransport` (`InProcess` over `ProviderCore`+`FsStores` for tests; `Https` for production, standard platform/public-CA TLS per D-12) and `BackupCoordinator` (sequencing, backoff, queue) |

Hosting is not chosen (D-2). The provider is a normal containerized Rust
service with no platform-specific APIs.

---

## 4. Credential / authentication design

### 4.1 Device class (D-1, approved)

**Signing:**

- The key is the existing SE signing key (`sign_pub`, registry field).
- The helper signs through the existing bridge call `ov0_se_sign_digest`.
  No bridge change is needed, and the bridge stays at 200/200 lines.
- **No presence is required.**
  - [Fact] N8: the keys are created without access-control flags; their
    blobs are `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`.
  - [Owner] Background signing must not trigger Touch ID/LA.
  - If the Keychain or SE is unavailable (for example, the machine is
    locked), the helper returns `KEYCHAIN_UNAVAILABLE`. The coordinator
    queues and retries.
  - ACLs are never changed and no prompt is ever raised.
- **LOCKED state:** background reads (`state_get`, `blob_get`) are
  signable while the vault is LOCKED, because they need no VK. This closes
  N3.

**Provider authorization:**

- `state.active_devices` is derived from the registry blob of the
  current committed state. Active means installed by genesis, enroll or
  recovery_epoch and not revoked.
- A device's key becomes active when a state transition commits a
  registry that installs it.
- It becomes inactive when a transition commits a registry that revokes
  it (§8).

### 4.2 Recovery class — exact derivation (D-1 refinement)

**Goal:** give the provider only public keys for MP/RK recovery. No
reusable symmetric secret, no ad-hoc scalar reduction, no new primitive.

#### 4.2.1 Inputs

| Input | MP class | RK class |
|---|---|---|
| Secret (never the raw password) | `PK` = Argon2id(MP, `header.kdf.salt`, m=64 MiB, t=3, p=1), 32 B | `RK_bytes` = BIP-39 entropy, 32 B (§2.4) |
| Class salt (public, 16 B, OsRng) | `header.auth_salt_mp` | `header.auth_salt_rk` |
| Vault binding | `vault_id` (16 B) | `vault_id` (16 B) |
| Domain (§2.9, new) | `ov0/provider-recovery-auth/mp/v2` | `ov0/provider-recovery-auth/rk/v2` |

`auth_salt_mp`/`auth_salt_rk` **replace** v1's `locator_salt_mp`/`locator_salt_rk`
in header v2. Locators are removed (§4.2.5).

#### 4.2.2 Derivation

```text
ikm_c   = HKDF-SHA256(ikm  = secret_c,                 # PK or RK_bytes
                      salt = auth_salt_c,               # 16 B
                      info = domain_c ‖ vault_id,       # e.g. "ov0/provider-recovery-auth/mp/v2" ‖ 16 B
                      L    = 32)

(sk_c, pk_c) = DeriveKeyPair_DHKEM_P256_HKDF_SHA256(ikm_c)     # RFC 9180 §7.1.3, KEM id 0x0010
```

**`DeriveKeyPair`, exactly as RFC 9180 §7.1.3 for DHKEM(P-256,
HKDF-SHA256):**

```text
suite_id = "KEM" ‖ I2OSP(0x0010, 2)
LabeledExtract(salt, label, ikm)   = HKDF-Extract(salt, "HPKE-v1" ‖ suite_id ‖ label ‖ ikm)
LabeledExpand(prk, label, info, L) = HKDF-Expand(prk, I2OSP(L,2) ‖ "HPKE-v1" ‖ suite_id ‖ label ‖ info, L)

dkp_prk = LabeledExtract("", "dkp_prk", ikm_c)
for counter in 0..=255:
    bytes    = LabeledExpand(dkp_prk, "candidate", I2OSP(counter,1), 32)
    bytes[0] &= 0xFF                      # P-256 bitmask (no-op)
    sk       = OS2IP(bytes)
    if 0 < sk < n: return (sk, sk·G)      # rejection sampling — no reduction, no bias
error DeriveKeyPairError                   # probability ≈ 2^(−32·256)
```

- `pk_c` is serialized as 65-byte uncompressed X9.63.
- `key_id_c = SHA-256(pk_c)`.

**Why this construction [Rec]:**

- It is standardized and specified to the byte.
- It is **unbiased** (rejection sampling on 256-bit candidates).
- It comes with official test vectors: RFC 9180 Appendix A.3 lists
  `ikmE → skEm/pkEm` and `ikmR → skRm/pkRm` for this exact KEM.
- It uses only primitives already in the helper: `hkdf` (Extract/Expand),
  `sha2`, and `p256` (`SecretKey::from_bytes` rejects 0 and values ≥ n).
- It is ~30 lines. No dependency and no feature flag is added.
- The `hpke` crate in the PoC workspace can cross-check it in tests only.

**Signing:** ECDSA-P256-SHA256 over the §4.3 prehash, with an RFC 6979
deterministic nonce (`p256::ecdsa::SigningKey`, already used by the
helper's software identities in `registry/device.rs`), normalized to
low-S. Recovery-class signatures are therefore **byte-reproducible** for
vectors.

**[Closed S-1 — approved, with this qualification, which becomes
normative text]:**

- **The composition** is `PK or RK_bytes → domain-separated HKDF →
  RFC 9180 DHKEM(P-256, HKDF-SHA256) DeriveKeyPair → P-256 scalar → ECDSA
  recovery-auth key`. The byte-level algorithm and its rejection sampling
  are exactly RFC 9180 §7.1.3.
- **Entropy assumption.** RFC 9180 recommends that `DeriveKeyPair` input
  carry `Nsk` (= 32) bytes of entropy.
  - `RK_bytes` satisfies this: it is 256 bits from OsRng.
  - An MP-derived `PK` **does not in general**. Its security is bounded by
    the human master password.
  - Argon2id makes each guess expensive (m = 64 MiB, t = 3); it does not
    create entropy.
- **Consequence:** the MP recovery-auth key is in **the same
  password-guessing security class as `password.wrap`**. Nothing in this
  construction upgrades it to a 256-bit authentication factor, and no text
  may claim that it does.
- **Domain separation** (the HKDF `info` with class, version and
  `vault_id`) prevents cross-protocol key reuse. It does **not** increase
  entropy.
- **ECDSA use of a "KEM"-labeled derivation:** the output is a uniform
  scalar on [1, n−1]. `ikm_c` is never used in any HPKE context.
- **Vectors, kept separate:**
  1. **Primitive conformance:** RFC 9180 Appendix A.3 `ikmE → skEm/pkEm`
     and `ikmR → skRm/pkRm`, checked directly against our
     `DeriveKeyPair`.
  2. **SOURCE composition** (XV-RECOVERY-AUTH): `(PK | RK_bytes,
     auth_salt, vault_id, class) → ikm_c → sk_c/pk_c → key_id_c →
     RFC 6979 signature` over a fixed `ProviderRequest`. MP and RK cases,
     byte-exact.
- The candidate-rejection loop (`counter > 0`) is exercised by a unit test
  with an injected candidate source, since no natural vector hits it
  (probability ≈ 2⁻³² per candidate).
- **No new crypto dependency or feature flag.**

#### 4.2.3 What is stored where

| Item | Helper | Main | Provider | Backup |
|---|---|---|---|---|
| `sk_c`, `ikm_c` | transient only (derived when needed, zeroized) | never | never | never |
| `pk_c`, `auth_salt_c` | yes | may carry (public) | `state.recovery_auth[c] = {pub, salt}` | salts in header blob |
| `PK`, `RK_bytes`, MP | per existing rules (never over IPC) | never | never | never |

**Offline-guessing exposure** is unchanged from v1 and is stated as such
(S-1):
- whoever holds `pk_mp` can test MP guesses at one Argon2id evaluation
  each, exactly as against `password.wrap`, which the provider already
  stores;
- `pk_rk` gives no practical guessing avenue (256-bit RK).

`pk_c` is served only to authenticated devices (§7.3), never to anonymous
callers.

#### 4.2.4 Registration and update flows (D-11)

**Invariant [Rec]:** in every committed provider state,
`recovery_auth.mp.pub` is derived from the PK that opens that state's
`password.wrap`, and `recovery_auth.rk.pub` from the RK that opens its
`recovery.wrap`.

**Provider-side enforcement (structural, §7.4):**
- a transition that changes `header.auth_salt_c` **must** carry an update
  for class c;
- an update for class c **must** change `header.auth_salt_c`.

To make this checkable, every recovery-key change regenerates
`auth_salt_c`.

| Event | Helper actions (one staged transition) | Transition kind | Update carried |
|---|---|---|---|
| **Setup** | MP → PK; RK generated; fresh `auth_salt_mp/rk`; derive `pk_mp`, `pk_rk` | `create` | mp + rk |
| **MP change** (§1.5 `change_master_password`, both modes; scenario 5) | new PK′ (new `kdf.salt`); new `auth_salt_mp`; `pk_mp′`; new `password.wrap`; header v2 updated | `publish` (gen+1) | mp |
| **RK replacement** (scenarios 6/7; revocation per §11.4) | new RK′; new `auth_salt_rk`; `pk_rk′`; rotation | `publish` | rk |
| **Total loss via MP, RK kept** | recover; rotate; wraps re-sealed under the same PK and RK; salts unchanged | `finalize` | none |
| **Total loss via MP, RK not kept** | rotate; issue RK′ (sheet); new `auth_salt_rk`; `pk_rk′` | `finalize` | rk |
| **Total loss via RK** | rotate; user sets MP′ (required, §12 scenario 4); new `kdf.salt`, `auth_salt_mp`; `pk_mp′` | `finalize` | mp |

**Authentication of each transition:**
- `create`/`publish` transitions are device-signed;
- `finalize` is signed by the class key registered in the state being
  replaced.

**Local vs provider timing:**
- Locally, MP change and RK replacement commit first: `password.wrap` by
  atomic rename; RK via the rotation journal.
- Their publication is queued with priority.
- Until it commits, the provider keeps the *old, mutually consistent* pair
  (old wrap + old pub), so the invariant holds at every committed state.
- The security consequence of that interval is explicit in §4.2.6.

**Crash between local commit and publish:** the pending-publication flag
is persisted in the same local commit and retried (§8.3).

#### 4.2.5 Locators removed [Rec]

- In v1, a locator was both the lookup key and an implicit
  proof-of-password.
- In v2, lookup is by handle (§4.4), and the proof is the recovery
  signature.
- The `ov0/locate/*` and `ov0/locator/v1` infos, the locator routes and
  the stored locators are retired.

#### 4.2.6 Remote completion of security-relevant changes (N14)

**Operations covered:** MP change (both modes), RK replacement (scenarios
6/7), revocation (§8), enrollment, and any trusted-device RK issuance.

**Persisted operation status** (helper `kv`, written in the same local
commit as the change itself):

```text
pending_remote {
  op: mp_change | rk_replacement | revocation | enrollment,
  security_driven: bool,          # true: rk_replacement of a lost/stolen RK (scenario 7), revocation
  local_committed_at: u64,
  staged_session: id | null,      # present when the full transition is already staged (§8.3)
  attempts: u32, last_error: code }
```

| Status | Meaning | What is true remotely |
|---|---|---|
| `LOCAL_COMMITTED` | the local vault changed (wrap renamed / journal committed) | nothing yet |
| `REMOTE_UPDATE_PENDING` | a transition is staged or being retried | **the provider still holds the previous state**: old wraps + old recovery-auth public keys (+ for revocation: the revoked key still authenticates) |
| `REMOTE_COMMITTED` | the provider returned `200` for a transition containing the change (or `state_get` shows a `state_commit` that includes it) | the change is effective remotely; the old MP/RK no longer authenticates to the provider, and the new state's wraps no longer open with it |

`LOCAL_COMMITTED` → `REMOTE_UPDATE_PENDING` is immediate (the same
commit). The status leaves `REMOTE_UPDATE_PENDING` only on
`REMOTE_COMMITTED`. There is no timeout; it survives lock, restart and
crash.

**Consequence during `REMOTE_UPDATE_PENDING` (stated to the user
honestly):**
- the old MP or old RK can still authenticate a total-loss recovery at the
  provider and recover the *previous* remote state;
- for a lost/stolen RK, the old RK has **not** yet been cut off at the
  backup. Someone holding it and the public handle could complete a
  total-loss recovery, which under S-4 would revoke this Mac. The Mac
  would then learn it on its next publish (`403`), and a registry showing
  a `recovery_epoch` it did not perform is surfaced as a takeover.

**UI rules:**

| Op | While pending | Only after `REMOTE_COMMITTED` |
|---|---|---|
| MP change | "Your new master password works on this Mac. Your backup still accepts the previous password until this Mac reaches it." | "Your backup now uses the new master password." |
| RK replacement (routine) | "The new Recovery Key is active on this Mac. Your backup still accepts the previous key until this Mac reaches it." | "The previous Recovery Key no longer works." |
| RK replacement (security-driven) / revocation | **Persistent warning banner:** "Not yet cut off at your backup — the old Recovery Key / removed device still has access to your backup until this Mac connects. Keep this Mac online." | the banner clears |

The phrase "old Recovery Key no longer works" is **never** shown before
`REMOTE_COMMITTED`.

**Retry and offline behavior:**
- Security-driven pending operations get the highest queue priority,
  backoff 1 → 5 → 15 min, plus an immediate retry at launch, unlock,
  network change and manual "retry now".
- Routine ones use the normal backup queue.
- **Offline:** the status persists indefinitely. A fully staged
  transition (§8.3) can be completed while LOCKED as soon as connectivity
  returns. If the remote state moved and a merge is needed (which needs
  the VK for a new checkpoint), completion waits for the next unlock, and
  the warning stays up.
- **Provider failure** (`5xx`/`429`) counts toward
  `BACKUP_REVOCATION_FAILED` (revocation) or `BACKUP_STALE` (48 h) as
  applicable. The status stays pending.
- **Superseding:** a second change while one is pending stages a new full
  transition that includes both. The remote goes straight from the old
  state to the newest. `security_driven` is sticky.

### 4.3 Signed provider request — exact format (D-1)

**`ProviderRequest`** is canonical TLV (§4.2 rules: ascending tags,
minimal integers, re-encode check). Tag 0x0B is present only for
`state_commit`; tag 0x08 only for the device class.

| Tag | Field | Content |
|---|---|---|
| 0x01 | `proto` | u32 = 2 |
| 0x02 | `audience` | UTF-8 provider origin, lowercase `https://host[:port]`, from the helper's config (header v2 `provider`) |
| 0x03 | `vault_id` | 16 B |
| 0x04 | `operation` | u16 (table below) |
| 0x05 | `method` | UTF-8 (`GET`/`PUT`/`POST`) |
| 0x06 | `path` | UTF-8 canonical path; no query string; lowercase hex segments |
| 0x07 | `signer_class` | u8: 1 device, 2 recovery-mp, 3 recovery-rk |
| 0x08 | `signer_device_id` | 16 B |
| 0x09 | `signer_key_id` | 32 B = SHA-256(signer public key, 65 B) |
| 0x0A | `body_sha256` | 32 B, SHA-256 of the exact body (SHA-256("") if empty) |
| 0x0B | `expected_state` | 32 B `state_commit` of the state being replaced |
| 0x0C | `t` | u64 Unix seconds (helper clock) |
| 0x0D | `n` | 16 B OsRng |

**Signature:** `sig = ECDSA-P256(SHA-256("ov0/provider/request/v2" ‖
tlv))`, 64 B `r‖s`, low-S.

**HTTP header:** `Ov0-Auth: v2.<base64url(tlv)>.<base64url(sig)>`.

**Operations:**

| u16 | operation | method + path | classes | helper states |
|---|---|---|---|---|
| 1 | `state_get` | GET `/v2/vaults/{vid}/state` | device, recovery | LOCKED, UNLOCKED, SYNCING, BACKING_UP, RECOVERING |
| 2 | `blob_get` | GET `/v2/vaults/{vid}/blobs/{sha}` | device, recovery | same |
| 3 | `blob_put` | PUT `/v2/vaults/{vid}/blobs/{sha}` | device; recovery (RECOVERING only) | BACKING_UP, RECOVERING, LOCKED\* |
| 4 | `state_commit` | POST `/v2/vaults/{vid}/state` | device (`create`/`publish`); recovery (`finalize`) | BACKING_UP, RECOVERING, LOCKED\* |
| 32–47 | reserved (Phase G push) | — | — | — |

\* LOCKED only to finish a publication that was **fully staged** while
unlocked (§8.3). Staging itself needs the VK for the checkpoint.

**Helper checks before signing** (all → `SIGNING_REFUSED`, no signature):
1. state × operation × class table;
2. `blob_put` only for a SHA-256 in the requesting session's blob set;
3. `state_commit` only when `body_sha256` equals the helper's own staged
   transition body, and 0x0B equals that body's `expected_state`;
4. clock sanity (`t` ≥ header creation time, ≤ now+60 s);
5. `audience` comes from the helper's config, never from main.

Main supplies only the typed operation, typed parameters and the body
hash. It **cannot substitute a body** (0x0A), a path or method (0x05/06),
a vault (0x03), a provider (0x02) or a target state (0x0B).

**Provider verification** (any failure → no side effect):
1. parse canonical; `proto` = 2;
2. `audience` = own origin;
3. `method`/`path` = the actual request line, and `vault_id` = the path's
   `{vid}`;
4. `operation` matches the route;
5. signer resolves:
   - device: `signer_device_id` ∈ `state.active_devices` and
     `signer_key_id` = SHA-256 of its `sign_pub`;
   - recovery: `state.recovery_auth[class]` exists and the key ids match;
   - `create` is the only exception: the signer resolves against the
     genesis entry inside the transition's registry blob (§7.4);
6. low-S signature verifies;
7. `|now − t| ≤ 300`;
8. SHA-256(received body) = 0x0A;
9. for `state_commit`, 0x0B = the body's `expected_state`;
10. nonce fresh (§4.3.1).

Unknown vault, unknown key and bad signature all return the same generic
`401 AUTH_INVALID`.

#### 4.3.1 Replay cache

| Scope | Storage | Key | Behaviour |
|---|---|---|---|
| Mutating (`blob_put`, `state_commit`) | S3 create-only (`If-None-Match: *`) | `v2/nonces/{vid}/{key_id}/{n}` | exists → `409 BACKUP_REPLAY`; lifecycle expiry 2 days (> 300 s window + grace) |
| Reads | in-memory, per instance | `(vid, key_id, n)`, capacity ≥ 10 000 per key, LRU | best-effort, as §11.4 already allows; a replayed read returns ciphertext the capturer already saw |

Keying by `key_id` rather than label means a rotated recovery key starts
a fresh nonce space.

### 4.4 Recovery handle and lookup (D-9, approved)

#### 4.4.1 Normalization and privacy

**Normalization** (in `vault-proto`, shared by helper, main and provider):

```text
h1 = NFKC(s) ; h2 = trim(h1) ; h3 = NFKC(to_lowercase(h2))      # Unicode default lowercase mapping
reject unless: 3 ≤ utf8_len(h3) ≤ 128, no Unicode White_Space, no Cc/Cf/Cs/Co/Cn code points,
               and NFKC(to_lowercase(h3)) == h3 (idempotence)
handle_key = SHA-256("ov0/handle/v2" ‖ UTF-8(h3))
```

- Confusable characters are *not* folded. The handle is an identifier,
  not an authentication factor.
- Email addresses are valid handles. **SOURCE never requires an email.**
- `unicode-normalization` is already a vetted helper dependency.
- Handles are immutable in v1. Uniqueness and crash-safe claiming are in
  §7.4.1.

**Privacy, stated honestly:**
- The handle is a deliberately **public identifier**, not a secret.
- `handle_key` is an unsalted, deterministic hash of the normalized
  handle. A provider or operator who sees the claim objects can
  dictionary-test predictable handles, such as email addresses, and learn
  which ones have vaults.
- The pepper (§4.4.3) protects only the *remote enumeration behavior* for
  handles that do not exist. It does **not** make stored handles opaque to
  the provider.
- No OPRF or other privacy protocol is added in Phase F.
- The setup UI states that the handle is public and that choosing an
  email as the handle reveals that email to the backup operator.

#### 4.4.2 Locate response and KDF-downgrade protection (N12)

**Route (unauthenticated):** `POST /v2/recover/locate {handle_key}`. The
client hashes; the provider never receives the raw handle.

**Response** (always this exact shape):

```json
{ "vault_id": "hex(16 B)",
  "kdf": {"alg": "argon2id", "version": 19, "m_kib": 65536, "t": 3, "p": 1, "out_len": 32,
          "salt": "hex(16 B)"},
  "auth_salt_mp": "hex(16 B)", "auth_salt_rk": "hex(16 B)" }
```

**Client policy — enforced in the helper before any MP prompt or
derivation:**
1. Parse strictly: exact keys, no extras, lowercase hex.
   - `vault_id`, `kdf.salt`, `auth_salt_mp` and `auth_salt_rk` are
     **exactly** 16 bytes each.
2. `kdf` must equal **exactly** the frozen v1 policy (§2.3, §19 item 30):
   `alg = argon2id`, `version = 19` (0x13), `m_kib = 65536`, `t = 3`,
   `p = 1`, `out_len = 32`.
   - Weaker, stronger, unknown or malformed values → `KDF_POLICY_VIOLATION`.
     Recovery stops; nothing is derived; no MP prompt is shown.
   - The helper never substitutes or "fixes" provider-supplied
     parameters. It uses its own compiled-in constants.
3. RK recovery applies rules 1–2 to the fields it uses (`vault_id`,
   `auth_salt_rk`) and ignores `kdf` until the MP-reset step, which
   applies rule 2 to what the header says.

**After authentication — cross-check (§4.8 order, fresh device):**
- recover the VK → verify the checkpoint → trust the registry → verify
  the manifest signature → read the **committed header blob** listed in
  the verified index;
- then require `vault_id`, the `kdf` block (all fields including salt) and
  both auth salts from the locate response to **byte-equal** the committed
  header.
- A mismatch → `RECOVERY_METADATA_MISMATCH`, recovery aborted as provider
  tampering, staging deleted. It is never silently accepted.
- The same check runs whenever a device reads `locate` metadata again
  (for example, a new MP entered during RK-path recovery).

**What a malicious provider can and cannot do before the authenticated
header:**
- **Can:** deny service (garbage or refused locate responses, wrong salts
  that make authentication fail).
- **Cannot:** make the helper run a cheaper-than-policy MP derivation, and
  therefore cannot obtain a verifier that is cheaper to attack than
  `password.wrap`.
- Supported-tuple changes (§2.3 upgrade path) require a client release
  that adds the new tuple to the compiled-in allowlist. The provider can
  never introduce one.

#### 4.4.3 Unknown handles, rate limits, the sheet

- **Unknown handle:** the same response shape.
  - `vault_id` and salts are `HMAC-SHA256(pepper, label ‖ handle_key)`
    truncated per field.
  - `kdf` is the frozen policy tuple, so it passes the client policy.
  - Timing is equalized best-effort (same S3 reads).
  - The next step, a signed recovery request, fails with the same generic
    `401` as a wrong MP/RK.
- **`pepper`:** a 32 B provider-held secret, **enumeration mitigation
  only, no vault authority**. Leaking it re-enables enumeration of
  nonexistent handles and nothing else.
- **Lookup limits:** per source IP and per `handle_key`, in memory per
  instance. Tunable; lookups reveal only public metadata.
- **Recovery-authentication limits:** provider-wide (§4.5).
- **Recovery Key sheet (§1.7):** adds the normalized handle and the
  provider origin to `vault_id`, generation and head prefix.
- **MP-only recovery** needs the handle (a non-secret identifier) plus the
  MP. No second secret is introduced.

**[Closed S-5 — approved with stronger rate limits]:** the pepper and fake
responses as above; the shared throttle in §4.5.

### 4.5 Provider-wide recovery-authentication throttle (S-5)

**Requirement:** adding provider instances must not multiply the allowed
MP/RK guessing rate. Per-instance memory is not sufficient for recovery
authentication.

**Design:** S3 slot reservation, strict under concurrency.

```text
window  = floor(now / W)                           # W = 3600 s (tunable)
prefix  = v2/ratelimit/{vid}/recovery/{window}/
slots   = k ∈ [0, L)                               # L = 10 (tunable)
```

**For every recovery-class request (MP or RK), before verifying its
signature:**
1. `LIST prefix`. If all L slots exist → `429 RECOVERY_THROTTLED`,
   without verifying anything.
2. Reserve the lowest free slot k: `PUT prefix/k` with `If-None-Match: *`,
   body `{reserved_at, key_id_prefix}`.
   - `412` → try k+1.
   - No free slot → `429`.
3. Verify the request (§4.3).
   - **Success** → `DELETE prefix/k` (release). The request proceeds.
   - **Failure** → the slot **stays**, counting as one failed guess.
     Response: generic `401`.

**Properties:**
- **Strict across instances:** at most L signature verifications can
  *fail* per vault per window, however many instances run.
  - Every verification holds a slot while it runs.
  - Slots are claimed with create-only semantics, so two instances never
    share one.
  - A failed verification never releases its slot.
- **Legitimate recovery** (hundreds of successful requests) holds a slot
  only during each verification, so it consumes nothing. Its concurrency
  is bounded by the free slots.
- **An attacker who exhausts the window** blocks recovery for that vault
  for ≤ W. Denial of service is inherent to any throttle, and the client
  shows "too many recovery attempts, try again later".
- **Crash between reserve and release:** the slot leaks for that window
  only. That is conservative: it counts as a failure.
- **Boundary effect:** fixed windows allow ≤ 2L failures across a
  boundary. That is acceptable and tunable. A two-window sum can tighten
  it later.
- **Cleanup:** S3 lifecycle expires `v2/ratelimit/` after 2 days.
- **Costs:** LIST + PUT + DELETE per recovery-class request. Recovery is
  rare; device-class traffic is not throttled here.
- **Scope:** recovery-class requests naming a nonexistent vault get the
  generic `401` and consume only the in-memory per-IP limits. There is
  nothing to guess.
- **Trait:** `OpsStore` (`create_if_absent`, `delete`, `list_prefix`),
  shared with handle claims and nonces. `FsStores` implements it with
  `O_EXCL`.
- **Test ST-RL-01** (§14) runs two `ProviderCore` instances over one
  `FsStores` and proves the combined failure count never exceeds L.

**Read-replay cache:** stays in memory and best-effort (§4.3.1).
**Mutating-request nonces:** stay durable (S3 create-only).

---

## 5. Provider / backend (D-2, approved)

**Vendor facts [Fact]** (re-verified in revision 1):

- **S3 `If-None-Match: *`:** create-only; the first concurrent writer
  wins, and later ones get `412`.
- **S3 `If-Match: <ETag>`:** fails with `412` on mismatch; `409` is
  possible under concurrency.
- **S3 durability:** designed for 99.999999999%, across ≥ 3 AZs.
- Sources: [S3 conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html),
  [S3 durability](https://docs.aws.amazon.com/AmazonS3/latest/userguide/DataDurability.html).

**Architecture** (hosting not chosen):

- One stateless container, 0.25–0.5 vCPU and 256–512 MiB.
- Horizontal scaling is safe for mutations and for recovery throttling,
  because CAS, nonces, handle claims and rate-limit slots are all in S3.
  The read-replay cache and the lookup limits are per-instance
  best-effort.
- One S3 General Purpose bucket:
  - versioning on;
  - Block Public Access;
  - lifecycle: `v2/nonces/` and `v2/ratelimit/` 2 days; abort incomplete
    multipart uploads after 1 day; non-current versions 30 days;
  - IAM scoped to the bucket.
- Secrets: the lookup `pepper` (§4.4) and nothing vault-related.
- Logs: route, status, `vid`, `key_id` only; never bodies.
- **ETags** are concurrency/version tokens only (owner requirement).
  Integrity comes from SHA-256 names and the state commitment (§7.2).
- S3 client: a lean SigV4 client with conditional-header support,
  vetted under a separate provider supply-chain scope. The helper's
  dependency count is unaffected.

---

## 6. Process boundaries, ops and states

### 6.1 Ownership

| Responsibility | Helper | Main | Provider |
|---|---|---|---|
| Build, seal, index, sign manifest, MAC checkpoint | **yes** | no | — |
| Canonical `ProviderRequest` + signature | **yes** | never supplies path, method, audience, `t` or `n` | verifies |
| Transition bodies | **builds** | transports verbatim | validates structurally |
| HTTP/TLS, retry, queue | no | **yes** | — |
| Verify downloaded state (signatures, chain, checkpoint, rollback/fork, AEAD) | **root of trust** | no | defense in depth |
| CAS, replay, GC, handle claims | — | — | **yes** |

Main transports ciphertext and public data only. It never sees plaintext
vault records, `PK`/`RK`/MP, `sk_c`, device private keys, or any reusable
authentication secret. None exists (§4).

### 6.2 Helper ops (new §1.5 rows)

| op | Allowed in | Effect |
|---|---|---|
| `backup_prepare` | UNLOCKED | → BACKING_UP; stages a `publish` transition; → `{session, expected_state, new_state, blob_count, body_sha256}` |
| `backup_blob_list {session, page}` | BACKING_UP, RECOVERING, LOCKED (staged) | ≤ 400 `{sha256, size, role}` per page |
| `stream_read {session, sha256, offset}` | same | outbound chunk (§9) |
| `backup_transition_body {session}` | same | the staged `StateTransition` bytes (≤ 8 KiB) |
| `backup_commit_result {session, outcome, state?}` | same | committed → persist last-seen; conflict → → SYNCING path; failed → retry policy |
| `backup_state_offer {state}` | UNLOCKED (→ SYNCING), RECOVERING | verify signature, rollback and fork; → `{session, need pages}` |
| `stream_begin / stream_write / stream_end / stream_cancel` | SYNCING, RECOVERING | inbound (§9) |
| `backup_apply {session}` | SYNCING → UNLOCKED | §10 merge + envelope refresh; counts only |
| `recovery_begin {kind, locate_response}` | UNINITIALIZED, LOCKED → RECOVERING | panel collects MP/RK; derives `sk_c`; → `{session}` |
| `recovery_preview` | RECOVERING | FR-01 data |
| `recovery_complete {session}` | RECOVERING (re-encryption phase) | epoch, re-encrypt, stage `finalize` |
| `enroll_confirm` (changed) | UNLOCKED | returns an enroll-bundle **session** instead of one frame |
| `sign_provider_request {session?, operation, params, body_sha256}` | per §4.3 | replaces `sign_backup_request` |
| `session_close {session}` | any | abort + delete staging |
| `resolve_conflict {ref, chosen, edits?}` | UNLOCKED + presence | §10.3 |
| `quarantine_status` | UNLOCKED | counts only (§10.6) |

`backup_snapshot_prepare`/`backup_state_apply` are superseded.

### 6.3 States and transitions (added to `VaultState`)

| State | Entered from → by | Leaves to | VK | On lock / auto-lock | On crash / restart | Timeout |
|---|---|---|---|---|---|---|
| **BACKING_UP** | UNLOCKED → `backup_prepare`; LOCKED → coordinator resumes a fully staged publication | UNLOCKED/LOCKED (whichever it came from) on commit, fail, abort | resident only while building; not needed once staged | building: abort (staging deleted). Staged: continues without VK | staging swept unless marked `pending_publication` (§8.3); then re-staged or re-sent | session idle 10 min |
| **SYNCING** | UNLOCKED → `backup_state_offer` | UNLOCKED | resident | abort; staging deleted | LOCKED; nothing applied (apply is one DB transaction) | 10 min idle |
| **ROTATING_KEYS** (internal, O-2: reported via a `rotation_progress` status event, not presented as a vault state) | UNLOCKED → `rotate_recovery_key`, `revoke_device` | UNLOCKED | old+new, old zeroized at flip | **deferred** until the journal commit or roll-back completes (no half state) | journal roll-forward/back at open (existing) | none (resumable) |
| **RECOVERING** | UNINITIALIZED/LOCKED → `recovery_begin` | UNLOCKED (finalize committed); UNINITIALIZED/LOCKED (abort) | per §13.2 | abort before re-encryption; after it, continue to finalize or abort | restart from re-verification of downloaded state with a new fresh VK (§13.3) | 120 s for entry phases; none during re-encryption |
| **COMPROMISED** | any → registry fork (§4.6), equivocation *of the registry*, or confirmed vault-level tamper | LOCKED via lock; exit only by an owner-specified resolution (§16 U-9) | unchanged | LOCKED (COMPROMISED flag persists in `kv`) | re-entered at open | — |

**New error codes (§15):**

| Code | Meaning / handling |
|---|---|
| `SIGNING_REFUSED` | helper refused to sign |
| `BACKUP_REPLAY` | nonce reused |
| `BACKUP_REVOCATION_FAILED` | revocation not yet published after 3 attempts |
| `BACKUP_STALE` | no successful publication for 48 h |
| `KEYCHAIN_UNAVAILABLE` | queue/retry |
| `TRANSFER_INVALID` / `TRANSFER_ABORTED` | IPC stream failures |
| `HANDLE_TAKEN` | handle claimed by another vault |
| `RECOVERY_AUTH_STALE` | provider 422, §7.4 |
| `COUNTER_REGRESSION` | tamper evidence, not user-facing |
| `REVOKED_AUTHOR_REFUSED` | count only, §10.6 |
| `STATE_MOVED` | provider 409, client merges; replaces v1's use of `BACKUP_CONFLICT` for the first attempts |
| `KDF_POLICY_VIOLATION` | locate metadata outside the frozen KDF policy; no MP prompt (§4.4.2) |
| `RECOVERY_METADATA_MISMATCH` | locate metadata ≠ committed header (§4.4.2) |
| `RECOVERY_THROTTLED` | provider 429 from the shared throttle (§4.5) |

**Remote-completion status** (§4.2.6): `LOCAL_COMMITTED` /
`REMOTE_UPDATE_PENDING` / `REMOTE_COMMITTED` is an *operation status*
persisted in `kv` and reported by a `remote_update` event. It is not a
`VaultState`, because it outlives lock and restart and coexists with
every state.

`BACKUP_CONFLICT` keeps its meaning: surfaced after 3 failed merge
attempts.

---

## 7. Object, index, manifest and vault-state model

### 7.1 Storage layout (S3 keys or `FsStores` paths)

```text
# vault data (what clients verify)
v2/vaults/{vid}/state                     the only mutable vault object; If-Match CAS (create: If-None-Match: *)
v2/vaults/{vid}/blobs/{sha256}            immutable; If-None-Match: *; name = SHA-256(bytes), verified by provider
# provider operational state (never a vault root of trust)
v2/handles/{handle_key}                   claim {vault_id, claim_id, created_at, status}; create If-None-Match: *,
                                          transitions only by If-Match (§7.4.1)
v2/nonces/{vid}/{key_id}/{n}              create-only; lifecycle 2 days
v2/ratelimit/{vid}/recovery/{window}/{k}  create-only reservation / delete on success; lifecycle 2 days (§4.5)
```

**The overwrite race (N1) is impossible:**
- every blob is written once under its content hash, and a second write of
  the same name is rejected;
- same-hash blob content is identical by construction;
- `state` and handle claims change only by CAS on their ETag;
- nonce and rate-limit objects are create-only (plus the throttle's
  release-delete).

`FsStores` emulates this with `O_EXCL` creates and a
version-counter-plus-rename CAS.

**Operational state is not a root of trust.** Handle claims, nonce
objects, rate-limit slots, GC bookkeeping, `recent`/`finalized` and the
derived fields of `state` exist for availability, idempotency and abuse
control. A malicious or buggy provider can lie about them: refuse
service, report a handle as taken, throttle, replay a stored result, GC
too early, or serve fake locate data. It **cannot** produce a vault state
a client accepts. Clients accept only what verifies:
- manifest signature under a registry they have anchored (§4.4 rules and
  rollback floor, or the §4.8 checkpoint on a fresh device);
- registry chain;
- checkpoint MAC under the VK they hold;
- index and blob hashes;
- AEAD.

### 7.2 Vault-state object and state commitment

**Provider-internal JSON** (`v: 2`):

```json
{ "v": 2, "vault_id": "…",
  "generation": 7, "manifest_hash": "…", "checkpoint_hash": "…", "index_hash": "…",
  "registry_hash": "…", "registry_head": "…", "registry_seq": 12, "epoch": 0,
  "header_hash": "…", "vk_generation": 3,
  "active_devices": [{"device_id": "…", "sign_pub": "…"}],
  "recovery_auth": {"mp": {"pub": "…", "salt": "…"}, "rk": {"pub": "…", "salt": "…"}},
  "locate": {"kdf": {"alg":"argon2id","version":19,"m_kib":65536,"t":3,"p":1,"out_len":32,"salt":"…"},
             "auth_salt_mp":"…","auth_salt_rk":"…"},
  "handle_key": "…", "claim_id": "…",
  "state_commit": "…",
  "retained": [{"generation":6,"manifest_hash":"…","checkpoint_hash":"…","index_hash":"…"},
               {"generation":5, "…": "…"}],
  "recent": [{"body_sha256":"…","result":{"generation":7,"state_commit":"…"}}],
  "finalized": {"<expected generation>": "<body_sha256>"} }
```

- `active_devices`, `locate` and `vk_generation` are **derived** at commit
  from the registry and header blobs. They are never taken from the
  request.

**Client-visible CAS token:**

```text
recovery_auth_digest = SHA-256("ov0/recovery-auth-set/v2" ‖ for c in [mp, rk] if present:
                                u8(class) ‖ pub(65) ‖ salt(16))
state_commit = SHA-256("ov0/vault-state/v2" ‖ TLV{
    0x01 vault_id(16) 0x02 generation(u64) 0x03 manifest_hash(32)
    0x04 checkpoint_hash(32) 0x05 recovery_auth_digest(32) })
```

- The helper computes `state_commit` for both the state it expects and the
  state it proposes.
- Every field is either verified by the helper (manifest, checkpoint) or
  public (`recovery_auth`), so a provider cannot swap state behind an
  unchanged token.
- **The ETag never appears on the wire to clients.**

### 7.3 Routes

| Route | Auth | Returns / accepts |
|---|---|---|
| `GET /v2/vaults/{vid}/state` | device, recovery | `{generation, state_commit, manifest(b64), checkpoint(b64), recovery_auth[] (device class only), vk_generation}` |
| `GET /v2/vaults/{vid}/blobs/{sha}` | device, recovery | bytes |
| `PUT /v2/vaults/{vid}/blobs/{sha}` | device; recovery | creates. The provider hashes the stream and compares to `{sha}` → `422` on mismatch; exists → `200` (idempotent). Size caps: index ≤ 8 MiB, registry ≤ 4 MiB, others ≤ 1 MiB |
| `POST /v2/vaults/{vid}/state` | per transition kind | `StateTransition` (§7.4) |
| `POST /v2/recover/locate` | none | §4.4 |
| *(module reserved)* `/v2/push/*` | — | Phase G only (D-8) |

There is **no delete route** (D-4). GC is §7.6.

### 7.4 `StateTransition` — the atomic vault-state CAS

**Body:** canonical TLV.

| Tag | Field |
|---|---|
| 0x01 | `proto` u32 = 2 |
| 0x02 | `vault_id` 16 B |
| 0x03 | `kind` u8: 1 `create`, 2 `publish`, 3 `finalize` |
| 0x04 | `expected_state` 32 B (all-zero iff `create`) |
| 0x05 | `manifest` bytes (manifest v2) |
| 0x06 | `checkpoint` bytes (§4.8 format, unchanged) |
| 0x07 | `recovery_auth_updates` bytes (optional): concatenation, sorted by class, of `u8 class ‖ pub(65) ‖ salt(16)` |
| 0x08 | `handle_key` 32 B (`create` only) |

**Provider algorithm.** Any failure means no mutation. Reads use S3 GET,
and the ETag is kept.

1. **Authenticate** (§4.3) and check the kind/class pairing:
   - `create` → device signer = the genesis device of the supplied
     registry;
   - `publish` → device signer ∈ current `active_devices`;
   - `finalize` → the recovery class registered in current state.
2. **Idempotency:** if SHA-256(body) ∈ `recent`, return the stored result
   (`200`). For `create`, the idempotent path is §7.4.1 (C3/C4), which also
   completes a binding a crash interrupted. Step 2 never short-circuits a
   `create`.
3. **Precondition:**
   - `create` → no state exists (`If-None-Match: *` at commit);
   - otherwise `expected_state` = `state.state_commit`, else
     `409 STATE_MOVED {state_commit, generation}`.
4. **Manifest v2:**
   - `vault_id` matches;
   - `generation` = current + 1 (`create`: 1);
   - `prev_manifest_hash` = current `manifest_hash` (`create`: zeros);
   - `signer_device_id`:
     - `publish`: = the request signer;
     - `finalize`: = the device installed by the appended recovery_epoch;
     - `create`: = genesis.
5. **Index v2** (blob named by `manifest.object_index_hash`, which exists):
   - it parses; its generation matches;
   - every listed blob exists with the listed size;
   - roles: exactly one `header`, `registry` and `wrap mp`; at most one
     `wrap rk`;
   - `env` ids = exactly the active devices of the *new* registry;
   - every `rev` line's parents are listed in the same index (ancestor
     closure).
6. **Registry blob:**
   - it parses, and the current registry is an exact prefix by entry hash
     (`create`: the registry is exactly one valid genesis);
   - it verifies under `verify_chain_with(CheckpointAnchored)`;
   - the new head = `manifest.registry_head`.
   - **Allowed appended entries by kind:**
     - `publish`: `enroll`/`revoke` only, each signed by a device active
       at its seq. If any `revoke` is appended: `manifest.vk_generation`
       = current + 1 (mandatory rotation), and no `env` for the revoked
       id.
     - `finalize` (**S-4, approved**):
       - exactly one `recovery_epoch` (epoch = current + 1);
       - **immediately followed by** one `revoke` per device in the
         *current* state's `active_devices`, in ascending `device_id`
         order, each with `authorizer` = the epoch's newly installed
         device and signed by its `sign_pub`;
       - no other entries.
       - The set of revoked ids must **equal** the prior active set.
         Missing or extra → `422 REGISTRY_INVALID`.
       - The resulting `active_devices` is exactly {the new device}.
       - `manifest.vk_generation` = current + 1, the index carries no
         `env` for any prior device, and the replay rule
         `finalized[expected generation]` applies (§11.8 semantics).
7. **Header blob:** it parses as header v2; `vault_id` matches; `locate` is
   taken from it.
   - **D-11 rule:** for each class c,
     `header.auth_salt_c ≠ current.locate.auth_salt_c` ⇔ an update for c
     is present, and the update's salt = the header salt. Otherwise
     `422 RECOVERY_AUTH_STALE`.
   - `create` requires updates for both classes.
   - The header's `kdf` block must equal the frozen policy tuple
     (§4.4.2). Otherwise `422 MANIFEST_INVALID`. This is defense in depth:
     the client enforces it independently.
8. **Signatures and bindings:**
   - the manifest signature verifies under the signer's `sign_pub` (for
     `finalize`, the recovery_epoch entry's `sign_pub`);
   - the checkpoint binding matches structurally: `vault_id`, registry
     head, `manifest_core_hash`, generation, `vk_generation`, epoch.
9. **Commit:**
   - put the checkpoint blob (`If-None-Match: *`; an identical existing
     blob is fine);
   - build the new state: derived fields; `retained ← [old current, old
     retained[0]]`; `recent` (last 3); `finalized`; new `state_commit`;
   - `create`: run the claim protocol of §7.4.1 (claim → create state →
     bind);
   - otherwise PUT state with `If-Match: <ETag from step 3's read>`.
     On `412`/`409`, reload and go to step 2: an idempotent hit returns
     `200`; a moved state returns `409 STATE_MOVED`.
10. **Respond:** `200 {generation, state_commit}`.

**Errors:**

| Status | Code | Client handling |
|---|---|---|
| 401 | `AUTH_INVALID` | generic |
| 403 | `DEVICE_NOT_AUTHORIZED` | never interpreted as revocation |
| 409 | `BACKUP_REPLAY` | — |
| 409 | `STATE_MOVED` | merge, re-stage (≤ 3), then `BACKUP_CONFLICT` |
| 412 | `BLOB_MISSING {count}` | re-upload, retry |
| 409 | `HANDLE_TAKEN` | — |
| 413 | — | too large |
| 422 | `MANIFEST_INVALID` / `INDEX_INVALID` / `REGISTRY_INVALID` / `CHECKPOINT_MISMATCH` / `RECOVERY_AUTH_STALE` / `FINALIZE_CONFLICT` | — |
| 429 | `RECOVERY_THROTTLED` / generic | shared recovery throttle (§4.5) / other limits |
| 503 | — | `BACKUP_UNAVAILABLE` |

#### 7.4.1 `create`: handle claim and state creation without a cross-key transaction (N13)

**Claim object** `v2/handles/{handle_key}` (JSON):
`{vault_id, claim_id (16 B OsRng, chosen by the provider), created_at,
status: "pending" | "bound"}`.

`state` records `claim_id` and `handle_key`.

**Liveness:** a claim is **live** iff
- `status = bound`; or
- `status = pending` and `now − created_at < G` (grace, 24 h); or
- `status = pending` and `state(claim.vault_id)` exists with
  `state.claim_id = claim.claim_id`, i.e. a create crashed after the state
  was written but before binding. A created vault always keeps its handle.

**Lookup (§4.4)** resolves a handle only if `status = bound`, the state
exists, and `state.claim_id = claim.claim_id` and `state.handle_key` match.
Anything else is answered with the fake response.

**Create algorithm** (after steps 1–8 validated the body; the signer is
the genesis key):

| Step | Action | Outcomes |
|---|---|---|
| C1 | GET claim | absent → C2. Present, same `vault_id` (our retry) → C3 with that `claim_id`. Present, other vault: **live** → `409 HANDLE_TAKEN`; **not live** (stale pending, no state) → C2′ |
| C2 | PUT claim `{vault_id, new claim_id, now, pending}` with `If-None-Match: *` | `200` → C3. `412` (another create won the race) → back to C1 |
| C2′ | reclaim: PUT the same body with `If-Match: <stale claim ETag>` | `200` → C3. `412` (someone changed it) → back to C1 |
| C3 | PUT state (with `claim_id`, `handle_key`) using `If-None-Match: *` | `200` → C4. `412` → state exists: if `state.claim_id = claim_id` and SHA-256(body) ∈ `state.recent` → C4 (our retry); otherwise `409` (vault_id collision; practically impossible) |
| C4 | bind: PUT claim `{…, status: bound}` with `If-Match: <claim ETag read in C1/C2>` | `200` → respond `200`. `412` → re-GET: already `bound` to us → `200`; now another vault's (it reclaimed us) → **roll back**: DELETE our state with `If-Match: <its ETag>` (only a generation-1 state whose `claim_id` lost its claim), then `409 HANDLE_TAKEN` |

**Crash and concurrency cases:**

| Case | Result |
|---|---|
| Crash after C2 (claim pending, no state) | Our retry → C1 finds our claim → C3 → C4. If we never retry, the claim is not live after G, and another vault may reclaim it (C2′). The handle is **never permanently burned** |
| Crash after C3 (state exists, claim pending) | Our retry → C1 (ours) → C3 `412` + `recent` hit → C4 → `200`. Others see the claim as live (third liveness clause), so there is no theft |
| Crash after C4, before the response | Retry → C1 finds `bound`/ours → C3 `412` + `recent` hit → C4 `412` then re-GET (bound to us) → `200`. **Idempotent** |
| Two concurrent creates for one handle | Exactly one C2 `If-None-Match` succeeds. The other re-reads → a live pending claim of another vault → `409 HANDLE_TAKEN` |
| Reclaim racing the original owner's late retry | Both write the claim under `If-Match` of the same ETag, so exactly one wins. If the reclaimer wins, the owner's C4 fails; the owner rolls back its (unbound, generation-1) state and gets `409`. The client asks for a new handle. No vault data is lost, because nothing was ever published beyond generation 1 of a vault that had not completed setup. If the owner wins, the reclaimer sees `bound` → `409` |
| Lost `409` after rollback | The client's retry → C1 sees another vault's live claim → `409` again |

**The client side** (setup, §12): the helper keeps the staged `create`
until `200`. On `409 HANDLE_TAKEN` it asks the user for another handle and
re-stages (same `vault_id`, new header only if needed). Local setup is not
considered backed up until `REMOTE_COMMITTED` (§4.2.6).

State rollback in C4 is a provider-internal compensation for a failed
`create`. It is not a client delete capability (D-4).

### 7.5 Index v2 and manifest v2

```text
#ov0-index v2
#generation <u64>
#items <u64>
<role> <logical-id> <blob-sha256-hex> <size> [<parents>]      # lines sorted bytewise
  header   -
  registry -
  wrap     mp|rk
  env      <device_id hex32>
  rev      <record_id hex32>/<revision_id hex64>   <parent revision_ids, comma-separated, sorted, or "-">
```

- The index bytes **are** the hashed bytes.
- Manifest v2 keeps the v1 TLV fields, with `version = 2`. Field 0x07
  `object_index_hash` = SHA-256(index bytes), which is also the index's
  blob address.
- Parents appear in the index so the provider can check ancestor closure
  without decrypting. They are the same plaintext-classified graph
  metadata as today (§3.4).

### 7.6 GC (D-4)

**Reachable set:**
- the current plus the two `retained` states: their manifest, checkpoint
  and index blobs;
- every blob those indexes list;
- blobs younger than 7 days.

**Rules:**
- Everything else is deleted by the provider's scheduled GC. No client
  path deletes.
- A commit that races GC (a blob deleted after upload) fails with
  `412 BLOB_MISSING`. The client re-uploads (the create now succeeds) and
  retries. This is a liveness cost only.

### 7.7 Every format and vector that changes (D-3)

| # | Artifact | Change | Vectors / fixtures |
|---|---|---|---|
| 1 | Record object | `OV0OBJ01` → **`OV0OBJ02`** (§10.2 layout) | new XV-OBJ |
| 2 | Record/meta AEAD AAD | v2 binds `revision_id` + graph digest (§10.2) | CR-05/06 fixtures; new XV-RECORD-AAD |
| 3 | `rev_hash` | **removed** (replaced by `revision_id` + `blob_hash`) | old RG/SY fixtures regenerated |
| 4 | SQLite schema | `record_revs` keyed by `revision_id`; new `quarantined_revs`, `pending_revs`, `record_flags`, `author_hwm`; `user_version` 2 | schema tests |
| 5 | Object index | JSON → v2 text | new XV-INDEX |
| 6 | Manifest | version 1 → **2** | XV-TLV manifest entries |
| 7 | Checkpoint | format unchanged; values change | fixtures only |
| 8 | Header JSON | v2: `auth_salt_mp/rk` replace `locator_salt_*`; add `provider`; `kdf` block in the exact §4.4.2 shape (`alg`, `version`, `m_kib`, `t`, `p`, `out_len`, `salt`) | SC-03 lint updated; KD-01…03 fixtures |
| 9 | Device envelope payload | **v2 without `device_backup_cred`** (D-13, §11.1) | XV-ENROLL (Rust + Swift) |
| 10 | HPKE envelope `info` | `ov0/envelope/v1` → **`ov0/envelope/v2`** | XV-ENROLL; XV-HPKE-SE unchanged (suite vectors) |
| 11 | `creds.bin`, `ov0/device-creds/v1` | **removed** | CR-12 rewritten |
| 12 | Rotation commit marker | `stage` array no longer lists `creds.bin` | rotation crash-matrix fixtures |
| 13 | Request auth | HMAC v1 → `ProviderRequest` v2 signature | new XV-REQSIG (recovery byte-exact; device verify-only) |
| 14 | Recovery auth keys | new (§4.2) | new XV-RECOVERY-AUTH + RFC 9180 A.3 DKP conformance + RFC 6979 conformance |
| 15 | `StateTransition`, `state_commit` | new | new XV-STATE |
| 16 | Finalize body v1 (`FinalizeBody`) | **retired** → `StateTransition kind=3` | RF fixtures regenerated |
| 17 | Handle normalization | new | new XV-HANDLE (NFKC, case, rejects) |
| 18 | §2.9 infos | add `ov0/provider-recovery-auth/{mp,rk}/v2`, `ov0/provider/request/v2`, `ov0/vault-state/v2`, `ov0/recovery-auth-set/v2`, `ov0/handle/v2`, `ov0/record/v2`, `ov0/meta/v2`, `ov0/rev-graph/v2`, `ov0/envelope/v2`. Retire `ov0/backup-auth/*/v1`, `ov0/locate/*`, `ov0/locator/v1`, `ov0/device-creds/v1`, `ov0/rev/v1` | XV-HKDF |
| 19 | Enrollment bundle JSON | `objects` entries become `[role, logical_id, sha256, data]`; `provider` added | iPhone parser (`VaultEnrollment.swift:41,178`) |
| 20 | Recovery sheet | adds handle + provider origin | FR-02 fixtures |
| 21 | XV-RECOVERY-EPOCH | format unchanged; binds manifest v2 hashes | values regenerated |

**Unchanged:** registry entry TLV (unless S-4 changes rules; the encoding
is unaffected either way), wrap files and their AAD, XV-ECDSA,
XV-HPKE-SE, XV-SAS, XV-BIP39, XV-ORIGIN.

**No migration** of synthetic vaults (D-3).

---

## 8. Revocation transaction

### 8.1 One transition (resolves C4)

- Revocation = the local journaled commit (existing Phase E: presence →
  MP → new-RK sheet → `revoke` entry + rotation + surviving envelopes),
  then one `publish` transition.
- That transition carries the new registry, the rotated state, the
  survivors' envelopes and an **rk recovery-auth update** (new RK, D-11).
- The provider verifies the `revoke` entry against the registry *in the
  same request* (§7.4 step 6), enforces the rotation and the missing
  envelope, and recomputes `active_devices`.
- The revoked key's authentication ends at the same CAS that makes the
  revocation current. There is no separate endpoint.

### 8.2 The irreducible interval

- Between the local commit and the provider commit, the provider cannot
  know about the revocation.
- The target can:
  - read ciphertext it could already decrypt;
  - commit a racing `publish` of its own. That forces the revoker to
    merge and retry; its unseen revisions are refused (§10.6).
- The target cannot delete (D-4).
- A registry entry it appends at the same seq is a fork → COMPROMISED
  (§4.6).

### 8.3 Crash, retry, idempotency

**Staging:**
- The local journal commit also writes `pending_publication{reason:
  revocation, target, staged: session}`.
- The staged transition (blobs + body, VK-derived checkpoint included) is
  built **before** the helper may lock, so publication can finish while
  LOCKED (§6.3).
- If the state moved and a merge is needed (which needs the VK for a new
  checkpoint), publication waits for the next unlock. The UI shows "not
  yet cut off at the backup".

| Crash point | Recovery |
|---|---|
| before journal commit | nothing happened |
| after commit, before upload | pending flag → resume at launch (priority) |
| mid-upload | idempotent re-upload |
| after CAS, before the response | the retry hits `recent` (`200`), or `state_get` shows the helper's own `state_commit` → committed |
| 3 failures | `BACKUP_REVOCATION_FAILED` surfaced; backoff 1 → 5 → 15 min continues |

**Enrollment** uses the same path (`pending_publication{reason:
enrollment}`). The new device authenticates at the provider from that
commit.

---

## 9. IPC transfer design (chunked ciphertext streams)

**Budget:**
- Raw chunk ≤ **24 KiB** (24 576 B), which is 32 768 B base64.
- With the JSON envelope (≤ 1 KiB), a frame is ≈ 33 KiB, about 52% of
  the 64 KiB cap.

**Outbound (helper → main), pull-based:**
- `stream_read {session, sha256, offset}` → `{data, offset, total_len,
  eof}`.
- Main issues the next read only when ready (natural backpressure).
- The declared `size` and `sha256` come from `backup_blob_list`. Main
  verifies SHA-256 before PUT, and the provider re-verifies.
- Only blobs in that session's set are readable.

**Inbound (main → helper), acknowledged:**
1. `stream_begin {session, sha256, size}` → `{stream_id}`. Allowed only
   if `sha256` ∈ the session's need list, `size` ≤ the role cap, and the
   session budget is not exceeded.
2. `stream_write {stream_id, seq, offset, data}`. Strictly contiguous:
   `seq` = previous + 1 and `offset` = bytes received so far. Each write
   is acknowledged; there is one outstanding write per stream, and ≤ 4
   open streams per session.
3. `stream_end {stream_id}`. The helper checks the total length = `size`
   and the incremental SHA-256 = `sha256`, then atomically renames into
   the session store.
   - Any violation → `TRANSFER_INVALID`, the partial file is deleted, and
     the stream is closed.

**Identifiers:**
- `session` and `stream_id` are 128-bit OsRng values.
- They are bound to the opening connection. A different connection
  presenting them gets `TRANSFER_INVALID`.

**Caps and cleanup:**

| Limit | Value |
|---|---|
| Chunk | ≤ 24 KiB |
| Blob, by role | 1 MiB; registry 4 MiB; index 8 MiB |
| Session total | 512 MiB |
| Idle stream | 60 s |
| Idle session | 10 min |
| Staging | `vault/staging/{session}/`, 0700, helper-owned |

- **Cancellation:** `stream_cancel`, `session_close`, connection drop,
  lock (except fully staged publications, §8.3), TTL expiry, and helper
  start (sweeps unmarked sessions).

**Memory:**
- The helper streams to disk with incremental hashing.
- Main holds ≤ one chunk per inbound stream and ≤ one blob per in-flight
  PUT, which it can stream from successive reads.

**Enrollment bundle:**
- `enroll_confirm` returns a session. Main pulls blobs with the same
  `stream_read` and assembles the phone's bundle response.
- That removes C6's one-frame bundle without a second protocol.

**Content:** ciphertext (records, sealed wraps, sealed envelopes) and
public data (registry, index, manifest, checkpoint, header).
- §1.5's never-list exception must name sealed wraps and envelopes in
  backup sessions.
- There is no new exposure: main is same-UID with the 0600 files, and the
  provider already holds them.

---

## 10. Revision / sync design (D-5)

### 10.1 Authorship

- New revisions carry this device's registry `device_id`.
- The all-zero id is invalid in v2 (a clean break; no migration).
- `author_hwm(record_id)` in `kv` stores the highest counter this device
  has ever authored for the record. It never decreases.
- `next_counter = max(author_hwm, max counter of own revisions held) +
  1`.

### 10.2 Stable logical identity (replaces `rev_hash`)

- **`revision_id`:** 32 B from OsRng, created once when the revision is
  authored. Parents are `revision_id`s.
- **`blob_hash`:** SHA-256 of the serialized encrypted object. It is used
  only for storage addressing and in the index.

**Object `OV0OBJ02`:**

```text
magic "OV0OBJ02" | kind u8 | flags u8 (bit0 tombstone) | vk_generation u32 | counter u64
| author 16 | record_id 16 | revision_id 32 | parent_count u8 (≤8) | parents 32×n (sorted)
| nonce 24 | ct_len u32 | ct | meta_nonce 24 | meta_ct_len u32 | meta_ct
| trailer: schema_version u32, created_at u64, updated_at u64      (no trailing bytes; ≤ 1 MiB)
```

**Integrity binding:**

```text
graph_digest = SHA-256("ov0/rev-graph/v2" ‖ record_id ‖ revision_id ‖ author ‖ u64be(counter)
                       ‖ u8(flags) ‖ u8(kind) ‖ u8(n) ‖ parents(sorted))
record_aad   = "ov0/record/v2" ‖ vault_id ‖ record_id ‖ revision_id ‖ u32be(schema) ‖ u32be(vk_generation) ‖ graph_digest
meta_aad     = "ov0/meta/v2"   ‖ vault_id ‖ record_id ‖ revision_id ‖ field_tag ‖ graph_digest
```

| Threat | What stops it |
|---|---|
| Provider or any party without the VK altering, swapping or re-parenting an object | the blob is named by its hash; the index authenticates `revision_id → blob_hash` and the parents; the manifest signs the index; the checkpoint MACs the manifest |
| A ciphertext moved to another `revision_id`, record, parent set, author, counter, flag or kind (including in a tampered local `vault.db` or a future peer path) | AEAD failure → `RECORD_CORRUPT` |
| An enrolled device (holds the VK) forging an object with any identity | not preventable; authorship is attributable only to the *publishing* device via the manifest signature (per-revision signatures are not proposed for v1) |

**Rotation:**
- Each row is re-sealed: new nonces, new `vk_generation` in the AAD, new
  `blob_hash`.
- `revision_id`, parents, author and counter are **unchanged**.
- The v1 hash-remap step in `storage/rotation.rs` is deleted.
- The next index maps the same ids to new blob hashes.
- A device with unsynced local revisions under the old VK, after it
  adopts the new VK through its envelope (§11), re-seals *its own* pending
  rows under the new VK with the same ids before publishing. There is no
  remap and no re-parenting. The old VK is zeroized after that step (a
  transition, not retention).

**Duplicates:**
- Same `revision_id`, same `vk_generation`, different `blob_hash` → the
  helper decrypts both.
  - Identical header and plaintext → benign; the lexicographically lower
    `blob_hash` is kept.
  - Otherwise → **revision equivocation**: record freeze, §10.5.
- A lower `vk_generation` representation of a known id is superseded
  (stale), not a conflict.

**Why random 256-bit:**
- It is the simplest construction with no semantics.
- It never needs recomputation.
- It carries no derived semantics and does not change when the
  ciphertext changes.
- It is **linkable across rotations by design**. That is the point of a
  stable id, and `record_id` already exposes record continuity to the
  provider. Nothing new leaks.

| Alternative | Why not |
|---|---|
| 128-bit random | would suffice against accidental collision |
| `H(record_id, author, counter)` | would merge identity with counter semantics and make local-rollback cases ambiguous |

A malicious VK holder can choose any id in all three schemes, so the
choice does not affect adversarial resistance.

**[Closed S-2 — approved]:** `revision_id` = 256 bits from OsRng.

### 10.3 Heads model

- Per record, `heads` = the revisions with no *admitted* child.
- Applying R: `heads ← (heads \ ancestors(R)) ∪ {R}`.
- `|heads| = 1` → `tip_rev`; otherwise `tip_rev = NULL` and
  `record_conflicts = heads` (fills exclude it; `CONFLICT_PENDING`).
- **Resolution** (`resolve_conflict`, fresh presence): a new revision
  whose parents are all current heads collapses them to one (SY-08). If
  another device holds an extra head, a partial cover leaves those heads
  in conflict.
- **Tombstones:**
  - a delete becomes the state only as the sole head;
  - a non-deleted revision with a tombstone ancestor on the path that
    made it a head → conflict against the tombstone (no resurrection;
    SY-03/04);
  - authoring on a tombstoned record is refused locally.
- Timestamps never decide.

### 10.4 Counters — exact algorithm

**Definitions:**
- A revision R is **applicable** when every parent is admitted locally.
  Otherwise it waits in `pending_revs`.
- Applicability is transitive, so for an applicable R the set `anc(R)` is
  exact and complete, whatever the delivery order.

**Honest-author invariant I:**
- An honest author A never reuses a counter (it keeps `author_hwm`).
- When A authors on X, every revision A previously authored on X is an
  ancestor of the new revision. That holds because A's parents are all
  current heads, every known revision is an ancestor of some head, and A
  may only edit (single head) or resolve (all heads).

**On applying an applicable R = (author A, counter c, record X)**, compare
it with every admitted P ≠ R by A on X:

| Relation | Condition | Classification | Action |
|---|---|---|---|
| P ∈ anc(R) | `P.counter < c` | normal | — |
| P ∈ anc(R) | `P.counter ≥ c` | **counter regression**: R contradicts its own ancestry | R **rejected**: not applied, stored as tamper evidence (`COUNTER_REGRESSION`); the record stays as it was (SY-06) |
| P ∉ anc(R) (R ∉ anc(P) is guaranteed, since P was admitted first) | `P.counter = c` | **equivocation** | — |
| P ∉ anc(R) | `P.counter ≠ c` | **author fork**: violates invariant I for the later-authored one | both kept; both become heads; the record is frozen (SY-05, §10.5) |

**Why out-of-order delivery cannot misfire:**
- The check runs only on applicable revisions, whose ancestry is complete.
- Any lower-counter revision by A that should be an ancestor must already
  be present. Otherwise R would still be pending.
- A genuinely concurrent pair by A is detected the same way whichever
  arrives first. When the second becomes applicable, the other is admitted
  and not in its ancestry.
- Pending revisions are never compared.
- A revision whose parents never arrive, within a manifest that claims
  ancestor closure → that manifest is `MANIFEST_MISMATCH`.

**Local rollback** (for example `vault.db` restored from Time Machine):
- A device whose local generation is below its Keychain last-seen
  generation refuses to author until it has synced to at least that
  generation. That restores `author_hwm` from its own published history.
- This is the existing `MANIFEST_ROLLBACK` machinery.

### 10.5 Equivocation and freeze

- `record_flags(record_id, frozen=1, evidence=[ids])`.
- The record is excluded from fills; `update_item`/`delete_item` return
  `CONFLICT_PENDING`, and the item is marked "tamper".
- It unfreezes only through `resolve_conflict` with fresh presence and an
  explicit acknowledgement of the tamper evidence. The merge revision
  parents all heads.
- Vault-level COMPROMISED stays reserved for registry forks and
  vault-level tamper.

### 10.6 Unsynced revisions authored by a subsequently revoked device (D-5, exact)

**Terms:**

| Term | Meaning |
|---|---|
| D | the revoked device |
| `s_r` | the registry seq of `revoke(D)` |
| `M_rev` | the first manifest in the accepted manifest chain whose registry includes `s_r`. By §8.1 this is the revoker's revocation transition |
| `Admit(D)` | the set of `revision_id`s authored by D that appear in `M_rev`'s index |

**Rule on the revoking device:**
- `Admit(D)` = every D-authored revision in the revoker's local database
  at the moment of its **local** revocation commit.
- These are exactly the revisions the human authorizing the revocation
  had available. The rotation re-seals them into the new state, so they
  appear in `M_rev`'s index automatically.
- From that commit on, any **incoming** D-authored `revision_id` not
  already held is **refused**. That covers a racing provider state
  (§8.2), a later manifest, or pending rows.

**Rule on every other device**, on accepting `M_rev`:
- D-authored revisions in `Admit(D)` stay ordinary history.
- D-authored revisions held, pending or later received that are **not**
  in `Admit(D)` are refused.

**"Refused" means:**
- not admitted (never a head, parent-satisfier, fill candidate or conflict
  participant);
- never included in any index this device publishes;
- local ciphertext deleted after the device's rotation barrier;
- counted in `quarantine_status` as tamper evidence, per record, by
  count only. UI: "N changes from removed device *X* were not accepted".

**It does not mean deletion from committed provider history.** Older
manifests that referenced them stay retained until normal GC (D-4).
Nothing *in* `Admit(D)` is ever removed. So legitimate history committed
before the revocation, as the revoker saw it, survives.

**Why refuse rather than quarantine for review:**
- Every unseen D revision is sealed under a VK that the revocation's
  rotation retires.
- Invariant 7 forbids keeping that VK, so no device can later decrypt
  such a revision to show it to the user.
- "Quarantine for review" would be a promise the system cannot keep.

**Descendants authored by non-revoked devices:**
- A revision C by device B with a refused ancestor can never become
  applicable.
- If B still holds C's plaintext (it is in B's pending local work under
  its rotation barrier), **B re-authors it**: a new `revision_id`,
  `author = B`, parents = current heads.
  - It is then an ordinary edit, which fast-forwards or conflicts.
  - It is surfaced to B's user as "re-applied after removing device X".
- Devices other than its author cannot re-author C, so they refuse it
  with the same count.

**Why the cutoff is the revoker's knowledge and not provider order:**
- A provider-order cutoff ("everything committed before `M_rev`") lets a
  device compromised while unlocked (§12 scenario 8) keep injecting
  revisions until the revocation lands. That is exactly the window the
  revocation is meant to close.
- The cost is that legitimate last-moment edits made on D, and never seen
  by the revoker, are lost as a counted refusal.

**[Closed S-3 — approved as proposed]:**
- (a) the revoker-knowledge cutoff;
- (b) refuse-not-quarantine;
- (c) re-authoring only by the original non-revoked author.

The distinctions the normative text must keep:
- history the revoker had **already accepted** survives;
- **previously unseen** revisions from the removed device are not admitted
  after the revocation barrier;
- **provider arrival order never determines trust.**

**Authorship is not non-repudiation (normative statement):**
- The per-revision `author` and `counter` are **not** cryptographic proof
  of authorship against another *currently trusted* VK holder. Any device
  holding the current VK can construct arbitrary validly encrypted
  revisions, with any `author`, `counter` and parents, that pass AEAD and
  §10.4.
- The author/counter machinery exists to **detect consistency problems**
  (equivocation, regression, forks) and to **drive synchronization**. It
  is not an access-control boundary.
- The security boundary after revocation is the **new VK** (the removed
  device cannot produce or read anything under it) plus the **provider
  authorization cutoff** (its key is inactive from the revoking CAS).
- §10.6's refusal rule relies only on those two, plus the revoker's own
  local knowledge. It never trusts an `author` claim to *grant* anything.
- Per-revision signatures are **not** added in Phase F.

### 10.7 SY coverage

| Test | Covered by |
|---|---|
| SY-01 | §10.3 |
| SY-02 | §10.3 |
| SY-03 | §10.3 |
| SY-04 | §10.3 |
| SY-05 | §10.4/10.5 |
| SY-06 | §10.4 |
| SY-07 | `OV0OBJ02` parser |
| SY-08 | §10.3 |

**New tests:**

| Test | Checks |
|---|---|
| SY-09 | permutation/out-of-order convergence (property test) |
| SY-10 | rotation keeps ids and parents; blob hashes change |
| SY-11 | §10.6 revoked-author refusal and counts |
| SY-12 | duplicate-id handling |
| SY-13 | local rollback refuses authoring until synced |

---

## 11. Device envelopes and iPhone catch-up (D-6, D-13)

### 11.1 Envelope v2

- **Payload:** `DeviceEnvelopePayload v2 = TLV{0x01 vk (32 B), 0x02
  wrapped_at u64, 0x03 vk_generation u32}`.
- Tag 0x04 (the v1 `device_backup_cred`) is **forbidden** (decode
  rejects).
- HPKE `info = "ov0/envelope/v2" ‖ vault_id ‖ device_id ‖
  enrollment_nonce`.
- `creds.bin` and its HKDF context are removed.
- `device_unlock` returns the VK only.
- Affected sections:
  - §2.2: the payload definitions, and the removal of "two payload shapes
    differ deliberately" (the shapes are now identical, distinguished by
    HPKE/AEAD context);
  - §2.5: device wrap and `creds.bin`;
  - §2.8/§2.10: the envelope unlock text and the rotation stage list;
  - §2.9 infos;
  - §5.2: the envelope row;
  - §11.4: class 1 deleted.
- Vectors: XV-ENROLL (Rust and Swift). CR-12 is rewritten to assert that
  **no** payload of any kind carries backup-credential material.

### 11.2 Envelope distribution

- Every index lists `env <device_id>` for exactly the active devices. The
  provider enforces it (§7.4 step 5).
- **Authentication:** the index hash goes into the manifest, which is
  signed, and the checkpoint MACs the manifest. The envelope's HPKE `info`
  binds vault, device and enrollment nonce. The inner `vk_generation` =
  `manifest.vk_generation`.
- **Versioning:** each rotation re-seals every surviving envelope in the
  same journal, so only the latest is needed.
- **Revocation is not weakened:**
  - a revoked device has no envelope;
  - its key is inactive at the provider from the revoking CAS;
  - old envelopes it may hold open retired VKs only.

### 11.3 iPhone envelope catch-up (the only mobile scope in Phase F)

Triggered by the existing event-driven refresh points (§4.7): launch,
foreground, vault screen, reconnection, manual. There is no background
polling.

1. `state_get`, signed by the phone's SE signing key (`ProviderRequest`,
   device class). A `401`/`403` shows "unable to verify" and **never**
   deletes anything (§4.7 discipline).
2. Decode manifest v2. Require generation ≥ the phone's persisted
   **manifest floor**. The floor is new; the phone already keeps a
   registry floor.
3. `blob_get` the index (SHA-256 = `object_index_hash`), then the registry
   and its own `env` blob (SHA-256 per index lines).
4. Verify the registry extends the phone's accepted chain (the existing
   Swift verifier and rollback floor). A valid `revoke` naming this phone
   → the existing §4.7 revoked handling. The phone's absence alone is
   *not* revocation.
5. Verify the manifest signature under an active signer's `sign_pub`
   (CryptoKit `P256.Signing`).
6. SE-open the v2 envelope, and check `vk_generation` =
   `manifest.vk_generation`.
7. Verify the checkpoint MAC under the new VK **and** its binding to this
   manifest's `manifest_core_hash`. This closes N11: the phone computes
   the core hash itself.
8. Atomically replace the stored envelope, manifest and checkpoint; raise
   both floors.

**Swift additions:**
- `ProviderRequest` TLV + SE digest signing (the same pattern as today's
  ACK signing);
- a `URLSession` provider client (WebPKI, D-12; no pinning);
- manifest v2 + index v2 decoders and core-hash computation;
- the envelope v2 parser.

**No record store, merge or publication.**

The provider origin reaches the phone in the enrollment bundle (header v2
`provider`).

**[Closed O-1 — approved]:** the iPhone catches up from the **provider**.
No new Mac route is added.

**Limitation, stated:** revoked keys are refused before any operation
(BK-13). So a phone revoked by a revocation or by an S-4 recovery cannot
learn *through the provider* that it was revoked. It sees
"unable to verify" (never deletion) and learns it through the existing
§4.7 Mac refresh or by re-enrolling. Its envelope opens nothing, because
the VK rotated.

### 11.4 Spec text assuming the iPhone is a full vault writer (to correct; full sync deferred)

| § | Text to correct |
|---|---|
| §11 intro | "Normal sync stays direct peer-to-peer (§7.1 channel carries encrypted record objects)" |
| §11.3 | "Conflicts replicate to all devices…" (true for Mac writers only in F) |
| §12 scenario 1 | the iPhone revokes the Mac, rotates, re-encrypts and publishes. That requires an iPhone vault engine. Until the sync phase, scenario 1 needs a Mac (or total-loss recovery) |
| §12 scenario 8 | "any surviving trusted device" → any surviving **Mac** |
| §5.1 / §5.2 | "store envelope/wraps; Keychain" and "Initial vault transfer … all current record objects": the phone receives but does not maintain records |
| §2.10 | "delivering it is Phase F sync" → envelope catch-up §11.3 |
| §16.7 | RC-01 (scenario 1 end-to-end) → deferred or Mac-only variant |
| §18 | add an explicit later phase, e.g. "F.2 — iPhone record store, merge and backup client; direct Mac⇄iPhone peer sync" |
| Architecture | `credential-vault-security-architecture.md` §11.1 and matrix row C8 ("normal sync prefers direct peer-to-peer"). The spec defers to the architecture doc, so it needs a note that v1 implements provider-mediated Mac sync first and peer sync in F.2 |

---

## 12. Owner decisions

### 12.1 Status of D-1…D-13

| # | Decision | Status | Where incorporated |
|---|---|---|---|
| D-1 | Signature auth; recovery via public keys | **Approved**; recovery derivation closed by S-1 | §4 |
| D-2 | Rust + S3; hosting open | **Approved** | §5 |
| D-3 | Clean v2 break | **Approved** | §7.7 |
| D-4 | No client delete; provider GC | **Approved** | §7.6 |
| D-5 | Merge principles with corrections | **Approved w/ corrections**; S-2, S-3 closed | §10 |
| D-6 | Mac + provider; iPhone envelope catch-up only | **Approved (narrow)**; O-1 closed | §11 |
| D-7 | Fresh macOS account qualifies as "fresh environment" | **Approved** | §14 |
| D-8 | Push relay → Phase G | **Approved** | §7.3 |
| D-9 | Recovery handle, no email requirement | **Approved**; S-5 closed | §4.4, §4.5, §7.4.1 |
| D-10 | Fix Keychain prompts before F | **Approved** | §12.3 |
| D-11 | Atomic recovery-auth updates | **Approved** | §4.2.4, §7.4 |
| D-12 | Public-CA HTTPS, no pinning | **Approved** | §5, §11.3 |
| D-13 | Keep `device_backup_cred` | **Rejected** → removed in v2 | §11.1 |

### 12.2 Security and owner choices — all closed

| # | Decision | Status | Normative consequences |
|---|---|---|---|
| S-1 | RFC 9180 `DeriveKeyPair` composition for recovery-auth keys | **Approved with qualification** | the entropy statement in §4.2.2 becomes spec text: the MP key is in the same password-guessing class as `password.wrap`; domain separation adds no entropy. Primitive vectors and composition vectors are kept separately |
| S-2 | `revision_id` = 256 bits from OsRng | **Approved** | stable ids are linkable across rotations by design (§10.2) |
| S-3 | Revoker-knowledge cutoff; refuse-not-quarantine; re-author by the original author | **Approved** | plus the normative "authorship is not non-repudiation" statement (§10.6) |
| S-4 | Total-loss `finalize` revokes every prior active device via named `revoke` entries signed by the newly installed device, in the same transition | **Approved** | provider requires the complete set (§7.4 step 6). Keeping an existing device means using trusted-device recovery, not total-loss recovery |
| S-5 | Pepper + fake locate responses; **provider-wide** recovery throttle | **Approved with stronger limits** | §4.5 S3 slot reservation; tests across two instances |
| O-1 | iPhone envelope catch-up from the provider | **Approved** | no new Mac route (§11.3) |
| O-2 | ROTATING_KEYS internal, with a status/progress event | **Approved** | §6.3 |

**Review corrections incorporated in revision 3:**

| Correction | Where |
|---|---|
| KDF-downgrade protection | N12 → §4.4.2 |
| Crash-safe handle claim | N13 → §7.4.1 |
| Explicit remote completion | N14 → §4.2.6 |
| Honest handle-privacy wording | §4.4.1 |
| Operational state is not a root of trust | §7.1 |
| The listed design tests | §14 |

### 12.3 Keychain gate fix (D-10) — test-only design and cleanup procedure

**Root cause [Fact]:**
- The tests' Keychain namespace is `ov0ops-<pid>-`
  (`tests/vault_fx/mod.rs:136–144`).
- Items are deleted only at fixture start, never at teardown.
- A new test process reusing the PID of a process from 2026-09-21 hit
  that process's item, whose ACL trusts a different binary. macOS
  prompted.

**Fix (test code only, when F is authorized):**
1. **Namespace:** `ov0t-<run_id>-` with `run_id` = 16 hex chars from
   OsRng, set once per process. The same scheme applies to `ipc_capture`
   (`ov0ipccc-`), `lock_preempts_panel` and `helper_termination`.
2. **Teardown:** a `KeychainNamespaceGuard` whose `Drop` deletes every
   item under its prefix, plus a process-exit hook. The gates' existing
   `trap cleanup` additionally sweeps `ov0t-*`.
3. **Fail fast:** test binaries call
   `SecKeychainSetUserInteractionAllowed(false)` at start, so any would-be
   prompt returns `errSecInteractionNotAllowed` and the test fails instead
   of hanging. This must be verified first; see U-4.
4. **Pre-flight:** `phase-f-gate.sh` fails at start if any `ov0*` Keychain
   service exists, printing the cleanup command.
5. **SE test keys (N10):** tests create SE keys under tag prefix
   `test.<run_id>.` instead of `dev.`, and delete them in teardown. That
   makes a future cleanup of test SE keys pattern-safe.
6. **Production is untouched:**
   - the service-prefix override is `#[cfg(debug_assertions)]` and
     compiled out of release (the Phase E gate check 5 proves it);
   - no ACL, accessibility class or bridge code changes.

**One-time cleanup of the stale items — dry-run done (read-only, this
pass):**

```bash
# enumerate (read-only)
security dump-keychain | grep -oE '"svce"<blob>="[^"]*"' | sed -E 's/"svce"<blob>="(.*)"/\1/' > /tmp/kc-services.txt
grep -E '^ov0(ops|ipccc)-[0-9]+-com\.racker\.zero\.vault\.(state|helper-prefs)$' /tmp/kc-services.txt
```

**Dry-run result (2026-09-24):**

| Selected by the matcher | Count |
|---|---|
| `ov0ops-<pid>-com.racker.zero.vault.state` | 353 |
| `ov0ops-<pid>-com.racker.zero.vault.helper-prefs` | 14 |
| `ov0ipccc-<pid>-com.racker.zero.vault.state` | 3 |
| **Total** | **370** (= all `ov0*` items; none unmatched) |

**Why the matcher cannot select production entries:**
- Production services are the compile-time constants
  `com.racker.zero.vault.state`, `com.racker.zero.vault.helper-prefs`
  (`keychain.rs`) and `com.racker.zero.vault.se-{signing,agreement}`
  (`Bridge.swift`). All begin with `com.`.
- The matcher is anchored at `^ov0` and requires the full synthetic
  suffix, so a `com.`-prefixed service can never match.
- The same dry-run found 1 `com.racker.zero.vault.state` and
  542 + 542 `…se-*` items. **None matched.**

**Deletion** (owner-run, after reviewing the dry-run list):

```bash
grep -E '^ov0(ops|ipccc)-[0-9]+-com\.racker\.zero\.vault\.(state|helper-prefs)$' /tmp/kc-services.txt \
  | while read -r s; do security delete-generic-password -s "$s" -a default >/dev/null && echo "deleted $s"; done
```

- `-a default` further restricts matches to the test fixtures' account.
- Deleting items whose ACL trusts another binary may itself prompt (U-4).
  If it does, try one item first, and stop and report rather than
  approving 370 dialogs.
- **The 542 SE key pairs are excluded.** Their tags can't be separated
  from a real key by pattern, so they need an allowlist-based procedure
  later.

---

## 13. Normative spec sections requiring amendment

| § | Amendment |
|---|---|
| Header / status | v0.4, change list; drop "pre-implementation" |
| §1.1, §1.2 | crates (`vault-proto`, `vault-provider-core`, `vault-provider`); the "Backup request authentication" row → request signatures; network row unchanged |
| §1.3 | chunked stream sub-protocol (§9); frame cap unchanged |
| §1.5 | ops per §6.2; never-list: remove "backup credentials" (none exist); name sealed wraps/envelopes in backup sessions; `sign_backup_request` → `sign_provider_request` |
| §1.6 | resumable staged publication while LOCKED |
| §1.7 | recovery sheet adds handle + provider origin; remote-pending warning copy for RK replacement (§4.2.6) |
| §2.2 | `DeviceEnvelopePayload` v2 (no credential); recovery-auth keys added to the key hierarchy with the S-1 entropy statement |
| §2.3 | frozen-tuple **client allowlist**: locate/header KDF parameters outside it are refused before any MP derivation; tuple upgrades only by client release (§4.4.2) |
| §2.5 | device wrap v2 info; delete `creds.bin` |
| §2.8 | device-envelope unlock yields the VK only |
| §2.9 | infos added and retired (§7.7 #18) |
| §2.10 | header v2 fields; rotation keeps `revision_id`s; delete the "revision-hash remap" paragraph; stage list without `creds.bin`; offline envelope delivery → §11 |
| §3.1 | staging directory |
| §3.2 | schema v2 (`revision_id`, pending, quarantine counts, flags, hwm); heads model; counter algorithm; freeze; revoked-author rule |
| §3.3 | `revision_id` definition |
| §3.4 | plaintext classification: `revision_id`, parents in the index |
| §3.5 | `user_version` 2; header v2 |
| §3.7 | `OV0OBJ02` |
| §4.4 / §4.5 | S-4: a `recovery_epoch` in a total-loss `finalize` must be followed by named `revoke`s of every prior active device, signed by the epoch device (rule 6 text + §4.5 note). Encoding unchanged |
| §4.6 | fork / COMPROMISED entry points; `prev_manifest_hash` fork evidence |
| §4.7 | provider `401`/`403` ≠ revocation; iPhone catch-up |
| §5.1, §5.2 | delete "Backup credential registration"; enrollment = publish; bundle v2 + session; envelope v2 |
| §6.4 / §6.5 / §7 | untouched except §7.2 (APNs → Phase G, no provider change in F) |
| §11 (all) | replaced per §§4, 7, 8: intro (sync scope); §11.1 (abstractions); §11.2 (storage v2, operational state ≠ root of trust); §11.3 (state transition, handle-claim protocol, remote-completion status); §11.4 (auth v2, S-1 derivation + entropy statement, policy table, replay, shared recovery throttle, no delete, no revoke route); §11.5 (state response, fork evidence, locate policy + header cross-check); §11.6 (metadata: public handle, dictionary-testable `handle_key`, nonces, rate-limit slots); §11.7 (unchanged; sheet fields); §11.8 (finalize = `StateTransition` kind 3 with S-4 revokes) |
| §12 | scenario 1 (Mac required until F.2); scenarios 3/4 (locate by handle, KDF policy, recovery public keys, S-4 revoke-all); scenarios 5–7 (atomic auth updates; REMOTE_UPDATE_PENDING semantics and copy); scenario 7 (strong warning until the remote cutoff commits); scenario 8 ("surviving Mac") |
| §13.1–13.3 | states per §6.3 (ROTATING_KEYS internal); remote-completion status is not a state |
| §15 | error codes per §6.3 (incl. `KDF_POLICY_VIOLATION`, `RECOVERY_METADATA_MISMATCH`, `RECOVERY_THROTTLED`, `HANDLE_TAKEN`) |
| §16 | tests per §14 |
| §17 | provider supply-chain scope; helper unchanged |
| §18 | Phase F scope + gate (§14); add Phase F.2 |
| §19 | item 13 (rehearsal log); item 24 (signatures, not credentials) |
| §21 | OQ-2 note: provider remains a dumb ciphertext store; push → G |
| Architecture doc | §11.1 / matrix C8 note (§11.4 here) |

---

## 14. Test and gate changes

**Rewritten:**

- **CR-12:** no credential in any payload.
- **CR-13:** recovery-auth derivation vectors.
- **BA-01…06 → PR-01…06** (provider requests):
  - PR-01: IPC transcript and helper-log scan for `sk_c`/`ikm_c` canaries;
  - PR-02: no raw-bytes signing;
  - PR-03: XV-REQSIG;
  - PR-04: body, path, method, vault, audience or expected-state
    substitution → provider rejects;
  - PR-05/06: policy table.
- **BK-01…16 revised:**
  - BK-12: signed-request replay;
  - BK-13: a revoked key is refused from the revoking CAS;
  - BK-14: recovery-class scope;
  - BK-15: finalize installs the epoch device;
  - BK-16: atomic recovery-auth updates plus the D-11 salt rule.
- **RF-01…08** on `StateTransition` kind 3.

**New:**

- **BK-17:** concurrent publishers — no overwrite; the loser gets
  `STATE_MOVED` and merges.
- **BK-18:** provider state/blob dump contains no reusable secret.
- **BK-19:** revoke without rotation, or with the target's `env` →
  `422`.
- **BK-20:** idempotent transition replay.
- **BK-21:** GC never deletes a reachable blob; a GC race →
  `BLOB_MISSING` → retry.
- **BK-22:** handle claim uniqueness and fake-locate indistinguishability.
- **Review-mandated design tests (revision 3):**

  | ID | Scenario | Expected |
  |---|---|---|
  | HC-01 | crash after handle claim (C2), before state create | the same vault's retry completes (C1→C3→C4); after grace G, another vault can reclaim (C2′); within G it gets `409` |
  | HC-02 | two concurrent `create`s for one handle (two provider instances) | exactly one claim; the other `409 HANDLE_TAKEN` |
  | HC-03 | lost `create` response (after C4) | retry → `200`, same `state_commit`; no second state; claim `bound` |
  | HC-04 | crash after C3, before bind | retry binds; a reclaim attempt by another vault is refused (live) |
  | HC-05 | reclaim racing the original owner's late retry | exactly one wins; the loser's generation-1 state is rolled back and it gets `409` |
  | RU-01 | MP change while the provider is unavailable | local `REMOTE_UPDATE_PENDING` persists across lock/restart; the old MP still authenticates remotely; copy never claims the cutoff; commit on recovery of the provider → `REMOTE_COMMITTED`, old MP refused |
  | RU-02 | security-driven RK replacement, offline | strong banner while pending; priority retry; the old RK still recovers remotely until commit, then fails |
  | RU-03 | crash between the local commit and the publish | pending status restored at launch; staged transition resent; idempotent |
  | RU-04 | provider `5xx` ×3 during pending revocation | `BACKUP_REVOCATION_FAILED` surfaced; retries continue; status stays pending |
  | RU-05 | pending state moved remotely (needs merge) while LOCKED | waits for unlock; warning stays |
  | KD-01 | locate returns weaker Argon2 parameters (m, t, p, version, out_len) or wrong-length salts | `KDF_POLICY_VIOLATION`; **no MP prompt, no derivation** (asserted by panel/KDF call counters) |
  | KD-02 | locate returns *stronger* or unknown parameters | refused (exact tuple only) |
  | KD-03 | locate metadata differs from the authenticated committed header (salt, auth salt, `vault_id`, params) | `RECOVERY_METADATA_MISMATCH`; recovery aborted; staging deleted |
  | RL-01 | recovery-auth failures spread across **two provider instances** sharing one store | combined failures per vault per window ≤ L; the (L+1)th → `429` without verification |
  | RL-02 | a legitimate recovery with many successful recovery-class requests | consumes no slots; completes |
  | RL-03 | crash between slot reserve and release | the slot counts as a failure for that window only |

- **SY-09…13** (§10.7).
- **EV-01…05:**
  - envelope set = active set;
  - Mac offline-catch-up;
  - **iPhone catch-up** (§11.3), including N11;
  - revoked device gets nothing openable;
  - `403` ≠ revocation.
- **TR-01…08:** stream isolation, order, caps, hash mismatch, cancel,
  disconnect, restart sweep, enrollment bundle > 64 KiB.
- **ST-01…05:** state transitions per §6.3, including lock during each
  state.
- **Vectors:** XV-OBJ, XV-RECORD-AAD, XV-INDEX, XV-STATE, XV-REQSIG,
  XV-RECOVERY-AUTH (+ RFC 9180 A.3 + RFC 6979), XV-HANDLE, and XV-ENROLL
  v2 (Rust + Swift).

**Provider:**
- `ProviderCore` over `FsStores`;
- axum loopback;
- an optional S3 conformance run with owner-held credentials and
  synthetic data.

**`scripts/phase-f-gate.sh`:**
- nests `phase-e-gate.sh`;
- runs the Keychain pre-flight and fail-fast (§12.3);
- runs everything above;
- records the SE signing latency (U-1);
- checks the helper dependency count is unchanged and produces the
  provider supply-chain report;
- runs the **fresh-environment rehearsal** (D-7), below.

**Fresh-environment rehearsal:**
- A new macOS user account with no access to the original user's vault
  files or Keychain.
- It creates a new SE identity and recovers from provider state
  (loopback or deployed) plus the handle and MP (and, separately, RK).
- It records the FR-01 display, the sheet comparison, the KDF-policy
  check and the header cross-check, the S-4 revokes of every prior device,
  and a successful publish by the new device.
- The report calls this a **fresh environment**, not proof of
  physical-hardware loss.
- A second-Mac run is optional extra evidence.

---

## 15. Dependabot assessment — `security/dependabot/108`

**Facts [Fact]:**

| | |
|---|---|
| Advisory | GHSA-wrw7-89jp-8q8g / RUSTSEC-2024-0429 |
| Issue | unsound `glib::VariantStrIter` iterator impls |
| Affected | `glib` ≥ 0.15, < 0.20 |
| Present | 0.18.5 |
| Patched | 0.20.0 |

**Reach:**
- Enters only via Tauri's Linux GTK stack (atk/gtk → tray-icon, muda,
  tao, tauri-runtime-wry).
- `cargo tree -i glib --target aarch64-apple-darwin` prints nothing.
- Absent from `source-vault-helper`.
- SourceMobile has no Rust. The future provider has no GTK.
- No direct `glib` use in the project.
- `cargo audit` reports it as a warning, not a vulnerability.

**Relevance:** none for the macOS product, the vault or the provider.

**Action:**
- No dependency change in this pass.
- **Leave the alert open and visible**, as the owner instructed; do not
  dismiss it.
- Re-assess if Linux support ships or the Tauri/GTK graph changes.

**Not a Phase F blocker.**

**Informational:** `cargo audit` also warns on `anyhow` 1.0.102
(RUSTSEC-2026-0190) and `event-listener` 5.4.1 (RUSTSEC-2026-0221). They
are unsound warnings outside the helper; no action in this pass.

---

## 16. Remaining unknowns and blockers

| ID | Unknown | Resolution |
|---|---|---|
| U-1 | SE signing latency (M2, A15) | measure first. Above ~20 ms/request, add a batch blob upload (one `ProviderRequest` whose body hashes a blob list) |
| U-3 | Local S3 emulator fidelity for conditional writes | the loopback suite uses `FsStores`; S3 conformance runs against real S3 |
| U-4 | Whether `SecKeychainSetUserInteractionAllowed(false)` suppresses legacy ACL dialogs; whether deleting foreign-ACL items prompts | test on one item before relying on it |
| U-6 | Hosting platform for the provider container | not blocking (D-2) |
| U-7 | Vault size limits (index ≤ 8 MiB ≈ 40–50k revisions) | revisit before Phase J |
| U-9 | **No spec defines how a user exits COMPROMISED** (registry fork resolution). Pre-existing | Phase F implements entry and surfacing only; resolution needs its own owner-level design |
| U-10 | The 542 leftover SE test key pairs (N10) | allowlist-based cleanup after the tag-prefix change |

Revision 1's U-2, U-5 and U-8 are resolved (by D-7, D-2/S3 and N11).

**Status of every item:**

| Item | Status |
|---|---|
| S-1…S-5, O-1, O-2 | **closed** (§12.2) |
| U-6 hosting platform | **open, legitimately non-blocking.** The provider is a portable container, and `ProviderCore` is tested over `FsStores` |
| U-1 SE signing latency | **a measurement to perform** at the start of Phase F. It decides whether blob uploads need batch signing (one `ProviderRequest` over a blob list). The protocol accommodates either |
| U-4 | **a small pre-gate test-infrastructure experiment** (one foreign-ACL test item) before relying on the fail-fast call or running the 370-item cleanup |
| U-9 COMPROMISED exit | **a pre-existing, separate design issue.** Phase F implements entry and surfacing only and must not invent a resolution procedure |
| U-3, U-7, U-10 | tracked; not blocking |

**Genuinely new blockers found while applying the revision-3
corrections: none.** Three consequences are recorded rather than
treated as blockers:
1. **N14 × S-4.** During `REMOTE_UPDATE_PENDING` for a stolen-RK
   replacement, whoever holds the old RK plus the public handle can run a
   total-loss recovery that (by S-4) revokes this Mac. This is inherent to
   RK possession until the remote cutoff commits. It is mitigated by the
   priority retry and the warning, and surfaced as a takeover if it
   happens (§4.2.6).
2. **Exact-tuple KDF policy.** Any future Argon2 parameter upgrade (§2.3)
   now requires a client release that adds the tuple to the allowlist
   (§4.4.2). The provider can never introduce one.
3. **Revoked phones can't learn revocation from the provider** (O-1
   limitation). They learn it via §4.7 or re-enrollment.

**Before implementation:** the §13 spec and architecture revision,
approved by the owner.

**Before the Phase F gate:**
- D-10 implemented;
- U-4 checked;
- the one-time Keychain cleanup done;
- U-1 measured.
