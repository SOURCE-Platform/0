# Phase F readiness review — credential vault

| | |
|---|---|
| **Date** | 2026-09-24 |
| **Status** | **Phase F NOT AUTHORIZED / NOT STARTED** |
| **Desktop repo inspected** | desktop repo (`0`), `main` @ `b7a434f` (baseline `31957d7` + two `docs/`-only commits), 0 ahead / 0 behind `origin/main`, clean |
| **iPhone repo inspected** | iPhone repo (`source mobile`), `main` @ `d6fe885` (= baseline), 0 ahead / 0 behind `origin/main`, clean |
| **Phase E baseline gate** | `scripts/phase-e-gate.sh` → **`PHASE E GATE: PASS (12 checks)`**, exit 0, incl. nested `PHASE D GATE: PASS (12 checks)` (→ C → B → A). Required one manual Keychain approval — see §2 |

**This document is a readiness review, not a normative document.**
`credential-vault-implementation-spec.md` (v0.3.1) remains normative until
owner-approved corrections are made to it. Nothing here resolves the
contradictions it lists, and nothing here authorizes implementation.

---

## 1. Repository state (verified independently)

| | Desktop `~/Documents/0` | iPhone `~/Documents/source mobile` |
|---|---|---|
| Branch / tracking | `main` → `origin/main` | `main` → `origin/main` |
| Ahead / behind (after `git fetch`) | 0 / 0 | 0 / 0 |
| Working tree | clean | clean |
| HEAD | `b7a434f` (2 docs commits on baseline `31957d7`) | `d6fe885` (= baseline) |
| Non-`docs/` changes since baseline | **none** | **none** |

The only desktop change since the baseline was
the Opus 5 → Opus 5.5 handoff notes (internal, not published).

## 2. Baseline gate result

`scripts/phase-e-gate.sh`: all 12 checks PASS — Phase E tests (32), full
helper suite (187), file-length audit (bridge at exactly 200/200 lines),
debug sign + DR verify, release overrides compiled out, signed E2E, IPC/log
secret scan, UI-05 surface, main app + frontend build, supply chain (helper
dependency count 96, no `hpke`), iPhone tests (35 on iPhone 17 Pro Max
simulator), and the nested D → C → B → A regression.

**Finding not recorded in the handoff: the gate is not unattended.** During
the nested Phase A stage the `vault_contracts` test binary blocked in
`SecItemCopyMatching` → `SecKeychainItemCopyContent` for ~45 minutes on a
login-Keychain access prompt for its own synthetic item
(`ov0ops-<pid>-com.racker.zero.vault.state`), and continued only after the
owner entered the login password and clicked **Allow**. The handoff's
"briefly shows two real windows that dismiss themselves" is therefore
incomplete, and a Phase F gate nested on top inherits a human-in-the-loop
step.

## 3. Phase F scope and invariants (as understood)

**Scope (§18):** `BackupStore` trait + `FsBackupStore` + `HttpBackupStore`;
publication (object PUT → manifest CAS) and GC; §3.2 revision-graph merge;
§11.4 per-device and MP/RK recovery credentials, server-side revocation,
300 s timestamp window + per-credential nonce replay cache; minimal provider
(object store, manifest CAS, push-relay endpoint, recovery locator,
credential registry); BK-01…16, SY-01…08. **Gate:** kill-both-devices
rehearsal on a fresh machine with synthetic data; revocation and replay
tests green.

**Invariants to hold:**

- Network ≠ credentials: main does all networking; the helper holds every
  credential and returns only a MAC tag via `sign_backup_request`
  (helper-built method/path from a typed enum, helper-generated `t`/`n`,
  never MACs caller bytes).
- Provider untrusted for confidentiality and not a root of trust: devices
  verify manifest signature, registry chain, and §4.8 checkpoint
  themselves; provider down/filtering = "unknown", never "revoked".
- §4.6 rollback/fork: lower generation → `MANIFEST_ROLLBACK`; forks
  surfaced, never auto-resolved; §11.7 fresh-device freshness is the honest
  display only (generation, date, count, sheet comparison).
- Merge never timestamp-picks; concurrent edit and delete‖edit are
  conflicts; never resurrect.
- §11.8 finalize is the single atomic transaction; no retired VK is
  retained; no post-finalize rotation.
- Revocation takes effect at the provider immediately and is retried, not
  best-effort.
- No real credentials; no Path B HPKE.

## 4. Present vs. missing

**Present:** §3.7 object format; object index; `SignedManifest`; §4.8
checkpoint; snapshot build/upload/download/materialize; §11.8 finalize
body; `FsBackupStore` (models the *provider*: auth by credential hash,
device/recovery scoping, CAS publish, revoke, locate/bundle/finalize,
atomic single-head commits); Phase D recovery engine (library-only,
test-driven); `apply_revision` (fast-forward, concurrent→conflict,
tombstone, equivocation detection); recovery credential/locator derivation;
device-credential issuance + `creds.bin`; error codes `FINALIZE_CONFLICT`,
`BACKUP_CONFLICT`, `BACKUP_UNAVAILABLE`, `BACKUP_OBJECT_MISSING`.

**Missing:**

- the `BackupStore` trait itself, `HttpBackupStore`, the provider service,
  and any backup client in the main app;
- IPC ops `sign_backup_request`, `resolve_conflict`,
  `backup_snapshot_prepare`, `backup_state_apply`;
- states RECOVERING / SYNCING / BACKING_UP / ROTATING_KEYS / COMPROMISED
  (`VaultState` has six states);
- HMAC request signing, nonce replay cache, timestamp window;
- error codes `SIGNING_REFUSED`, `BACKUP_REPLAY`,
  `BACKUP_REVOCATION_FAILED`, `BACKUP_STALE`;
- provider wiring in product flows: setup collects no email, creates no
  account, registers no locators/recovery credentials; enrollment registers
  no device credential (Phase E gate item deferred); MP/RK changes do not
  re-register; `revoke_device` makes no provider call;
- any app-level total-loss recovery flow (library only);
- GC, 48 h stale warning, backoff/queueing.

## 5. Production backend options (§11.1) — not chosen

The provider must execute logic (HMAC verification, replay cache, registry
signature check on revoke, finalize structural validation, rate-limited
locate), so the decision is *storage with conditional writes + a small
service*, not a bare bucket. `FsBackupStore` already keeps all mutable
provider state in one head document committed by replacement; any backend
offering compare-and-swap on a single object can therefore make
publish/revoke/finalize atomic, with content-addressed immutable objects
beside it.

| Option | For | Against |
|---|---|---|
| **A. One VPS: small Rust `axum` service + SQLite (state) + local disk (objects)** | real transactions make finalize atomicity trivial; reuses helper Rust code (TLV, manifest, registry verify); one process to reason about; cheapest; closest to `FsBackupStore` | owner operates durability — the entire point of a backup — needs off-site replication/snapshots, TLS, patching, uptime; single region |
| **B. Same Rust service, stateless, in front of managed object storage with conditional writes** (S3 `If-Match`/`If-None-Match`, GCS generation preconditions, Azure Blob ETags, R2) | vendor durability; head-object CAS via ETag/generation; shared Rust code | two components; replay cache in memory (§11.4 allows best-effort) or another store; S3 egress cost; conditional-write semantics differ per vendor |
| **C. Cloudflare Workers + one Durable Object per vault (+ R2 blobs)** | per-vault serialized transactional state makes CAS, replay cache, and finalize atomicity natural; minimal ops | JS/TS or Rust→wasm, so server-side P-256/TLV checks must be ported or compiled; hardest parity with `FsBackupStore`; lock-in |
| **D. Rust service + managed Postgres (state) + object storage (blobs)** | mature transactions and managed backups | most moving parts; likely more than v1 needs |

Cross-cutting factors that shape the choice: who operates the service
(Source vs. user); where the APNs `.p8` key will live (Phase G); email-handle
storage; how main trusts the provider's TLS (public CA vs. pinned — spec
silent). Vendor specifics (conditional-write support, pricing) must be
re-verified against current documentation before deciding.

## 6. Contradictions, missing prerequisites, decisions

### Spec contradictions — need owner/spec decision

1. **Backup credentials cross IPC in the spec's own flows.** §1.5/§11.4
   say no backup credential ever crosses IPC, yet the §11.8 finalize body
   carries `new_device_backup_credential` (0x0B), which main transports
   verbatim; and `device_register` (§5.2) and recovery-credential
   registration (§11.4) must deliver the raw credential to the provider
   (the provider needs the key to verify HMACs), with main as the only
   network path. Options (all design changes): an explicit documented
   exception (as for sealed envelopes), encrypting registration bodies to a
   provider key, or moving device-class request auth to Secure Enclave
   signatures.
2. **Object addressing.** §11.4 paths are `/objects/<sha256hex>`; §11.2 and
   the code use `objects/rec/<record_id>/<rev_hash>`, `objects/index/<gen>`,
   `objects/checkpoint/<gen>`. The client cannot know the index or
   checkpoint SHA-256 before fetching, and `rev_hash` ≠ SHA-256(bytes).
   This is a wire format → version bump + vectors on change.
3. **§11.1 trait vs. architecture.** The trait is `async`, carries no auth
   or `vault_id`, and lacks register/revoke/locate/bundle/finalize/push-token
   methods; `cas_manifest` carries no checkpoint although §4.8 requires the
   provider to receive one. The helper has no async runtime and, per §1.1,
   no HTTP client, so `HttpBackupStore` must live in the main process;
   `FsBackupStore` plays the provider, not a client. Decide the trait shape
   and whether provider logic becomes a shared crate used by both
   `FsBackupStore` and the service.
4. **Revocation ordering.** §11.4 has the provider verify the revoke entry
   "against the uploaded registry" (publish first); §12 scenario 1
   publishes then deactivates; scenario 8 deactivates then publishes.
5. **`manifest_cas` wire format undefined** (expected generation +
   manifest + checkpoint in one request?); §11.3's `manifest.new`
   PUT/DELETE steps are redundant with a server-side CAS; retention name
   `manifest.gen-<n>.json` ≠ the code's content-addressed manifest keys.

### Missing prerequisites

6. **IPC frame cap 64 KiB (§1.3) vs. objects up to 1 MiB (§3.7).** The
   Phase E enrollment bundle already sends a whole snapshot in one frame and
   will fail on a realistically sized vault. Main may not open vault storage
   (§1.2), so a chunked/paged transfer design is needed.
7. **The helper calls the store directly today** (recovery engine, snapshot
   upload). In production, recovery must become a helper↔main multi-step
   exchange; §1.5 has no ops for feeding downloaded state into a RECOVERING
   helper, extracting re-encrypted objects and the finalize body, or
   staging publication. New ops need §1.5 never-list review (§19 item 14).
8. **All revisions are still authored under the all-zero
   `LOCAL_DEVICE_ID`, even after enrollment.** Two syncing devices would
   share author and counter → false equivocation. SY-01…05 are not
   meaningful until authors use the real `device_id`.
9. **`apply_revision` gaps.** A conflict-resolving merge revision is
   classified as a new conflict (tip is NULL → not fast-forward), making
   SY-08 impossible; no counter-regression check (SY-06); equivocation does
   not freeze writes or set a tamper flag (SY-05); no topological
   application for out-of-order arrival. The code comment "Phase F sync
   adds no new merge code" is not accurate.
10. **The object index omits device envelopes** (listed in §11.2).
    Delivering a re-sealed envelope to an offline device (handoff open
    item 5) depends on them being in the backup.

### Scope decisions for the owner

11. **iPhone / peer sync.** Mac⇄iPhone direct sync — "normal sync" per §11
    and the architecture — is not assigned to any §18 phase. The iPhone has
    no record store, merge, or backup client, which §12 scenario 1 (iPhone
    revokes the Mac, rotates, publishes) would need. Is the iPhone in F?
12. **"Fresh machine" for the gate.** A Secure Enclave is required (OQ-1),
    and macOS VMs are believed not to expose one. Does a second physical
    Mac count, or a fresh macOS user account on this Mac?
13. **Push relay.** Listed in F's provider scope, but APNs is Phase G —
    stub now or defer?
14. **Email handle.** Setup must collect an email and send it to the
    provider — a product/privacy decision.
15. **Gate Keychain prompt** (§2) — fix before Phase F's gate stacks on it,
    or record it as a known manual step?

### Minor

- The server cannot tell whether a recovery-credential `object_put` is
  "inside §11.8", so in practice any MP/RK holder may upload objects at any
  time; the spec should state this.
- The §2.12 Swift bridge is at exactly its 200-line cap.
- The spec header's "pre-implementation" wording is stale (already noted in
  the handoff).

Everything else in the handoff notes matched the repositories.

---

*Readiness review only. Phase F begins only when separately authorized by
the owner, after decisions on §6 items 1–5 and 11–15 and the §5 backend
choice.*
