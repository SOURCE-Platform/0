# Phase D.1 Verification Report — Pre-Phase-E Protocol Closure

Date: 2026-09-20. Spec: `credential-vault-implementation-spec.md` v0.3.1
(this pass adds the §4.8 checkpoint and makes the landed Phase D mechanics
normative). Scope: the five closure items only — repeated-recovery fix,
spec synchronization, printing decisions, Argon2 calibration, hygiene.
Not started: Secure Enclave keys, HPKE, device enrollment, or any other
Phase E work.

Status: **PHASE D GATE: PASS (12 checks)**, run of record 2026-09-20 with the checkpoint work included, plus Phase C (19/19), Phase B (6/6), Phase A (11/11) and the 10-minute TLV fuzz. The Phase D blocker (repeated recovery) is closed.

## 1. Repeated-recovery protocol (exact)

The pre-D.1 rule required every future fresh device to hold the VK that
keyed each historical `recovery_epoch` proof. That VK is retired by the
rotation the same recovery performs, so a second total-loss recovery was
impossible without persisting extinct keys. It is replaced by a
**current-VK registry checkpoint** (spec §4.8, `backup/checkpoint.rs`).

```text
checkpoint_key = HKDF-SHA256(ikm=current_VK, salt=vault_id,
                             info="ov0/registry-checkpoint/v1")
registry_checkpoint = HMAC-SHA256(checkpoint_key,
    "ov0/registry-checkpoint/v1" ‖ tlv(checkpoint_body))
```

`checkpoint_body` is canonical TLV (§4.2 rules: ascending tags, minimal
big-endian integers, terminator) with exactly:

| Tag | Field | Type |
|---|---|---|
| 0x01 | `version` | u32 = 1 |
| 0x02 | `vault_id` | 16 B |
| 0x03 | `epoch` | u64 |
| 0x04 | `registry_head` | 32 B (`entry_hash` of the tip) |
| 0x05 | `manifest_core_hash` | 32 B |
| 0x06 | `manifest_generation` | u64 |
| 0x07 | `vk_generation` | u32 |

The stored object appends 0x08 `mac` (32 B) to the same entry, and decode
refuses any non-canonical re-encoding.

```text
manifest_core_hash = SHA-256("ov0/manifest/core/v1"
                             ‖ tlv(SignedManifest without 0x10 signature))
```

**No recursion, by construction:** the manifest never contains the
checkpoint; the object index never lists it (it lives at
`objects/checkpoint/<generation>`); and `manifest_core_hash` excludes the
manifest signature, so nothing the checkpoint commits to is derived from
the checkpoint.

**Regeneration.** Rebuilt under the then-current VK on every recovery
epoch, every registry mutation, every VK rotation, and every manifest
change covered by `manifest_core_hash` (which includes `generation`, so
every publication). Provider-side, a publication or finalize whose
checkpoint does not describe the state being installed is refused
structurally (the provider holds no VK and cannot verify the MAC).

**Fresh-device order** (`recovery/total_loss.rs::begin`):

1. recover the current VK through MP or RK;
2. verify the served checkpoint's MAC under that VK;
3. require `registry_head` to equal the downloaded registry's head
   exactly, and `vault_id`, `manifest_core_hash`, `manifest_generation`,
   `vk_generation`, `epoch` to equal the served manifest's — else
   `MANIFEST_MISMATCH`;
4. only then treat that registry as the authorization anchor;
5. verify ordinary device signatures and structural/hash-chain integrity
   as usual (`EpochPolicy::CheckpointAnchored` keeps §4.4 rules 1–5, 7, 8
   and the single-installation rule; it only stops *requiring* proofs for
   historical epochs), plus the manifest signature under a non-revoked
   installed device.

Historical `recovery_epoch` proofs remain audit evidence and are still
verified by devices that hold the relevant transition state
(`EpochPolicy::RequireProof`, unchanged). **No old VK and no old proof key
is persisted anywhere** to make this work.

## 2. Second and third recovery results

`tests/registry_checkpoint.rs` (CP-01…CP-08), on FsBackupStore with
synthetic data:

| Test | Result |
|---|---|
| CP-01 second and third total-loss recovery | both succeed on brand-new devices; `vk_generation` 2 → 3 → 4 and backup generation 2 → 3 → 4 (exactly one rotation each, before finalize); contents identical to the original vault after the third; three epochs present as audit history; the superseded VK cannot MAC the current checkpoint |
| CP-02 provider substitutes a different registry (own device + own manifest signature) | refused (`MANIFEST_MISMATCH`) — it cannot produce the checkpoint |
| CP-03 provider alters historical `recovery_epoch` bytes | refused twice: in place → `BACKUP_OBJECT_MISSING` (index hash); with index+manifest rebuilt → `MANIFEST_MISMATCH` (checkpoint binding) |
| CP-04 checkpoint for the wrong registry head | refused |
| CP-05 checkpoint for the wrong manifest generation / wrong epoch | refused |
| CP-06 checkpoint under the old VK after rotation | refused; the freshly published checkpoint verifies under the new VK only |
| CP-07 stale-but-valid complete state | still recovers; classified by the printed sheet as "older than sheet" — §11.7 limitation unchanged |
| CP-08 existing device | still verifies epoch proofs with the VK it holds; rollback → `MANIFEST_ROLLBACK`; fork → `REGISTRY_FORK` |

## 3. Checkpoint construction and threat analysis

- **What it is:** a MAC, not a signature. Only a holder of the current VK
  can produce one — exactly the party the recovering user has just proven
  to be by unwrapping MP/RK. It authenticates *state*, not identity, and
  adds no new key material to store.
- **Provider substitutes a registry:** cannot forge the MAC; the served
  checkpoint binds the real head and manifest core, so any substitution
  mismatches (CP-02).
- **Provider edits history:** changes the registry object's hash (index,
  authenticated by the manifest) and the head (checkpoint). CP-03.
- **Provider replays an old checkpoint with new state, or new checkpoint
  with old state:** every bound field must match simultaneously (CP-04,
  CP-05).
- **Rotation:** the checkpoint key is VK-derived, so a checkpoint made
  before a rotation fails after it, and vice versa (CP-06). This is what
  forces regeneration.
- **Whole-account rollback to an older complete, internally consistent
  state:** still undetectable on a device with no memory (§11.7). The
  checkpoint does not claim to fix that; the printed sheet remains the
  user-held comparison (CP-07).
- **Attacker holding the current VK:** already holds the vault; the
  checkpoint grants nothing further.
- **Existing devices:** unchanged. They keep verifying epoch proofs and
  keep detecting rollback and fork from their persisted head (CP-08).
- **Residual:** a fresh device trusts the current registry on the strength
  of VK possession. An attacker who has the VK could therefore present a
  registry of their choosing to a *new* device — but with the VK they can
  already read and rewrite the vault, so this is not a new capability.

## 4. Deviations absorbed into the normative spec

| Phase D deviation | Now specified as |
|---|---|
| Rotation crash journal | §2.10: staged `*.next` files + one `rotation.commit` marker as the commit point; roll forward/back at open; "re-run rotation from scratch" removed as impossible (the new VK lives only in the staged wraps) |
| Revision-hash remapping | §2.10: `rev_hash` re-derived parents-first and `parent_revs`/tips/conflicts remapped; object keys change at rotation; pre-rotation objects stay in retained generations |
| Backup-object byte format | §3.7: `flags` bit0 = tombstone (other bits reserved, refused); fixed 16-byte trailer `schema_version` u32 + `created_at` u64 + `updated_at` u64; parser recomputes `rev_hash` and refuses key mismatch |
| Object index coverage | §11.2: the index names record objects **plus** header, registry and each wrap (key/sha/size), all content-addressed; `header.json` is part of backed-up state (salts); the checkpoint is the one object never listed |
| RK replacement needs the current MP | §1.5 + §12 scenario 6: the panel collects the current MP because `password.wrap` must be re-sealed under the new VK |
| MP-only total-loss recovery and the RK | §12 scenario 3: the RK wrap can only be re-sealed by a holder of `RK_bytes`; the UI offers to enter the existing RK, otherwise a new RK is issued, shown, and its locator/credential re-registered — no vault keeps a wrap of a retired VK |
| RK-only total-loss recovery and the MP | §12 scenario 4: a new master password must be set during recovery; MP locator/credential and `header.kdf` salt re-registered |
| `change_master_password {mode:"reset"}` | §1.5 op catalog row (UNLOCKED + presence, new+confirm only, no rotation, atomic wrap replacement) |
| Acknowledged RK sheet before setup commits | §1.7 + §5.4: the window is acknowledgement-gated; a dismissed window leaves no vault |
| (new) Registry checkpoint | §4.8, §11.8 body tag 0x0C + validation step 5a, §11.5 fresh-device order, §16.17 CP-01…CP-08 |

**Format versions:** no shipped format existed, so the specification was
corrected to match the implementation rather than the reverse. No version
field needed incrementing: the object format keeps `OV0OBJ01` (its flags
byte and trailer are now specified, not changed), registry entries stay
`entry_version = 2`, the signed manifest stays `version = 1`, and the
finalize body stays `proto = 1` with a new field. The checkpoint is new at
`version = 1`. Phase B cross-language vectors cover the crypto primitives
and were unaffected; no vector needed regeneration (`gen_vectors --check`
is green in the gate's Phase B regression).

## 5. Argon2 calibration and v1 tuple status

Measured with the **production crate and parameters** (`argon2` 0.6,
Argon2id, `Version::V0x13`, `m=64 MiB, t=3, p=1`, 32-byte output —
`crypto::kdf::derive_pk`'s exact call), cross-compiled to
`aarch64-apple-ios` (release + LTO) and run from a throwaway SwiftUI
harness on a real device. Synthetic password and fixed salt. The harness
lives outside the repos and is not committed.

| Device | OS | RAM | Runs | Median | Worst | Budget (§2.3) |
|---|---|---|---|---|---|---|
| iPhone 13 mini (iPhone14,4, A15) | iOS 27.0 | 3674 MB | 7 | **89 ms** | **124 ms** (cold first run) | ≤ 2 s |
| MacBook Air M2 (Mac14,2) | macOS 26.6.2 | — | 3 + warmup | 115 ms | — | ≤ 1 s |

Memory-pressure behavior: the 64 MiB block was allocated and released on
every run, with no memory warning and no jetsam kill on a 4 GB device.
The iOS vault boundary is the **app** (§1.7), not an app extension, so
extension memory limits do not apply.

**Tuple status: unchanged and NOT frozen.** `m=64 MiB, t=3, p=1` stands,
was not weakened, and is ~22× inside the iPhone budget at the median. Per
the owner's decision the freeze waits for a measurement on the **oldest
supported iPhone class (A12 / iPhone XS–XR, the iOS 17 floor)**; an
extrapolation from the A15 is not accepted as evidence. Release-gate item
30 now carries this requirement, and
`docs/security/argon2-calibration.md` records the run in full.

## 6. Print-gate disposition

- v1 keeps the **standard** macOS print dialog; no custom print panel is
  built to remove "Save as PDF" (owner decision, now in §1.7).
- The Recovery Key window carries the required copy verbatim: *"Print to
  paper. Saving as PDF creates an unencrypted copy of your Recovery Key."*
- The honest spool limitation is preserved in §1.7 and in the window copy:
  the macOS print system may retain spooled data outside the helper, and
  no erasure is claimed.
- A real configured-printer exercise is now **release-gate item 29**
  (first-real-credential gate), not a Phase D/E gate item. This
  development Mac has no printer configured; the Phase D gate continues to
  verify the window, the capture bracket, and that no file is written, and
  reports the print leg as not exercised. This does not block Phase E.

## 7. Gate and regressions

`npm run gate:phase-d`, run of record 2026-09-20:

| # | Check | Evidence |
|---|---|---|
| 1 | Phase D tests | **38 passed, 0 failed** (32 + the 8 new CP tests, minus the superseded blocker test) |
| 2 | full helper suite (A–D) | **152 passed, 0 failed** |
| 3 | file-length audit | all files ≤ 350 lines |
| 4 | debug helper build + sign + verify | OU 9RGW34CMA2, deep-strict + DR |
| 5 | release: debug overrides compiled out | none present |
| 6 | signed E2E over the real socket | setup shows RK → RK unlock → add → rotate (`vk_generation` 1→2) → record intact → new RK unlocks |
| 7 | RK / MP absent from IPC | longest BIP-39 word run: 4 in client frames, 1 in helper log (an RK is 24) |
| 8 | UI-04 live RK window | capture-excluded, bracketed, no file; print leg not exercised (no printer — release-gate item 29) |
| 9 | main-app IPC surface lint | no RK/MP-bearing argument |
| 10 | main app + frontend | `cargo check` + `npm run build` green |
| 11 | supply chain | audit + vet green; 96 helper deps (< 120); no rsa; **no dependency change in this pass** |
| 12 | Phase C gate regression | PASS (19) — incl. Phase B PASS (6) with vector freshness, Phase A 11/11, 10 min fuzz clean |

The Phase D report's `second_recovery_blocked_pending_spec_decision` test
is removed: the behavior it pinned is now fixed, and CP-01 asserts the
opposite outcome.

## 8. Remaining blockers to Phase E

1. **None from the repeated-recovery protocol** — the Phase D blocker is
   closed by §4.8 and CP-01…CP-08.
2. **Argon2id freeze is open** (not a Phase E blocker by the owner's
   instruction): needs an A12-class iPhone measurement; the tuple stays
   provisional and unchanged until then.
3. **Printer exercise is open** and deferred to the release gate (item 29).
4. **CS-03 (keystroke suppression)** remains vacuous until macOS keystroke
   capture exists; re-verify when it lands (carried from Phase C).
5. Phase E still requires separate authorization; nothing in it was
   started here.
