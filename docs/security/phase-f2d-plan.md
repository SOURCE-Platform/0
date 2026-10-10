# Phase F.2d — phone authority: plan

Date: 2026-10-10. Spec: v0.5 §22.17 milestone 4 ("publication, rotation,
revocation (RC-01), total-loss recovery on a phone"), §2.7 (master-password
adoption), §22.5 (tiers), §22.7 (freshness), §22.9 (lifting the removal
lock, `BACKUP_ACCESS_LOST`), §22.10 (iPhone rules: staged-first rotation,
background task), and the items earlier milestones deferred here
(phase-f2-verification §5–§9). Internal working plan; each step goes
through the usual review loop. **Step 2 changes the FFI catalogue, which
is a spec change, so its design below is reviewed before any of its code
is written.**

## Steps

1. **Master-password adoption (done in code, 2026-10-10).**

   *Why first:* it must land before any other device can publish (§2.7,
   owner decision 2026-10-02).

   *How it works:*
   - `sync::adopt_mp` opens the served state's `wrap_mp`. The wrap is
     hash-checked against the signed index, and the apply verifies the
     whole state under the key it yields.
   - `vault::adopt_prompt::pre_open` asks for the master password in the
     secure panel when this device's envelope answers
     `DEVICE_NOT_AUTHORIZED` (a discarded agreement key, or Touch ID
     unavailable). It does this only for a state signed by our registry,
     and outside the core mutex.
   - The §2.7 stated limitation is lifted.

   *Test:* `vault-tests/tests/mp_adoption.rs`. A password-only Mac,
   restored to before its own rotation, catches up; a wrong password
   adopts nothing.

2. **The phone's provider path (design for review).** The phone needs its
   own backup-service exchanges to:
   - verify a provider state (committed tier, §22.5);
   - be "fresh" (§22.7);
   - publish its own edits;
   - lift a removal lock (§22.9);
   - see `BACKUP_ACCESS_LOST`.

   On the Mac this is `vault-coordinator`: pure logic over two traits —
   `Helper` (one engine op) and `Transport` (one HTTPS request) — holding
   no secret. Proposal:
   - Link `vault-coordinator` into `vault-ffi` on the phone, with
     `Helper` = the engine's own dispatcher (in-process, as `vault-tests`
     already does).
   - Add **one callback**, `http_send(origin, method, path, auth, body) →
     (status, body, date)`, to `Ov0Callbacks`. Swift implements it with
     URLSession and standard certificate validation — no pinning, as on
     the Mac (§11.1).
   - Add **ops** `provider_sync` and `provider_publish` (and `provider_run`
     for a staged publication), each running one coordinator flow on the
     engine's FFI thread. No new exported symbol (FFI-01 unchanged); the
     catalogue gains one callback and three ops.
   - The coordinator still sees no plaintext, MP, PK, RK, VK or private
     key (its §11.1 contract). Signing stays inside the engine (the
     `sign_provider_request` op).
   - The provider origin comes from the vault itself (the header's
     allowlisted origin) and is never taken from Swift.

   Alternatives rejected:
   - Porting the coordinator to Swift: a second implementation of the
     flows.
   - Swift calling the existing ops one by one: it would carry the
     coordinator's state machine.

3. **Using the provider path.**
   - Freshness on the phone (§22.7). With it, the phone pushes its
     local-only revisions (`peer_revs_put`, deferred from F.2c).
   - `peer_state` from the phone (§22.5 provisional, deferred from F.2c).
   - A provider-confirmed state in which the phone is still active lifts
     the removal lock (§22.9).
   - The `BACKUP_ACCESS_LOST` copy: "removal pending" becomes "this iPhone
     was removed".
   - Triggers: unlock and "Sync now", never in the background (§4.7).

4. **Publication from the phone.** The phone's own edits are published as
   on the Mac (`backup_prepare` … `backup_commit_result`, through the
   coordinator). The phone's UI gains add and edit for logins (synthetic
   data only until Phase J).

5. **Authority on the phone.**
   - `rotate_recovery_key` in its §22.10 staged-first form: the rotation
     is staged before the sheet, in a background task, and the sheet says
     whether the key is live (IO-04). The op returns to `IOS_OPS` only in
     this form (SEC-B2 of F.2b).
   - Revocation of another device from the phone (RC-01).
   - Total-loss recovery on a phone (with the Recovery Key or the master
     password, as on the Mac).
   - The owner decision "iPhone adds replacement Mac" depends on reverse
     enrollment (F.2e, §22.13), so it is not in this milestone.

6. **"Remove this vault"** (§22.9). An explicit action that deletes the
   phone's keys and store. It warns that this may be the only remaining
   copy until `BACKUP_ACCESS_LOST` corroborates the removal. It also
   covers the ACK-lost path from F.2b (VER-O1).

7. **Carried items:**
   - a cached LOCKED store open for peer serving;
   - the header refresh and items-changed event after a peer put;
   - the RK sheet copy says "this iPhone" on the phone;
   - SEC-O3 / SEC-O4 of §5.2.

## Owner-facing consequence

From step 2 on, the phone needs network access to the backup service.
The real service is owner infrastructure and is not configured yet. All
tests use the in-process provider core, and a device run of steps 2–6
waits for the owner's backup service.

## Review plan

- Step 1: security + verification review.
- Step 2: spec + security review of this design before code; then the
  usual implementation review.
- Steps 3–7: each with its review loop.
