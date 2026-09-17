# O / Source Credential Vault — Implementation Specification

**Status:** Implementation specification v0.1 — pre-implementation. Nothing in
this document is implemented authorization.
**Canonical architecture:** `docs/security/credential-vault-security-architecture.md`
v0.3. Where this specification and v0.3 conflict, v0.3 controls and this
document must be corrected — except where this document records a genuine
platform constraint (§21), which requires owner decision before proceeding.
**Companion documents:** `docs/security/macos-signing-and-hardening.md`
(signing identities, designated requirements), plus the hardening mechanisms
already landed in this repository (capture exclusions, asset scope, CSP,
audit/vet) referenced in §14.
**Scope rule:** a separate implementation agent must be able to build the
system from this document without making new security architecture decisions.
Where this document is silent on a security-relevant choice, the correct
action is to stop and ask, not to choose.

**Terminology:** MP = master password, PK = password-derived key, RK =
recovery key, VK = vault key, SE = Secure Enclave, LA = LocalAuthentication,
HPKE = RFC 9180, AAD = AEAD additional authenticated data.

**Document conventions:**

- All byte strings in schemas are lowercase hex unless stated otherwise.
- All integers are unsigned, decimal in text form, big-endian in binary form.
- `uuid` = RFC 4122 UUIDv4, lowercase, hyphenated, random.
- Timestamps are Unix seconds unless stated otherwise.
- "TLV" refers to the canonical binary encoding defined in §4.2.
- Wire formats are versioned; any change requires a version bump and new
  cross-language test vectors.

---

## 1. Module and process architecture

### 1.1 Processes and binaries

v1 ships three signed executables, all inside `SOURCE.app`, all signed with
the team identity `9RGW34CMA2` defined in
`docs/security/macos-signing-and-hardening.md`:

| Binary | Bundle identifier | Role |
|---|---|---|
| `SOURCE.app` (existing Tauri app) | `com.racker.zero` | Main process: UI, capture, mobile server, backup client |
| `SOURCE.app/Contents/Library/vault-helper/SourceVaultHelper.app` | `com.racker.zero.vault-helper` | Vault core: keys, storage, policy |
| `SOURCE.app/Contents/Library/vault-nm-host/source-vault-nm-host` | `com.racker.zero.nm-host` | Chrome native-messaging broker (thin client of the helper) |

The two helpers are **separate Cargo packages** in a new Cargo workspace
(`src-tauri/vault-helper/`, `src-tauri/vault-nm-host/`), not modules of
`zero_lib`. This keeps their dependency graphs physically distinct from the
~500-crate main graph and makes `cargo tree -p source-vault-helper` a
meaningful audit gate.

**Helper bundle shape.** `SourceVaultHelper.app` is a proper `.app` bundle
with `LSUIElement = true` (no Dock icon, no menu bar) so that
LocalAuthentication prompts are attributed to a stable, signable bundle
identity. It is launched by the main app through LaunchServices
(`NSWorkspace`/`open -g --hide`), never `posix_spawn` of a bare executable,
so it becomes its own TCC/LA responsible process. It carries **no**
entitlements except `com.apple.security.cs.disable-library-validation` =
**absent** (v0.3 §15.3 requirement: the helper must not carry it) and no
sandbox entitlement (it is not sandboxed; it needs Keychain/SE access under
the user's existing grants).

The helper contains no Source functionality: no capture, no OCR, no mobile
server, no agent code, no FFmpeg/Tesseract, no HTTP client, no WebView. It
links no C libraries. Target dependency count is in §17.

### 1.2 Responsibility matrix

| Responsibility | Main process | Vault helper | nm-host |
|---|---|---|---|
| Vault Key residency | never | yes (unlocked only) | never |
| Master password / RK handling | UI field only, transient, passed once over IPC, zeroized | derives/uses, zeroizes | never |
| Encrypted vault storage (`vault.db`, wraps, registry, manifest) | never opens | exclusive owner | never |
| Cryptography (wraps, records, envelopes, signatures) | none | all | none |
| Device registry verification | none | all | none |
| Origin canonicalization / match policy | none | all | sends raw origin only |
| User-presence decisions (LA, iPhone approval) | none | all | none |
| Network access (mobile server, backup client, APNs relay client) | yes | **never** | no |
| Browser extension contact | no | no (via nm-host) | yes |
| iPhone protocol contact | yes (relays opaque frames) | verifies/signs | no |
| Capture suppression registration | yes (existing commands) | checks state via IPC before display-class releases | no |
| Vault management UI (item list, settings) | yes | serves metadata over IPC | no |

The main process must **not** become a general-purpose secret retrieval
layer: it never receives record passwords except the single approved fill,
and even that path is helper → nm-host → Chrome, bypassing the main process
entirely (§9). The main process's own vault UI receives metadata (titles,
usernames, hosts) only, after unlock.

### 1.3 IPC transport

- **Transport:** AF_UNIX `SOCK_STREAM` socket at
  `$HOME/.observer_data/vault/helper.sock`. Parent directory mode `0700`
  (already created by `core/vault_dir.rs`), socket file mode `0600`.
  Same-UID processes outside the signed set are additionally rejected by
  peer authentication (below).
- **Framing:** 4-byte big-endian length prefix + JSON frame. Maximum frame
  64 KiB; a larger declared length closes the connection. One request →
  one response; the helper may emit unsolicited `event` frames on the main
  app's connection only.
- **Protocol version:** `hello`/`hello_ok` handshake carries `proto: 1`.
  Mismatched major version → connection closed.
- **Concurrency:** the helper accepts at most one connection per client
  class (`app`, `nm-host`); a second connection of the same class replaces
  and closes the first (guards against a wedged client holding the slot).

### 1.4 Helper peer authentication

Both directions authenticate via code identity, per the signing model in
`docs/security/macos-signing-and-hardening.md`:

1. **Before launch**, the main app statically verifies the helper bundle:
   `SecStaticCodeCreateWithPath` + `SecStaticCodeCheckValidityWithErrors`
   against designated requirement
   `anchor apple generic and certificate leaf[subject.OU] = "9RGW34CMA2" and identifier "com.racker.zero.vault-helper"`.
   Failure → helper is not launched; vault stays unavailable; event logged.
2. **On connection accept**, the helper obtains the peer pid
   (`getsockopt(LOCAL_PEEREPID)`), then
   `SecCodeCopyGuestWithAttributes(kSecGuestAttributePid)` +
   `SecCodeCheckValidityWithErrors` against:

   ```text
   anchor apple generic and certificate leaf[subject.OU] = "9RGW34CMA2"
     and (identifier "com.racker.zero" or identifier "com.racker.zero.nm-host")
   ```

   (Debug builds may relax to same-team, never to "any process".)
3. **Reverse direction:** the main app applies the same pid→SecCode check
   to the socket's accepted peer identity after connecting, requiring
   `identifier "com.racker.zero.vault-helper"`.
4. **PID-reuse mitigation (honest residual risk):** the check runs once at
   connect time and the connection is then pinned — a verified connection
   that drops is never "resumed"; a new connection re-verifies. A local
   attacker who can already inject code into the main process is outside
   this boundary (v0.3 §17). This is the accepted cost of socket-based IPC
   without literal XPC, as authorized by the v0.3 §15.3 clarification.
5. `getpeereid` UID equality is checked as a cheap first gate before the
   SecCode call.

### 1.5 IPC message catalog

All frames are JSON objects with an `op` or `event` discriminator.
`ref` = record identifier (§3.3). Every response carries
`{"ok": bool, "error": <code from §15> | null}` plus op-specific fields.
No error frame ever carries secret material, only codes and
human-safe context.

**Main app → helper (`app` class):**

| op | Purpose | State required | Notes |
|---|---|---|---|
| `hello` | handshake | any | `{proto, client:"app"}` |
| `get_state` | state machine state | any | |
| `unlock` | unlock via LA presence or phone approval | LOCKED | never carries MP for routine unlock |
| `unlock_with_recovery` | MP or RK entry (setup/recovery/fallback) | LOCKED/RECOVERING | `{kind:"mp"\|"rk", secret}` — secret zeroized by both sides after use |
| `lock` | immediate lock | any | |
| `list_items` | metadata list | UNLOCKED | `[{ref, kind, title, username, hosts}]`, no secrets |
| `add_item` / `update_item` | create/modify record | UNLOCKED + fresh presence | full record crosses IPC once |
| `delete_item` | tombstone record | UNLOCKED + fresh presence | |
| `reveal` | show one password in capture-suppressed UI | UNLOCKED + fresh presence + capture check | one-shot, §14 fail-closed |
| `change_master_password` | re-wrap VK under new PK | UNLOCKED + fresh presence | old + new MP cross once |
| `rotate_recovery_key` | new RK + VK rotation | UNLOCKED + fresh presence | returns new RK words once for printing |
| `list_devices` / `revoke_device` | registry view / revocation | UNLOCKED + fresh presence | revocation triggers VK rotation |
| `begin_enrollment` | start §5 flow | UNLOCKED + fresh presence | returns QR payload for rendering |
| `relay_to_device` / `relay_from_device` | opaque vault-protocol frames for iPhone | any | main is a dumb pipe (§5, §6) |
| `backup_snapshot_prepare` | produce encrypted objects + signed manifest | UNLOCKED | main then uploads (§11) |
| `backup_state_apply` | verify + import downloaded state | UNLOCKED/RECOVERING | rollback/fork checked in helper |
| `import_dashlane` | §10 import from user-picked path | UNLOCKED | helper opens the file itself |
| `approval_result` | deliver signed iPhone approval | AUTHORIZING | §6.5 |

**nm-host → helper (`nm-host` class):**

| op | Purpose | Notes |
|---|---|---|
| `hello` | `{proto, client:"nm-host"}` | |
| `fill_candidates` | `{origin, tab_url}` → `{request_id, accounts:[{ref,title,username}]}` or `{locked:true}` | no secrets; origin checked against tab_url (§9.4) |
| `fill_authorize` | `{request_id, ref, method:"local"\|"iphone"}` → presence → `{username, password, expires_in}` | one-shot; consumed on response |
| `save_new` | `{origin, username, password, title}` → `{ref}` | requires UNLOCKED |
| `save_update` | `{ref, password}` | requires UNLOCKED |

**Helper → main app events:** `state` (state transitions), `locked`,
`approval_requested` (main relays to iPhone, §6.5), `enrollment_progress`,
`backup_progress`, `registry_changed`, `capture_unsafe` (a display-class
release was refused).

**Never across IPC, in either direction, in any op:** VK, PK, device private
keys, bulk record export, password-history dumps, decrypted-notes search,
any "dump all" operation, any op returning more than one record's secret
fields. There is intentionally **no** `export_vault` op in v1.

### 1.6 Process lifecycle

- **Startup:** main app launch → verify + launch helper → connect →
  `hello`. Helper starts in `LOCKED` (or `UNINITIALIZED` on a machine with
  no vault). Unlock is user-initiated via the app UI or implicitly on first
  fill request.
- **Shutdown:** app quit → main sends `lock` → helper zeroizes and exits
  after all clients disconnect (5 s grace). The helper also exits after
  30 min with zero clients.
- **Helper crash:** VK dies with the process. Main detects the socket
  close, marks vault LOCKED, relaunches the helper once automatically;
  further crashes within 5 min → error surfaced, no restart loop.
- **Main app crash:** helper stays resident and retains its lock state;
  on reconnect the main app re-runs `hello` and reads `get_state`.
  Security note: an UNLOCKED helper surviving a main-app crash keeps VK
  resident until the idle timeout — the 15-minute auto-lock bounds this.
- **Auto-lock triggers (helper-side):** 15 min since last authorization
  (configurable 5–60), system sleep, screen lock / session switch
  (distributed notifications), explicit `lock`, `COMPROMISED` entry.
- **Lock behavior after helper restart:** always LOCKED; unlock requires
  the §6 user-presence path (device envelope unwrap is itself ACL-gated,
  §2.8).

---

## 2. Cryptographic data model

No custom primitives. All constructions below are named standards composed
in the stated way; §16.8 defines shared Rust↔Swift test vectors for every
composition.

### 2.1 Primitives

| Use | Primitive | Implementation |
|---|---|---|
| Password KDF | Argon2id (RFC 9106) | RustCrypto `argon2` |
| Record/wrap/metadata AEAD | XChaCha20-Poly1305 (RFC 8439 + XChaCha draft) | RustCrypto `chacha20poly1305` |
| Subkey derivation / KEK derivation | HKDF-SHA-256 (RFC 5869) | RustCrypto `hkdf` + `sha2` |
| Device signatures | ECDSA over P-256 with SHA-256, deterministic (RFC 6979), low-S enforced | RustCrypto `p256` / Apple SE + CryptoKit |
| Device key agreement envelopes | HPKE base mode: DHKEM(P-256, HKDF-SHA-256), HKDF-SHA-256, ChaCha20-Poly1305 (RFC 9180) | `hpke` crate / CryptoKit HPKE (iOS 17+) |
| Hashes | SHA-256 | `sha2` / CryptoKit |
| RNG | OS CSPRNG | `rand::rngs::OsRng` / `SecRandomCopyBytes` |
| Constant-time compare | `subtle` / manual | tokens, secrets, SAS |

### 2.2 Key hierarchy

```text
                        Vault Key (VK, 32 bytes, OsRng)
                ┌───────────────┼────────────────────┐
                ▼               ▼                    ▼
        password.wrap    recovery.wrap        devices/<id>.wrap
        key: HKDF(PK)    key: HKDF(RK)        HPKE-sealed to device
                                              agreement public key

  PK  = Argon2id(MP, kdf_salt, params)          (32 bytes, transient)
  RK  = 32 bytes OsRng, shown as BIP-39 words   (never stored by Source)
```

Every wrap seals the same **WrapPayload**:

```text
WrapPayload = {
  vk:                32 bytes,
  backup_auth:       32 bytes,   // §11.4 backup request authentication
  wrapped_at:        u64,        // unix seconds
  vk_generation:     u32         // matches header.json
}
```

`backup_auth` rides every wrap so that any recovery path (MP, RK, or device
envelope) also restores the ability to authenticate to the backup service
without that service ever seeing MP/RK/VK.

### 2.3 Argon2id parameters and upgrade path

- v1 parameters: `m = 64 MiB`, `t = 3`, `p = 1`, output 32 bytes, random
  16-byte `kdf_salt` (OWASP/RFC 9106 "first recommended" class, calibrated
  on the slowest supported device during Phase B; target ≤ 1 s on a 2020
  MacBook Air, ≤ 2 s on the oldest supported iPhone).
- Storage: `header.json` carries `{kdf: "argon2id", kdf_version: 1, m, t,
  p, salt}`. `password.wrap` carries a copy of the same parameter block.
- Upgrade: parameter changes bump `kdf_version`; the next successful MP
  unwrap re-derives PK with new parameters and rewrites `password.wrap`.
  Old parameter sets remain readable until re-wrapped. There is no
  downgrade: the helper refuses `kdf_version` lower than the highest it
  has ever seen for this vault (persisted in `header.json`).

### 2.4 Recovery Key representation

- RK = 32 bytes from OsRng.
- Display/entry encoding: BIP-39 English mnemonic, 24 words
  (256 bits entropy + 8-bit checksum). The BIP-39 encode/decode is
  **implemented in-house** (~100 lines: 11-bit packing over the official
  2048-word English list, SHA-256 checksum) in both Rust and Swift, checked
  against the published reference test vectors — this avoids a
  poorly-audited third-party mnemonic crate in the helper (§17).
- Entry normalisation: lowercase, trim, collapse whitespace, validate
  wordlist membership + checksum before use. Wrong words → §15 error
  `RECOVERY_KEY_INVALID`, no oracle beyond "not a valid recovery key".
- RK usage: `RK_bytes = mnemonic_decode(words)` (the 32-byte entropy,
  not a PBKDF2 expansion — BIP-39 seed stretching is not used; the entropy
  is already 256 bits).

### 2.5 Wrap constructions

```text
password.wrap (JSON):
{ "v":1, "kind":"mp", "kdf_version":1, "argon2id":{"m":65536,"t":3,"p":1,
  "salt":hex16}, "nonce":hex24, "ct":hex }

  wrap_key = HKDF-SHA256(ikm=PK, salt=argon2id.salt,
                         info="ov0/wrap/mp/v1")           // 32 bytes
  ct = XChaCha20-Poly1305-Seal(wrap_key, nonce=random24,
       plaintext=WrapPayload, aad="ov0/wrap" || vault_id || "mp")

recovery.wrap: identical shape, kind="rk",
  wrap_key = HKDF-SHA256(ikm=RK_bytes, salt=random16-stored,
                         info="ov0/wrap/rk/v1")
  aad = "ov0/wrap" || vault_id || "rk"
```

- Nonces: 24 bytes, OsRng, per seal operation. 192-bit random nonces make
  accidental reuse negligible at vault scale (≤ 2^20 seals).
- Wrong MP/RK produces an AEAD tag failure → `WRONG_CREDENTIAL`, fail
  closed, no partial state. There is no password-verifier oracle stored
  anywhere.

### 2.6 Record and metadata encryption

```text
record_key  = HKDF-SHA256(ikm=VK, salt=record_id_bytes,
                          info="ov0/record/v1")          // per-record subkey
record_ct   = XChaCha20-Poly1305-Seal(record_key, nonce=random24,
              plaintext=record JSON (§8), aad=record_aad)
record_aad  = "ov0/record" || vault_id || record_id_bytes
              || u32be(schema_version) || u32be(vk_generation)

meta_key    = HKDF-SHA256(ikm=VK, salt=header.meta_salt,
                          info="ov0/meta/v1")
meta_ct     = Seal(meta_key, nonce=random24, plaintext=metadata JSON,
              aad="ov0/meta" || vault_id || record_id_bytes || field_tag)
```

Per-record subkeys mean a hypothetical future single-record key exposure
does not cascade. Moving a record's ciphertext to another `record_id`
fails AAD. Re-encryption at VK rotation rewrites `vk_generation` and all
nonces.

### 2.7 Device identity keys

- **Signing key:** P-256 ECDSA, SE-generated (`kSecAttrTokenIDSecureEnclave`,
  `SecKeyCreateRandomKey`, `privateKeyUsage` sign). Non-exportable.
- **Agreement key:** P-256 ECDH, SE-generated, distinct key. Used only via
  HPKE DHKEM decapsulation (`SecKeyCopyKeyExchangeResult` on the SE key).
- Never one key for both roles (v0.3 C14).
- SE access control: `.privateKeyUsage` + `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`;
  the agreement key adds no biometric ACL of its own (presence is enforced
  explicitly by LA at the operation level, §6, so prompts are uniform and
  testable across devices).
- Public keys on the wire: ANSI X9.63 compressed points (33 bytes).
- Signatures on the wire: fixed 64-byte `r || s`, low-S required;
  verifiers reject high-S (malleability).

### 2.8 Keychain usage

| Item | Class | ACL | Purpose |
|---|---|---|---|
| `com.racker.zero.vault.state` | generic password, data blob | `WhenUnlockedThisDeviceOnly` | helper runtime bookkeeping (last manifest generation/head hash seen — rollback evidence) |
| SE keys (both) | key class | §2.7 | device identity |
| `com.racker.zero.vault.helper-prefs` | generic password | `WhenUnlockedThisDeviceOnly` | auto-lock minutes, non-secret prefs |

The Keychain never stores VK, WrapPayload, MP, or RK. Unlock uses the
device envelope: HPKE-decapsulate `devices/<self>.wrap` with the SE
agreement key after an LA presence check. If the SE key is missing
(device restored from backup, key wiped), the device must re-enroll or
recover — documented behavior, not an error.

### 2.9 HKDF context registry

| info string | Derives |
|---|---|
| `ov0/wrap/mp/v1` | MP wrap key from PK |
| `ov0/wrap/rk/v1` | RK wrap key from RK |
| `ov0/record/v1` | per-record key from VK |
| `ov0/meta/v1` | metadata key from VK |
| `ov0/recovery-auth/v1` | registry recovery-epoch proof key from VK (§4.5) |
| `ov0/locate/mp/v1`, `ov0/locate/rk/v1` | recovery locator keys from PK / RK-derived key (§12 scenario 3) |
| `ov0/enroll/sas/v1` | SAS display bytes from enrollment transcript |
| `ov0/approval/…` | not used — approvals are plain ECDSA over TLV (§6.5) |

HPKE `info` strings (envelopes): `"ov0/envelope/v1" || vault_id ||
new_device_id || enrollment_nonce`.

### 2.10 Versioning and rotation

- `header.json` fields: `vault_id` (uuid, random at creation), `version`,
  `kdf*`, `meta_salt`, `vk_generation`, `registry_head`, `manifest_generation`.
- Every record stores the `vk_generation` it was sealed under.
- Rotation (§12): new VK → `vk_generation += 1` → all records re-sealed →
  all wraps rewritten → new manifest generation → old VK zeroized.
  Rotation is atomic at the manifest flip: records are re-encrypted in a
  SQLite transaction; a crash mid-rotation leaves the old manifest
  pointing at old records (still valid), and the next launch detects
  `vk_generation` mismatch between header and manifest and re-runs
  rotation from scratch (idempotent).
- Old VK/new state: AEAD failure everywhere; there is no fallback path.

### 2.11 Memory-lifetime rules

- All secret buffers (`VK`, `PK`, `RK_bytes`, `WrapPayload`, record
  plaintext, `backup_auth`) live in `zeroize::Zeroizing`/`secrecy` types.
- Transient secrets cross a trust hop exactly once: derive → use →
  zeroize. The helper never caches MP/PK/RK beyond the call that used
  them (state machine §13 reflects this).
- The helper calls `setrlimit(RLIMIT_CORE, 0)` at startup and
  `mlock` on the VK buffer (best-effort; documented as non-guarantee,
  v0.3 §17).
- nm-host zeroizes fill responses after writing them to stdout; the
  Chrome extension nulls references after fill (JS cannot guarantee
  erasure — documented limitation, §9.9).
- Panic behavior: the helper does not catch panics around secret-bearing
  code; process death is the zeroization of last resort. Release builds
  compile the helper with `panic = "abort"`, `debug = false`, and no
  secret-bearing type implements `Debug`.

---

## 3. On-disk vault format

### 3.1 Layout and permissions

```text
~/.observer_data/vault/                 0700  (created by core/vault_dir.rs,
│                                             .metadata_never_index present)
├── helper.sock                       0600  IPC endpoint (§1.3)
├── header.json                       0600  §2.10 fields
├── vault.db                          0600  SQLite (WAL), records + tombstones
├── vault.db-wal / vault.db-shm       0600  never leaves this directory
├── manifest.json                     0600  signed state head (§11.3)
├── registry.json                     0600  append-only device log (§4)
├── wraps/
│   ├── password.wrap                 0600  §2.5
│   ├── recovery.wrap                 0600  §2.5
│   └── devices/<device_id>.wrap      0600  HPKE envelope per device
└── import/                           0700  transient; empty between imports
```

Only the helper opens anything under this directory. The main process
creates the directory (already landed) and knows paths for housekeeping,
but never opens `vault.db`, wraps, registry, or manifest. The directory is
structurally excluded from Source's asset protocol (config deny rule +
`core/asset_scope_guard.rs` tests + runtime probe), from capture/indexing
search roots, and from timeline/export paths (§14).

### 3.2 SQLite schema (vault.db, `user_version = 1`)

```sql
CREATE TABLE records (
  record_id     TEXT PRIMARY KEY,   -- uuid
  kind_tag      INTEGER NOT NULL,   -- 1=login, 2=card (plaintext; §3.4)
  vk_generation INTEGER NOT NULL,
  schema_version INTEGER NOT NULL,
  nonce         BLOB NOT NULL CHECK(length(nonce)=24),
  ct            BLOB NOT NULL,      -- record JSON under record_key
  meta_nonce    BLOB NOT NULL CHECK(length(meta_nonce)=24),
  meta_ct       BLOB NOT NULL,      -- metadata JSON under meta_key
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE TABLE tombstones (
  record_id     TEXT PRIMARY KEY,
  deleted_at    INTEGER NOT NULL,
  vk_generation INTEGER NOT NULL    -- generation that processed the delete
);
CREATE TABLE kv (                   -- helper-internal non-secret bookkeeping
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
```

`PRAGMA secure_delete = ON`. The DB file never leaves the device except as
per-record ciphertext objects in backups (§11); the DB itself is not
uploaded.

### 3.3 Record identifiers

Random UUIDv4, generated at creation, stable for the record's life,
unrelated to content. Nothing content-derived appears in `record_id`;
backup object keys embed it (§11), which leaks only existence/size to the
provider — accepted and documented in v0.3 F15.

### 3.4 Plaintext vs encrypted fields

| Field | Where | Plaintext? | Why |
|---|---|---|---|
| `record_id` | db, backup keys | yes | needed for sync/backup addressing; random, unlinkable |
| `kind_tag` | db | yes | chooser/list rendering without full metadata decrypt; leaks login-vs-card only |
| `created_at`, `updated_at` | db | yes | sync conflict resolution + UI sorting; second precision is required for merge correctness |
| ciphertext sizes | db/backup | yes | unavoidable; documented metadata leak |
| title, username, hosts/URLs, notes | `meta_ct` / `ct` | **no** | metadata minimization (v0.3 §7) |
| password, card fields | `ct` | **no** | — |
| `vault_id`, `vk_generation`, KDF params | header.json | yes | required for unwrap; not secret |
| device public keys, names | registry.json | yes | required for verification |
| vault item count | manifest | yes | provider metadata leak, accepted |

### 3.5 Versioning and migration

- `header.json.version` governs the directory format; SQLite
  `user_version` governs the schema. Migrations are forward-only,
  hand-written, each wrapped in a transaction; unknown newer versions →
  fail closed (`FORMAT_TOO_NEW`) — never silently open.
- `manifest.json` (local copy) mirrors the §11.3 signed manifest: the
  helper refuses to open a vault whose local records don't match the
  manifest's object list (`MANIFEST_MISMATCH`), forcing the
  restore/verify path instead of trusting a swapped directory.

### 3.6 Corruption handling

- Open sequence: parse header → `PRAGMA integrity_check` → load manifest →
  verify manifest signature + registry head → spot-verify zero records
  (lazy: per-record AEAD failure surfaces at use as `RECORD_CORRUPT`).
- AEAD tag failure on a record → that record is quarantined (read as
  corrupt, never partially returned), error surfaced, record restorable
  from backup/peer copy. Tampering never yields plaintext.
- `vault.db` unrecoverable → ERROR state, user offered "restore from
  backup" / "restore from peer device" (§12); the helper never deletes
  the file automatically.

### 3.7 Backup-compatible serialization

Each record's backup object is exactly its `(nonce || ct || meta_nonce ||
meta_ct)` concatenation plus a 16-byte object header
`{"ov0obj", u16 version, u16 kind_tag, u32 vk_generation, u64 updated_at}`
— all fields already plaintext-classified. The backup store therefore
needs no re-encryption and no transformation; the helper emits objects
byte-identical between local storage and upload.

---

## 4. Device registry

### 4.1 Model

`registry.json` is an append-only log of entries, one JSON object per
line (JSONL, `\n`-terminated). The canonical form for hashing and signing
is the TLV encoding in §4.2 — the JSONL is a storage/inspection
representation; two files are equivalent iff their TLV decodings are
identical.

Entry kinds: `genesis` (first device, self-signed), `enroll`, `revoke`,
`recovery_epoch` (§4.5).

### 4.2 Canonical binary encoding (TLV)

Deterministic encoding used for signatures, hashes, and approval payloads
(§6.5). JSON canonicalization (JCS) is deliberately **not** used:
float/Unicode edge cases across Rust/Swift/JS are a known footgun, and a
fixed-schema binary encoding is trivial to implement identically in all
three languages.

```text
Document  := Tag(0x00) Len(u32be) Value(Entry)*      -- outer wrapper
Entry     := (Field)*  EndOfEntry(0xFF)
Field     := Tag(u8) Len(u32be) Value
  - tags are emitted in strictly ascending numeric order
  - Len counts Value bytes only
  - strings: UTF-8, NFC-normalized, no truncation at encode; decoders
    enforce documented max lengths
  - integers: minimal-length big-endian (no leading zero bytes)
  - byte strings: raw
  - no floats, no booleans (use u8 0/1), no optional absence ambiguity:
    a field is present or absent, never empty-with-meaning
```

### 4.3 Registry entry fields (tags)

| Tag | Field | Type | Present in |
|---|---|---|---|
| 0x01 | `entry_version` | u32 (=1) | all |
| 0x02 | `seq` | u64, monotonic | all |
| 0x03 | `prev_hash` | 32 B | all (genesis: 32 zero bytes) |
| 0x04 | `epoch` | u64 | all |
| 0x05 | `kind` | u8: 1 genesis, 2 enroll, 3 revoke, 4 recovery_epoch | all |
| 0x06 | `device_id` | 16 B uuid | genesis/enroll/revoke |
| 0x07 | `device_name` | string ≤ 64 chars | genesis/enroll |
| 0x08 | `platform` | u8: 1 macos, 2 ios | genesis/enroll |
| 0x09 | `sign_pub` | 33 B compressed P-256 | genesis/enroll |
| 0x0A | `agree_pub` | 33 B compressed P-256 | genesis/enroll |
| 0x0B | `enrolled_at` | u64 | genesis/enroll |
| 0x0C | `authorizer` | 16 B device_id, or 16 zero bytes for recovery_epoch | all |
| 0x0D | `revoked_at` | u64 | revoke |
| 0x0E | `recovery_proof` | 32 B (§4.5) | recovery_epoch |
| 0x0F | `manifest_hash` | 32 B, head manifest at entry time | recovery_epoch |
| 0x10 | `signature` | 64 B r‖s | all |

```text
entry_hash = SHA-256("ov0/registry/entry/v1" || tlv(Entry))   -- includes signature
sign_input = SHA-256("ov0/registry/sign/v1"  || tlv(Entry without field 0x10))
```

### 4.4 Validity rules

A registry state is **valid** iff all hold:

1. seq starts at 0 and increments by exactly 1; no gaps (truncation).
2. Every `prev_hash` equals the previous entry's `entry_hash` (genesis:
   zeros).
3. Every `signature` verifies with the `sign_pub` of the `authorizer`
   device, which must be enrolled and not revoked at that `seq`.
4. Genesis: `authorizer` == own `device_id`; self-signature must verify.
5. A revoked device appears in exactly one `revoke` entry; no enroll after
   revoke for the same `device_id` (re-enrollment = new device identity).
6. `recovery_epoch` entries carry a valid `recovery_proof` (§4.5) and bump
   `epoch`; entries after it must authorise from devices enrolled in the
   new epoch.
7. Exactly one tip: two distinct entries with the same `prev_hash` → fork
   (§4.6).

### 4.5 Recovery-epoch authorization

When no trusted device survives, the recovering device proves possession of
the vault itself (v0.3 §10.3: standard primitives, no Source escrow, MP-only
must work):

```text
recovery_key  = HKDF-SHA256(ikm=VK, salt=manifest_hash,
                            info="ov0/recovery-auth/v1")
recovery_proof = HMAC-SHA256(recovery_key,
                  "ov0/registry/recovery/v1" || tlv(entry without 0x0E/0x10))
```

- Only a party that unwrapped the current VK (via MP, RK, or a surviving
  device) can produce this proof for the manifest it binds to.
- The new device's own `enroll` entry immediately follows at the new epoch;
  the recovering device then rotates VK (§12 scenario 3/4) so the proof key
  is retired with the old generation.
- An attacker holding an *old* VK/RK can forge an epoch bound only to the
  *old* `manifest_hash`; devices that have seen a newer manifest reject it
  (§4.6 rollback), and the recovery UI shows the bound manifest generation
  so a stale-epoch fork is visible to the user.

### 4.6 Fork, truncation, rollback

- **Fork:** two tips sharing `prev_hash`, or a downloaded registry whose
  chain diverges from the local one at any seq ≤ local head. Behavior:
  helper enters `COMPROMISED`-class error state (`REGISTRY_FORK`), refuses
  writes, surfaces both tips to the user (device names, dates, seqs).
  Never auto-resolve.
- **Truncation:** chain gap, bad `prev_hash`, or a presented head with
  seq < the seq this device has already accepted → reject, `REGISTRY_TRUNCATED`.
- **Rollback:** signed manifest with `generation` lower than the helper's
  persisted last-seen generation (Keychain state item, §2.8) → reject,
  `MANIFEST_ROLLBACK`. Devices persist every accepted head before applying
  state derived from it.
- **Conflict handling summary:** any ambiguity → stop, surface, let the
  user choose. The backup provider cannot authorise a device under any
  rule in this section: it holds no enrolled signing key and no VK.

### 4.7 Registry replication

The registry is public verification state: it is uploaded with every
backup manifest (§11) and exchanged during peer sync. Its confidentiality
requirement is nil; its integrity requirement is total.

---

## 5. Mac ↔ iPhone enrollment protocol

Reuses the proven Source pairing UX (`core/mobile/qr.rs`,
`pair_requests.rs`, `tls.rs`) but produces asymmetric device identity
(v0.3 §8), not a bearer token. The helper does all cryptography; the main
process runs a **dedicated, ephemeral** enrollment server (fresh rcgen
cert, port 0, bound only for the session) so the always-on mobile server
is not enrollment attack surface. The helper never touches the network:
main relays frames between the socket and the TLS connection as opaque
bytes tagged by the framing below.

### 5.1 Sequence

```mermaid
sequenceDiagram
    participant U as User
    participant MA as Mac main app
    participant H as Mac vault helper
    participant IP as iPhone (new device)

    U->>MA: Settings → Security → Add Device
    MA->>H: begin_enrollment
    H->>H: enroll_secret = OsRng(16), nonce_e = OsRng(16), TTL 300 s, single-use
    H-->>MA: QR payload {v, host, port, fp, secret_b32, mac_device_id}
    MA->>MA: spawn ephemeral TLS server (rcgen cert, port 0)
    MA-->>U: show QR
    U->>IP: scan QR with Source iOS app
    IP->>IP: generate SE signing + agreement keypairs (non-exportable)
    IP->>MA: TLS connect, pin SHA-256(cert DER) == fp
    MA->>H: relay ENROLL_HELLO
    IP-->>H: {secret, nonce_n, sign_pub, agree_pub, name, platform, proto}
    H->>H: verify secret (constant-time, single-use, TTL)
    H->>H: transcript = SHA-256(fp‖secret‖nonce_e‖nonce_n‖device_ids‖pubkeys)
    H-->>U (via MA): SAS = sas8(transcript) shown on Mac
    IP-->>U: same SAS shown on iPhone
    U->>U: compare; confirm on both devices
    U->>MA: confirm → Mac LA presence (Touch ID)
    MA->>H: enroll_confirm
    H->>H: LA presence → sign enroll entry (Mac sign key)
    H->>H: envelope = HPKE-Seal(iPhone agree_pub, WrapPayload)
    H-->>IP: relay {registry to head, envelope, manifest, records ciphertext}
    IP->>IP: verify registry chain + manifest; decapsulate envelope (SE agree key)
    IP->>IP: store envelope/wraps; Keychain ThisDeviceOnly
    IP-->>H: ENROLL_ACK = iPhone signature over registry head hash
    H->>H: verify ACK → entry marked confirmed
    MA->>MA: teardown ephemeral server
```

### 5.2 Message details

| Field / message | Spec |
|---|---|
| QR payload | JSON `{v:2, host, port, fp, secret (base32), mac_device_id, name}` — same shape family as existing `EnrollmentPayload`, `v:2` distinguishes vault enrollment; rendered with existing `render_enrollment_qr` |
| TLS | rustls server (main), iOS URLSession/Network.framework with SPKI-pin = QR `fp`; hostname ignored (consistent with decision D6) |
| `enroll_secret` | 16 bytes, base32-no-pad in QR; verified with `subtle::ConstantTimeEq`; single-use; TTL 300 s from `begin_enrollment`; failure count ≥ 5 on a session → session torn down |
| Nonces | both sides 16 B random; both enter the transcript |
| Transcript | `SHA-256("ov0/enroll/transcript/v1" ‖ fp_bytes ‖ secret ‖ nonce_e ‖ nonce_n ‖ mac_device_id ‖ new_device_id ‖ sign_pub ‖ agree_pub)` — binds everything the user is about to approve |
| SAS | 8 chars, alphabet `23456789ABCDEFGHJKLMNPQRSTUVWXYZ` (repo's existing unambiguous alphabet), from `HKDF-SHA256(transcript, salt=nil, info="ov0/enroll/sas/v1")` → 5 bytes → 40 bits → 8×5-bit indices. Longer than the legacy 4-char pair tag on purpose. |
| SAS confirmation | explicit tap on **both** devices; either-side abort → session torn down, secret burned |
| Key exchange | iPhone sends only public keys; private keys never leave SE |
| Envelope | HPKE base (§2.7/§2.9), plaintext = WrapPayload, `info` per §2.9 |
| Initial vault transfer | registry JSONL to head + all current record objects (§3.7) + wraps; all ciphertext; sent only after SAS + LA confirm |
| ENROLL_ACK | iPhone signs `SHA-256("ov0/enroll/ack/v1" ‖ registry_head_hash ‖ mac_device_id)` with its SE signing key; proves the private key exists in that device |
| Replay prevention | single-use secret + fresh nonces + transcript binding + ACK binds head hash; a captured session replays to nothing |
| Timeout | whole flow 300 s; per-message 30 s; expiry → teardown + fresh secret on retry |
| Cancellation | either side may cancel at any step; nothing is persisted on the new device before envelope decapsulation succeeds; the Mac registry entry is written only after ACK — a cancelled attempt leaves no registry trace |

### 5.3 Failure states and error UX

| Failure | Behavior | User sees |
|---|---|---|
| QR expired | teardown | "Code expired — generate a new one" |
| Wrong/guessed secret | close after 5 tries | "Pairing failed — rescan the code" |
| TLS fingerprint mismatch | abort before any frame | "Secure channel could not be established" |
| SAS mismatch | both sides warn, teardown | "Codes don't match — do not pair" (security UX, no retry button on same secret) |
| Network drop mid-transfer | resume unsupported → restart with fresh secret | "Connection lost — start over" |
| ACK invalid/never arrives | registry entry not written | "iPhone didn't confirm — try again" |
| iPhone OS < required version | polite refusal before keygen | "Update iOS to continue" (§21 OQ-3) |

### 5.4 First device

Vault creation on the first Mac: helper generates VK, vault_id, writes
genesis registry entry (self-signed), creates MP wrap (user sets MP),
RK wrap (user prints RK), first manifest. The iPhone then enrolls via
§5.1 with the Mac as authorizer.

---

## 6. Routine credential authorization

Three supported local authorization paths (v0.3 §6.4, C17). All three end
in the same helper-side decision point: `authorize(action, origin, ref)`.

### 6.A Mac open + Touch ID available

```text
Chrome login page
 → extension: fill_candidates(origin, tab_url)
 → helper: origin policy check (§9.5) → matching accounts
 → extension popup: user picks "Sign in as alice@…"
 → helper: LA evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics,
                             "Source Vault: fill <origin>")
 → success → helper decrypts exactly that record
 → helper → nm-host → extension fills the approved frame
 → fill authorization consumed (one-shot)
```

### 6.B Clamshell + enrolled iPhone reachable

```text
… same until account picked …
 → extension popup offers "Approve on iPhone"
 → helper: build ApprovalChallenge (§6.5), state → AUTHORIZING
 → helper → main: event approval_requested (opaque TLV blob)
 → main → iPhone: direct pinned channel if reachable, else APNs wake (§7)
 → iPhone UI: shows Mac name, origin, action, account title
 → Face ID / device passcode on iPhone
 → iPhone signs challenge with its SE signing key → returns signature
 → main → helper: approval_result
 → helper verifies (§6.6) → releases one credential → consumed
```

### 6.C Clamshell + iPhone unavailable

```text
… same until account picked …
 → helper: LA evaluatePolicy(.deviceOwnerAuthentication, …)
   (on a clamshell/no-Touch-ID Mac this natively prompts for the
    Mac login password — no separate password UI is built)
 → success → release one credential → consumed
```

The Source master password is **not** offered in the fill UI. It appears
only in setup, recovery, and the exceptional `unlock_with_recovery` path
(e.g., Keychain/SE state lost).

### 6.4 User-presence policy resolution

Helper-side order for a fill:
1. Caller asked `method:"iphone"` → §6.B (with fallback offer on timeout).
2. Default: LA `.deviceOwnerAuthenticationWithBiometrics` with
   `.deviceOwnerAuthentication` fallback behavior — Touch ID when present,
   password when not. This is one LA call, so path 6.A and 6.C are the
   same code path with OS-mediated UX.
3. Card fills always force a fresh evaluation (no grace), action=`fill_card`.

### 6.5 Signed iPhone approval payload

Canonical TLV (§4.2), tags ascending:

| Tag | Field | Type | Notes |
|---|---|---|---|
| 0x01 | `proto` | u32 = 1 | |
| 0x02 | `request_id` | 16 B uuid | matches helper's pending request |
| 0x03 | `mac_device_id` | 16 B | must equal this Mac |
| 0x04 | `iphone_device_id` | 16 B | must be enrolled, not revoked |
| 0x05 | `origin` | string ≤ 253 | canonical origin (§9.4) |
| 0x06 | `action` | u8: 1 fill_password, 2 fill_card, 3 reveal, 4 unlock_vault | |
| 0x07 | `credential_ref` | 16 B uuid or absent | required for actions 1–3 |
| 0x08 | `nonce` | 16 B | helper-generated, single-use |
| 0x09 | `iat` | u64 | issued-at |
| 0x0A | `exp` | u64 | iat + 120 s |
| 0x10 | `signature` | 64 B r‖s | over `SHA-256("ov0/approval/sign/v1" ‖ tlv(without 0x10))`, iPhone SE signing key |

### 6.6 Approval verification and edge behavior

Helper checks, in order, all must pass:
signature valid for `iphone_device_id` (registry, not revoked) →
`mac_device_id` == self → `request_id` pending → `origin`/`action`/`ref`
equal the pending request → `nonce` unconsumed → `now ≤ exp` → mark nonce
+ request consumed → release.

| Case | Result |
|---|---|
| Replayed approval | nonce consumed → reject `APPROVAL_REPLAY` |
| Duplicate delivery while pending | idempotent: same decision returned; no double release (fill response is generated once and cached against request_id until consumed) |
| Expired | reject `APPROVAL_EXPIRED`; extension shows "request expired — retry" |
| Wrong origin/ref/action | reject `APPROVAL_MISMATCH` — this is the phishing tripwire: phone UI showed exactly what it signed |
| Wrong Mac | reject `APPROVAL_MISMATCH` |
| Unknown/revoked iPhone | reject `DEVICE_NOT_AUTHORIZED` |
| User cancels on phone | phone returns unsigned `{request_id, cancelled:true}` (no signature needed to cancel) → helper cancels request |
| Phone unreachable / timeout (120 s) | helper cancels; extension offers §6.C fallback |
| Offline (no LAN, no push path) | same as unreachable → §6.C |

The phone signs an authorization statement only. Passwords, usernames,
VK, RK, MP, device private keys, and biometric data never appear in the
challenge, the approval, or any relay (v0.3 §15.2).

---

## 7. iOS / APNs approval architecture

### 7.1 Foreground path

Source iOS app open (or opened by tapping the alert) → connects to the Mac
over the existing pinned TLS channel (`core/mobile` server) → vault scope
authenticated by device challenge-response, not legacy bearer tokens:

```text
Phone → Mac: AUTH_HELLO {device_id, client_nonce}
Mac main relays to helper → helper: {server_nonce, registry seq it knows}
Phone signs SHA-256("ov0/session/v1" ‖ nonces ‖ device_id ‖ registry_seq)
Helper verifies against registry → session scoped to this device
```

Over that session the phone fetches pending approval challenges
(`GET /v1/vault/approvals/pending` semantics over the relay) and returns
signatures. All frames are opaque TLV blobs relayed by main; the mobile
server learns message sizes and timing only.

### 7.2 Background path (APNs)

- **Provider role:** the §11 backup service hosts a minimal push-relay
  endpoint (`POST /v1/push`) holding the APNs provider token (.p8). It is
  the only Source-infrastructure involvement.
- **Wake trigger:** main app (not helper — helper has no network) asks the
  provider to push. Provider authenticates the caller as a device of the
  vault via the §11.4 request signature.
- **Push payload (entire):**

  ```json
  {"aps": {"alert": {"title": "Source Vault",
                     "body": "Approval request — open Source to review"},
           "sound": "default"},
   "rid": "<uuid>"}
  ```

- **What Apple/APNs sees:** an opaque UUID, generic text, timing.
- **What the Source provider sees:** push token, opaque UUID, timing,
  vault_id (needed for routing).
- **What neither ever sees:** origin, action, credential ref, usernames,
  passwords, card data, VK/RK/MP, device private keys, biometric data.
  (v0.3 §15.2 clarification, restated as a hard requirement.)
- **No PushKit/VoIP** (App Store policy), **no silent pushes** (too
  unreliable for this UX) — alert pushes only, `apns-expiration` = 120 s
  aligned with challenge expiry.
- **App wake behavior:** user taps → app foregrounds → §7.1 direct fetch
  of the actual challenge. The push carries no actionable data, so a
  spoofed push can only open the app, never authorize anything.
- **Offline / unreachable fallback:** if the direct fetch fails (phone on
  cellular, Mac behind NAT), the request fails and the extension offers
  the §6.C macOS authentication fallback. v1 does not relay challenge
  contents through the provider; §21 OQ-2 covers that v1.1 option. UX
  copy must not promise phone approval when the phone is unreachable
  (v0.3 §15.2).

---

## 8. Credential record types

Record plaintext is a JSON object (serde, `deny_unknown_fields` off for
forward compatibility, `schema` tag discriminates). Encrypted per §2.6.

### 8.1 Website Login (`"login"`, schema 1)

```json
{
  "schema": "login@1",
  "title": "GitHub",
  "username": "alice@example.com",
  "password": "…",
  "urls": [
    {"host": "github.com", "match": "exact"},
    {"host": "www.github.com", "match": "exact"}
  ],
  "notes": "…",
  "created_at": 1789650000,
  "updated_at": 1789650000,
  "password_history": [
    {"password": "…", "changed_at": 1789650000}
  ]
}
```

| Field | Rules |
|---|---|
| `title` | required, ≤ 200 chars, NFC |
| `username` | ≤ 320 chars; may be empty (rare SSO cases) |
| `password` | ≤ 1024 chars; required for login kind |
| `urls[]` | 1–20 entries; `host` is the canonical registrable-or-exact host (§9.4), never a full URL with path/query; `match`: `exact` (default) or `domain` (registrable domain + all subdomains) |
| `notes` | ≤ 10 000 chars; inside `ct`, never in metadata |
| `password_history` | max 10 entries, oldest evicted; sealed inside `ct`; never returned by list ops |
| `created_at`/`updated_at` | duplicated outside as plaintext columns (§3.4) — values inside `ct` are authoritative for display |

**Domain matching policy (default-deny):** a fill candidate requires
`origin.host == urls[i].host` under `exact`, or
`registrable_domain(origin.host) == urls[i].host` under `domain`.
`domain` is opt-in per URL entry via the management UI with an explicit
confirmation explaining the risk. Scheme must be `https` unless the entry
carries `"allow_http": true` (imported only for `localhost`/RFC-1918
targets; settable only by direct record edit, never by the extension).

### 8.2 Payment / Debit Card (`"card"`, schema 1)

```json
{
  "schema": "card@1",
  "label": "Personal Visa",
  "number": "…",
  "expiry": "09/28",
  "cardholder": "A EXAMPLE",
  "billing_address": {"line1":"…","line2":"…","city":"…",
                      "region":"…","postal":"…","country":"…"},
  "notes": "…",
  "created_at": 0, "updated_at": 0
}
```

- **`cvv` is not in the schema.** The importer discards CVV columns; the
  add/update IPC ops reject unknown secret-class fields; there is no code
  path that stores one (v0.3 C20).
- Validation: Luhn check (UX warning, not a hard error — some valid cards
  fail Luhn-adjacent schemes; warn and allow with confirmation).
- Every fill requires fresh user presence with `action: fill_card`
  (§6.4.3); the approval dialog text says "card".

### 8.3 Explicit v1 exclusions

Not implemented in v1 (v0.3): passkeys, TOTP (Dashlane `otpSecret` is
import-reported as unsupported, not stored), shared vaults, teams,
enterprise admin, Source ID recovery, arbitrary API secrets, SSH keys,
developer secret management. The schema tag (`login@1`, `card@1`) and the
TLV/action registries leave numeric headroom so future types don't fork
the wire format — that is the only concession to them.

---

## 9. Chrome autofill extension protocol

Chrome/Chromium on macOS is v1 (v0.3 C19). Three components: MV3
extension, `com.racker.zero.nm-host` binary (spawned by Chrome per
browser session, connects to the helper socket as client class
`nm-host`), and the helper (all decisions).

### 9.1 Extension architecture and permissions

- Manifest V3. Permissions: `nativeMessaging`, `tabs`, `storage`. No
  `cookies`, no `<all_urls>` host permission — content scripts declare
  `matches: ["https://*/*", "http://*/*"]` (required for form access) but
  run at `document_idle` and do nothing until the background worker
  confirms vault availability.
- No remote code, no remote config, no telemetry. The extension's CSP is
  MV3 default (no remote scripts).
- Distribution: development via unpacked extension with a **pinned key**
  in `manifest.json` so the extension ID is stable across dev machines;
  production via Chrome Web Store (stable ID). The native host manifest
  pins the ID either way (§9.2).

### 9.2 Native messaging host

- Manifest at
  `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.racker.zero.vault.json`
  (+ Chromium/Edge/Arc/Brave paths), written by the main app at setup:
  ```json
  {"name": "com.racker.zero.vault",
   "description": "Source Vault native messaging host",
   "path": "/Applications/SOURCE.app/Contents/Library/vault-nm-host/source-vault-nm-host",
   "type": "stdio",
   "allowed_origins": ["chrome-extension://<pinned-id>/"]}
  ```
- **Authentication layers:** (1) Chrome enforces `allowed_origins`;
  (2) nm-host is signed and peer-authenticated by the helper (§1.4);
  (3) nm-host performs a handshake with the extension: on connect it
  sends a per-session random `session_id` that the extension must echo in
  every request — this binds messages to one browser session, not to
  another local process replaying stdio.
- nm-host is a dumb broker: it validates frame shape (schema, lengths)
  and forwards to the helper. It holds no keys, performs no matching,
  never logs field values.

### 9.3 Message schemas (extension ↔ nm-host, JSON, ≤ 32 KiB)

```text
→ {"op":"hello","proto":1,"session_echo":"<session_id>"}
← {"ok":true,"vault_state":"unlocked"|"locked"|"uninitialized"}

→ {"op":"get_candidates","origin":"https://github.com","tab_url":"https://github.com/login"}
← {"ok":true,"request_id":"uuid","accounts":[{"ref":"uuid","title":"GitHub",
   "username":"alice@example.com"}]}
   or {"ok":true,"locked":true}

→ {"op":"authorize","request_id":"uuid","ref":"uuid","method":"local"}
→ {"op":"authorize","request_id":"uuid","ref":"uuid","method":"iphone"}
← {"ok":true,"fill":{"username":"…","password":"…"},"expires_in":30}
   or {"ok":false,"error":"USER_CANCELLED"|"APPROVAL_EXPIRED"|…}

→ {"op":"save_new","origin":"…","tab_url":"…","username":"…","password":"…","title":"…"}
← {"ok":true,"ref":"uuid"}

→ {"op":"save_update","ref":"uuid","password":"…"}
← {"ok":true}

→ {"op":"request_unlock"}      -- triggers helper LA prompt via app UI
← {"ok":true,"vault_state":"unlocked"}
```

### 9.4 Origin retrieval and canonicalization

- The **browser-reported origin** comes from the content script's
  `window.location.origin`; the **tab URL** comes from `chrome.tabs`
  (`tab.url`, requires `tabs` permission). Both travel in the request;
  the helper recomputes canonical origins from both and requires
  agreement (test BR-11). A compromised extension can lie about
  both — the mitigation is that the user-presence prompt and iPhone UI
  **display the claimed origin**, so a lying extension must show the user
  an origin they didn't intend (§9.9 assumptions).
- Canonicalization (identical implementation in helper (Rust) and test
  vectors in JS/Swift): IDN → UTS-46 punycode (`xn--` form); lowercase;
  strip trailing dot; strip default ports (443/80); scheme must be
  `http`/`https`; no userinfo; no trailing path. Result form:
  `scheme://host[:port]`.

### 9.5 Matching policy (helper-side, fail closed)

| Input | Decision |
|---|---|
| exact `scheme://host` match with `match:"exact"` | eligible |
| subdomain of stored host (`app.github.com` vs `github.com`) | eligible only if that URL entry has `match:"domain"`; else a "similar item exists" hint with no autofill |
| same registrable domain, different host | same rule; PSL decides the boundary (§9.7) |
| stored `https`, page `http` | refuse; exception: host is `localhost`/`127.0.0.1`/RFC-1918 **and** the record has `allow_http:true` |
| IDN/punycode host (`xn--…`) | refuse + warn banner "this site's address uses look-alike characters" unless the record was explicitly saved for that punycode host |
| confusable rendering | helper computes a confusable skeleton (bundled Unicode `confusables.txt` subset); skeleton collision with a *different* stored host → refuse + warn |
| host with userinfo, IP-literal trickery, dotless/decimal-IP forms | refuse |
| cross-origin iframe | fill only the frame whose own canonical origin matches; never the parent on a child's behalf |
| opaque origins (`data:`, `file:`, `about:`, sandboxed null origin) | refuse |
| multiple accounts | chooser UI; no silent pick |

### 9.6 One-shot fill token and replay protection

- `request_id` + `ref` + approved origin + nonce form one authorization.
- The fill response is delivered once; the helper marks it consumed when
  the response frame is written. Re-`authorize` with the same
  `request_id` → `APPROVAL_REPLAY`.
- Response lifetime 30 s: the extension fills immediately; the content
  script re-verifies `location.origin` at fill time and refuses if it no
  longer equals the approved origin (navigation races).
- The content script fills via direct DOM assignment + `input`/`change`
  events (React-safe); it never logs field values, never stores them in
  `chrome.storage` (session memory only, cleared on fill).

### 9.7 Public Suffix List

- Rust: `psl` crate with a **pinned, vendored** PSL snapshot
  (`public_suffix_list.dat` committed, versioned, updated deliberately).
- Extension: `tldts` with its bundled list; the vendored PSL snapshot
  date is recorded in both builds; shared test vectors (§16.4) pin
  boundary behavior (`github.io`, `co.uk`, `appspot.com` …) so the two
  implementations cannot drift silently.

### 9.8 Save / update flows

- **Save new:** on form submit with a password field, content script
  captures `{origin, username, password}` into session memory; after
  navigation success heuristic (URL change without error page), the
  popup badge offers "Save to Source Vault". Tapping sends `save_new`.
  Requires vault UNLOCKED; if locked, offer unlock first. The helper
  merges duplicates by (canonical host, username): existing match →
  `save_update` suggestion instead.
- **Update:** on submit where a stored ref was filled and the password
  differs, offer "Update saved password" → `save_update`; old password
  moves to `password_history`.
- Save/update never require *fresh* presence beyond the vault being
  unlocked (the user just typed the secret), but they are logged in the
  vault's local audit view.

### 9.9 Extension compromise assumptions (documented, v0.3 §12.5)

A compromised extension can: request candidates for arbitrary origins
(getting only titles/usernames of matching items — a metadata leak
bounded by rate limiting: max 20 candidate queries/minute/session), and
trick the user into approving a fill for an origin displayed in the
prompt. It cannot: enumerate the vault, read passwords without a
per-fill user presence, obtain VK material, or replay a consumed fill.
The helper's origin display in every presence prompt is the load-bearing
mitigation — prompts are rendered by the OS/LA and the iOS app, not by
the extension.

---

## 10. Dashlane import

Deterministic, offline, synthetic-fixture-only until the §19 gate.
**The user's real Dashlane export is never requested, received, committed,
logged, pasted into AI context, or used in tests.**

### 10.1 Expected input formats

Dashlane's web export produces either a single `.csv` (newer) or a
`.zip` containing per-type CSVs (`credentials.csv`, `payments.csv`,
`securenotes.csv`, `ids.csv`, `personalinfo.csv`) (older). v1 supports:

- single CSV whose header matches the credentials or payments shape;
- the ZIP, reading `credentials.csv` and `payments.csv` only; other
  members are reported as unsupported (not parsed).

Parser: Rust `csv` crate (RFC 4180 + lenient quoting mode), streaming
(`Reader::records()` iterator), one row at a time. No `eval`, no
deserialization frameworks beyond `csv`, no network, no AI involvement.

### 10.2 Schema detection and field mapping

Header row decides the dialect (case-insensitive, trimmed):

| Dashlane credentials.csv column | → login@1 field |
|---|---|
| `title` / `name` | `title` |
| `username` / `login` (+ `username2`, `username3` fallback) | `username` (first non-empty) |
| `password` | `password` |
| `url` / `website` | `urls[0]` after §9.4 canonicalization; `match:"exact"` |
| `note` | `notes` |
| `otpSecret` | **unsupported** — counted and reported "re-enter manually" |
| `category`, `totp`, others | ignored |

| payments.csv column | → card@1 field |
|---|---|
| `name`/`account_name` | `label` |
| `card_number`/`number` | `number` |
| `expiration`/`expiry` | `expiry` (normalised MM/YY) |
| `cardholder`/`name_on_card` | `cardholder` |
| `billing_*` | `billing_address` |
| `cvv`/`cvc`/`security_code` | **discarded on sight**; the row's report line notes "CVV not imported (by design)" |

Rows whose shape matches neither table → skipped, counted, reported by
row number only.

### 10.3 Row handling rules

- **Malformed rows** (bad quoting, column count mismatch): skip + count;
  never guess field positions.
- **Embedded commas/newlines/quotes:** standard RFC-4180 handling;
  Dashlane's historical inconsistent quoting is handled by lenient mode
  plus a strict-mode fallback retry per file (whole-file strategy chosen
  once from the first 100 rows — no per-row mode mixing).
- **Unicode:** NFC-normalize display fields; reject unpaired surrogates
  / invalid UTF-8 at read (lossy conversion forbidden for secrets).
- **Oversized:** field > 64 KiB or row > 256 KiB → skip + count.
- **Duplicates within the file / against the vault:** key =
  (kind, canonical host, username) for logins, (kind, last-4-of-number)
  for cards. Duplicate → keep existing vault item, count as duplicate.
  Last-4 is stored only transiently for this comparison, never persisted.
- **Idempotent re-import:** an `import_log` table stores
  `SHA-256(normalized-row-key)` digests (no secret values — the digest
  input includes the password only in combination with a random per-vault
  import salt from `kv`, so the log is not an offline dictionary oracle).
  Re-importing the same file → all duplicates reported, zero new items.

### 10.4 Streaming encryption, no plaintext staging

```text
file → csv stream → row → map → JSON serialize → AEAD seal (record_key)
     → INSERT → zeroize row buffer
```

At no point is there a plaintext Vec of all records, a plaintext temp
file, or a plaintext "preview vault". The import UI shows counts and
titles only; titles come from the just-written `meta_ct` path like any
other list view. The file dialog path is handed to the helper; the helper
validates it is a regular file (not a symlink, not inside the vault
directory) and opens it itself.

### 10.5 Reporting

Import report (UI + savable text): counts imported / duplicates /
skipped-malformed / skipped-unsupported (with category names like
"otpSecret", never values), duration, and per-skip row numbers. No
secret values, usernames, or URLs in reports or logs. Report generation
runs with capture suppression active (§14).

### 10.6 Source-file deletion and honesty copy

Post-import the UI offers "Delete the export file" (move to Trash via
`tauri-plugin-dialog`+FS op by the main app) with copy that must say,
verbatim in spirit:

- the CSV is plaintext and readable by anyone on this Mac;
- it may already be in cloud-synced folders (iCloud Downloads/Desktop)
  and backups;
- delete it there too, and empty the Trash;
- SSD/APFS snapshots make guaranteed erasure impossible — Source does not
  claim secure deletion (v0.3 §13, C10 honesty rule).

### 10.7 Fuzzing and fixtures

- Synthetic fixtures under `src-tauri/vault-helper/tests/fixtures/dashlane/`:
  valid minimal, all-dialects, embedded commas/newlines/quotes, invalid
  UTF-8, oversized fields, duplicate storms, mixed ZIP, empty file,
  header-only, 10⁵-row stress. Every fixture is generated by a committed
  script with fixed seeds; none contain real-looking secrets (passwords
  are `fixture-…` tokens; the repository/CI check from v0.3 §14.1 item 6
  would flag real ones).
- `cargo fuzz` target `fuzz_dashlane_row`: arbitrary bytes → dialect
  sniff + row parse must never panic, never allocate > 2× input size,
  never emit a record with an empty password field from a non-empty
  password column, and round-trip escaping invariants hold.

---

## 11. Durable remote encrypted backup

The provider is untrusted (v0.3 §11). Normal sync stays direct
peer-to-peer (§7.1 channel carries encrypted record objects the same way).
Remote storage exists so that losing both devices leaves a recoverable
ciphertext copy.

### 11.1 Storage abstraction

```rust
#[async_trait]
trait BackupStore: Send + Sync {
    async fn head_manifest(&self) -> Result<Option<Manifest>, BackupError>;
    async fn get_object(&self, key: &str) -> Result<Vec<u8>, BackupError>;
    async fn put_object(&self, key: &str, bytes: &[u8],
                        sha256: &[u8; 32]) -> Result<(), BackupError>;
    async fn list_objects(&self, prefix: &str)
        -> Result<Vec<(String, u64)>, BackupError>;
    async fn cas_manifest(&self, expected_generation: u64,
                          manifest: &SignedManifest)
        -> Result<(), BackupError>;   // compare-and-swap on generation
    async fn delete_objects(&self, keys: &[String]) -> Result<(), BackupError>;
}
```

- v1 implementations: `FsBackupStore` (directory-backed; unit/integration
  tests, CI, recovery rehearsals) and `HttpBackupStore` (production,
  speaks to a small object service over HTTPS).
- **Backend selection is explicitly deferred** (owner instruction): the
  v1 production backend is chosen at Phase F start as "the simplest
  reliable object store with conditional writes"; ideology and
  decentralization are not selection criteria. Everything above the trait
  is backend-agnostic and fully testable today against `FsBackupStore`.

### 11.2 Object and manifest model

```text
vault/<vault_id>/
├── manifest.json                    current signed manifest (CAS target)
├── manifest.gen-<n>.json            previous generations (retention: 2)
├── registry.json                    copy of registry JSONL at manifest time
├── wraps/password.wrap | recovery.wrap | devices/<id>.wrap
└── objects/rec/<record_id>/<vk_generation>    §3.7 object bytes
```

`SignedManifest` (canonical TLV, signed by the device's signing key):

| Tag | Field |
|---|---|
| 0x01 | `version` u32 = 1 |
| 0x02 | `vault_id` 16 B |
| 0x03 | `generation` u64 (strictly increasing, +1 per publication) |
| 0x04 | `created_at` u64 |
| 0x05 | `registry_head` 32 B |
| 0x06 | `vk_generation` u32 |
| 0x07 | `object_index_hash` 32 B — SHA-256 over sorted `(key, sha256, size)` lines |
| 0x08 | `prev_manifest_hash` 32 B |
| 0x09 | `signer_device_id` 16 B |
| 0x10 | `signature` 64 B over `SHA-256("ov0/manifest/sign/v1" ‖ tlv(without 0x10))` |

The full object index is itself an object (`objects/index/<generation>`,
JSON, plaintext-classified fields only) so restoring doesn't require
listing thousands of keys.

### 11.3 Publication protocol (atomic)

```text
1. helper: seal any dirty records → objects (§3.7), build index object
2. main → provider: PUT every new/changed object (content-hash verified)
3. main → provider: PUT manifest.new (full SignedManifest bytes)
4. main → provider: CAS manifest ← manifest.new iff current generation
   == expected (server-enforced compare-and-swap)
5. main → provider: DELETE manifest.new; GC pass (below)
6. helper: persist accepted generation + head hash (Keychain state item)
```

- **Interrupted upload:** objects uploaded in step 2 but unreferenced by
  any manifest ≤ retention are garbage, collected after a 7-day grace
  (protects slow peers that may be referencing them mid-sync).
- **CAS failure** (another device published first): helper re-reads head,
  verifies, merges (records merge by record_id + updated_at with
  tombstone dominance; registry must chain — divergence → §4.6 fork
  handling), rebuilds manifest at generation+1, retries CAS. Max 3
  retries then `BACKUP_CONFLICT` surfaced.
- **GC:** objects referenced by neither the current nor the retained
  previous 2 manifests, older than 7 days → deleted.

### 11.4 Backup authentication and authorization

- `vault_id` (random UUID) is the account identifier.
- Every request carries
  `Authorization: SourceVault t=<unix> s=<HMAC-SHA256(backup_auth,
  "ov0/backup-req/v1" ‖ method ‖ path ‖ SHA-256(body) ‖ t)>`,
  timestamp skew ≤ 300 s, nonce-cache on the server for replays.
- `backup_auth` reaches devices only inside WrapPayload (§2.2): MP wrap,
  RK wrap, device envelopes. Disaster recovery therefore restores backup
  access by construction. `backup_auth` is generated at vault creation
  and is **not rotated** by VK rotation in v1 (rotating it would orphan
  the provider account; it protects availability and third-party
  vandalism only, never confidentiality).
- The server stores `backup_auth` to verify HMACs. This is honest and
  deliberate: the server is untrusted for confidentiality (it holds only
  ciphertext) and semi-trusted for availability; HMAC auth exists to
  prevent *third-party* vandalism of the account, not to keep secrets
  from the provider. Device **authorization** (who may publish a new
  manifest generation) is additionally enforced client-side by manifest
  signature verification — the provider cannot mint devices (§4.6).
- Upload authorization: a valid manifest signature from a non-revoked
  enrolled device at generation > current. The server checks signature
  against the uploaded registry's keys as a courtesy filter; devices
  never trust the server's filter.

### 11.5 Download, restore, verification

1. Fetch head manifest → verify signature (registry), generation > last
   seen (else `MANIFEST_ROLLBACK`), `registry_head` matches a valid
   registry (fetch + verify chain per §4.4).
2. Fetch index object; for each record object: verify SHA-256 from index,
   hand to helper; helper verifies AEAD at decrypt time (lazy) — restore
   completes on hash verification, corruption surfaces per-record later.
3. Missing/corrupt object → retry ×3 → `BACKUP_OBJECT_MISSING`;
   restore can proceed partially with the user told exactly how many
   records are unavailable (count only).
4. **Rollback:** generation regression → refuse + surface.
5. **Fork:** two validly signed manifests from different devices with the
   same parent generation → recovery UI lists both (device names,
   generations, created_at, record counts); the user picks; the loser is
   archived as `manifest.gen-<n>.json`, never deleted silently.
6. **Stale snapshot:** if the freshest downloadable manifest is older
   than a peer device's state, prefer direct peer sync; the backup is a
   floor, not a ceiling.

### 11.6 Metadata leakage and DoS assumptions (explicit)

Provider may learn: vault_id, device push token (§7), object count/sizes,
timing, IP addresses, generation cadence. Mitigations: none claimed in
v1 beyond TLS transport. Provider may also delete/withhold everything:
availability is out of scope cryptographically; the product monitors
backup success and warns after 48 h without a successful publication.
Devices always keep full local copies; provider outage never blocks local
unlock/fill.

---

## 12. Recovery

All scenarios share primitives: backup download (§11.5), registry epoch
rules (§4.5), VK rotation (§2.10), enrollment (§5). "Publish" = §11.3.

### Scenario 1 — Mac lost, iPhone retained

1. iPhone: Settings → Trusted Devices → Mac → Revoke (Face ID presence).
2. Helper-equivalent on iPhone appends signed `revoke` entry; VK rotation
   runs on iPhone (new VK, re-encrypt all records, new wraps for MP/RK/
   remaining devices, new manifest generation, publish).
3. Replacement Mac: §5 enrollment, iPhone authorizes; new device envelope
   under the *new* VK.
4. Old Mac's envelope object is GC'd per §11.3 retention.

### Scenario 2 — iPhone lost, Mac retained

Symmetric; Mac helper performs rotation; the iPhone's signing key is
rejected from that moment (`DEVICE_NOT_AUTHORIZED`).

### Scenario 3 — both devices lost, master password retained

1. New supported device → Source installed → "Recover vault".
2. **Account location.** `vault_id` is not memorizable, and `backup_auth`
   is inside the wraps being fetched, so recovery bootstraps through a
   locator registered at vault setup:
   - At setup the provider account is created with the user's **email
     address** as a non-secret lookup handle (also used for
     backup-failure notifications, §11.6).
   - At vault creation the helper generates two random 16-byte
     `locator_salt`s (stored in `header.json`) and registers on the
     provider:
     `locator_mp = HMAC-SHA256(HKDF(PK, salt=locator_salt_mp,
     info="ov0/locate/mp/v1"), "ov0/locator/v1") → vault_id`
     and the RK analogue under `…/rk/v1` with the RK-derived key.
   - Recovery: `POST /v1/recover/locate {email}` returns `{vault_id,
     kdf_salt, locator_salts}`; the client derives PK (or RK key),
     recomputes the locator, and
     `GET /v1/recover/bundle {locator}` returns manifest, registry,
     wraps, and the object index. A wrong MP/RK yields an unknown locator
     (404) — never a decryption result.
   - Provider exposure: the handle, the salts, and recovery-attempt
     timing. The locator gives the provider an offline MP-guessing oracle
     no stronger than the wraps it already serves (same Argon2id cost).
3. Download manifest/registry/wraps/objects → unwrap VK locally →
   verify manifest + registry chain.
4. Create new device identity (SE keys) → `recovery_epoch` entry with
   `recovery_proof` (§4.5) binding the downloaded manifest hash → new
   genesis-class enroll for the new device at epoch+1.
5. Rotate VK (the recovered VK authorized a registry transition; retire
   it) → re-encrypt → new wraps (MP unchanged material, RK unchanged
   unless user requests replacement) → publish.
6. Enroll the user's other replacement devices per §5.

### Scenario 4 — both devices lost, Recovery Key retained

Identical to scenario 3 with `kind:"rk"` locator/wrap. The 24-word RK is
entered on the new device; checksum validates before any network call.

### Scenario 5 — MP forgotten, trusted device retained

1. Trusted device: fresh LA presence → helper decrypts nothing bulk; it
   re-wraps resident VK under PK′ = Argon2id(MP′, new salt).
2. `password.wrap` atomically replaced (write tmp + rename); header salt
   updated; manifest generation +1 published; the MP recovery locator
   (§12 scenario 3) is re-registered with the provider.
3. Old password wrap is destroyed in the same transaction; there is no
   "both wraps work" window.

### Scenario 6 — RK lost (no theft suspicion), trusted device retained

1. Fresh LA presence → generate RK′ → **rotate VK** (v0.3 C12: re-wrap
   alone is insufficient) → re-encrypt records → new wraps for MP, RK′,
   all devices → publish → re-register the RK recovery locator → print
   new recovery sheet (print path never
   renders RK words to the screen longer than the print dialog requires;
   the words are shown once, in a capture-suppressed window, §14).
2. Old RK stops working immediately for current state; the old RK
   locator is de-registered.

### Scenario 7 — RK suspected stolen

Identical mechanics to scenario 6, plus: treated as a security incident —
UI banners on all enrolled devices at next sync ("Recovery Key was
replaced on <date>; if this wasn't you…"), backup retains pre-rotation
manifest for 2 generations per §11.3, and the audit view lists the
rotation event.

**Retained limitation (must appear in product copy):** an attacker who
previously copied old ciphertext plus the old recovery.wrap can still
decrypt that historical snapshot with the old RK. Rotation protects the
current and future states; it cannot erase already-exfiltrated copies
(v0.3 §10.4, C12).

### Scenario 8 — device compromised while unlocked

1. Revoke the device (any surviving trusted device, or post-recovery
   epoch) → VK rotation → publish.
2. The helper on the revoking device computes the **exposure set**:
   records whose plaintext was served to that device in the last N days
   (the helper keeps a local, non-secret audit log of `record_id` +
   timestamp + action for exactly this; default N = 30, log sealed under
   meta_key).
3. Product requires the user through a per-credential rotation checklist
   (change passwords at the account providers) for the exposure set, and
   recommends it for everything else. This is breach response, not
   recovery (v0.3 §9 honest limits apply).

---

## 13. Lock and runtime state machine

### 13.1 States

```text
UNINITIALIZED ──setup──▶ LOCKED ◀─────────────┐
                            │ unlock flow      │ lock / timeout /
                            ▼                  │ crash / sleep
                        UNLOCKING ──fail──▶ LOCKED
                            │ ok
                            ▼
                        UNLOCKED ◀────────────┐
                          │  ▲ per-op         │ op complete
                          ▼  └────────────────┘
                      AUTHORIZING (substate; VK resident)
UNLOCKED ──▶ SYNCING ──▶ UNLOCKED      (helper-side merge)
UNLOCKED ──▶ BACKING_UP ──▶ UNLOCKED   (helper seals; main uploads)
UNLOCKED ──▶ ROTATING_KEYS ──▶ UNLOCKED
LOCKED ──▶ RECOVERING ──▶ ROTATING_KEYS ──▶ UNLOCKED
any ──fatal──▶ ERROR ──retry/restore──▶ LOCKED
registry fork / confirmed tamper ──▶ COMPROMISED (writes frozen until
                                      user resolves)
```

### 13.2 State table

| State | VK | MP/RK | Decrypted records | IPC ops allowed | Network (main) | UI |
|---|---|---|---|---|---|---|
| UNINITIALIZED | none | none | none | `unlock_with_recovery`(setup), `begin_enrollment`(first-device) | backup locate | setup wizard only |
| LOCKED | none | none | none | `unlock*`, `get_state`, `fill_candidates`(→`locked`), `approval_result` | backup poll ok | unlock prompt |
| UNLOCKING | transient (unwrap in flight) | transient during entry | none | only the in-flight op | — | spinner, cancel |
| UNLOCKED | resident (helper only) | never resident | never resident as a set; per-record transient during an op | all §1.5 ops | sync/backup ok | full vault UI |
| AUTHORIZING | resident | never | the one approved record, transient, post-approval | the in-flight authorize op only for that request_id | approval relay | presence prompt / phone sheet |
| SYNCING | resident | never | none (ciphertext merge) | reads blocked ≤ 5 s, presence ops continue | peer/backup active | sync badge |
| BACKING_UP | resident | never | none (seal only) | all (snapshot is consistent) | upload active | backup badge |
| ROTATING_KEYS | old+new transient, old zeroized at flip | never | per-record transient re-seal | fill ops queue ≤ 30 s; high-risk ops refused | publish at end | blocking banner |
| RECOVERING | transient post-unwrap | transient during entry | verify-only transient | recovery ops only | download active | recovery wizard |
| ERROR | none (zeroized on entry) | none | none | `get_state`, `lock`, restore ops | restore download | error + restore path |
| COMPROMISED | unchanged but writes frozen | none | reads allowed, writes frozen | read ops, `revoke_device`, recovery | as unlocked | fork/tamper resolution UI |

### 13.3 Global rules

- **Unlock ≠ authorization.** UNLOCKED means VK residency only. Every
  credential release additionally passes through AUTHORIZING with one-shot
  user presence (§6). There is no grace window in v1 (v0.3 §6.5 default).
- Transition triggers: as drawn above; any unexpected IPC op for the
  current state → `BAD_STATE` error, no side effects.
- Timeout behavior: UNLOCKING/RECOVERING ops abort after 120 s →
  LOCKED/ERROR; AUTHORIZING expires with the challenge (120 s);
  ROTATING_KEYS has no timeout but is resumable-idempotent after crash
  (§2.10).
- Crash behavior: process death in any state → LOCKED on next start
  (except interrupted rotation, which resumes, and interrupted recovery,
  which restarts from downloaded state re-verification).
- Zeroization on every transition into LOCKED/ERROR and at the end of
  every transient use (§2.11).

---

## 14. Source capture isolation

Built on the mechanisms already landed in this repository (hardening
phase, commits fd659c3/d96d51c and the smoke harness in 47f6e3d). This
section defines how vault surfaces use them; it does not re-specify the
mechanisms.

### 14.1 Mechanism inventory (existing)

| Mechanism | Location | Behavior |
|---|---|---|
| Exclusion registry | `core/capture_exclusions.rs` | `register_excluded_window_title` / `unregister_excluded_window_title` / `is_excluded_window_title` |
| Sensitive-surface counter | same | `sensitive_surface_shown` / `sensitive_surface_hidden` / `sensitive_surface_visible` |
| Frame suppression | `core/screen_recorder/lifecycle.rs` | recording loop drops frames and one-shot `capture_frame` refuses while `screen_capture_suppressed()` |
| Keyboard suppression | `platform/input/keyboard_macos.rs` | `should_record_keystroke` choke point + `IsSecureEventInputEnabled` FFI gate |
| Vault path exclusion | `tauri.conf.json` + `core/asset_scope_guard.rs` + `core/vault_dir.rs` | asset scope `recordings/**` + explicit `!vault/**` deny; dir `0700` + `.metadata_never_index`; runtime probe in `src/dev/smoke.ts` |
| Tauri commands | `app/commands/capture_exclusions.rs` | `register_capture_excluded_window`, `unregister_capture_excluded_window`, `sensitive_capture_surface_changed`, `capture_suppression_active` |

### 14.2 Vault surface obligations

| Surface | Registration duty |
|---|---|
| Vault management window (unlock, item list, add/edit, reveal, recovery sheet) | window title registered as excluded at creation, unregistered at destroy; during reveal/edit/print steps the surface counter is additionally incremented |
| Dashlane import wizard (all steps) | window registered + counter incremented for the whole wizard |
| Setup wizard (MP creation, RK display/print) | same as import wizard |
| iPhone approval sheet on Mac | registered window |
| Chrome extension popup | not a Source window — protection comes from never rendering secrets in-page (passwords go into masked fields only); the fill itself happens in Chrome's window, which Source's recorder can see — accepted: filled passwords render as dots; reveal-in-page is not offered |
| Keyboard: all vault password/MP/RK fields | `secureInput` class → `IsSecureEventInputEnabled` suppression + password-field heuristics; RK/MP entry uses native secure text fields |

### 14.3 Interaction rules

- **OCR / indexing / timeline:** suppressed frames never reach OCR, so
  indexing and timeline exclusion follows from §14.1 frame dropping.
  Additionally the vault directory never appears in any search/export
  root (storage-layer exclusion, v0.3 §7).
- **One-shot screenshot APIs:** `capture_frame` refuses while any
  sensitive surface is visible (already implemented); vault UI calls
  `sensitive_capture_surface_changed(true)` before rendering secret
  material.
- **Secure event input:** when macOS reports secure input enabled (any
  app), keystroke recording suppresses; when a Source vault field has
  focus the surface counter is also up, so both signals suppress.
- **ScreenCaptureKit migration:** when the recorder migrates from
  `CGDisplay.image` to SCK per-window filters, vault windows must
  additionally be excluded by window ID in the content filter; until then
  global suppression is the normative control and this spec's fail-closed
  rule applies to it.

### 14.4 Fail-closed behavior (normative)

Before any **display-class** secret release (`reveal`, RK display,
import report, add/edit form prefill), the helper queries the main app:
`sensitive_capture_surface_changed` state must show the requesting
surface registered and visible, i.e. `capture_suppression_active()` =
true. If the check fails or times out (500 ms), the helper returns
`CAPTURE_UNSAFE`; the UI shows "Screen-capture exclusion is unavailable,
so Source won't display this right now." Fill-class releases (password
into Chrome's masked field) do not require the Source-side check — the
secret never renders in a Source window — but the extension still never
reveals in-page (§14.2). This is a code path, not a comment: no display
without the check, and the check's failure default is refusal.

---

## 15. Error handling

Universal rules: fail closed; errors carry a stable code + human-safe
context; no secret values, usernames, URLs (beyond the origin a user is
actively approving), VK/RK/MP fragments, or serialized secret objects in
any log, panic message, telemetry, or error string. `Debug` is not
implemented for secret-bearing types (§2.11); the helper's log filter
allowlists module paths rather than denylisting content.

| Condition | Code | Behavior | User sees |
|---|---|---|---|
| Wrong master password | `WRONG_CREDENTIAL` | AEAD failure on unwrap; 500 ms delay ×2^attempts backoff, capped 30 s; no counter oracle beyond attempts | "Incorrect password" |
| Wrong Recovery Key | `RECOVERY_KEY_INVALID` (checksum) / `WRONG_CREDENTIAL` (AEAD) | checksum checked offline first | "Not a valid recovery key" |
| Corrupted record | `RECORD_CORRUPT` | quarantine record, continue; restore from backup/peer offered | "One item is damaged and can be restored" |
| Corrupted wrap | `WRAP_CORRUPT` | that wrap path disabled; other wraps unaffected | "This unlock method is damaged — use another" |
| Corrupted database | `DB_CORRUPT` | ERROR state; never auto-delete; restore flow offered | restore wizard |
| Invalid AEAD tag (any) | `INTEGRITY_FAILURE` | refuse the operation; counts toward tamper signal | generic failure copy |
| Invalid signature | `SIGNATURE_INVALID` | reject object; if registry/manifest → COMPROMISED flow | "Vault data failed verification" |
| Registry fork | `REGISTRY_FORK` | COMPROMISED; writes frozen | fork resolution UI (§4.6) |
| Truncated registry | `REGISTRY_TRUNCATED` | reject; fetch full chain | sync error copy |
| Manifest rollback | `MANIFEST_ROLLBACK` | reject; keep local | "Backup is older than this Mac's vault" |
| Stale backup | `BACKUP_STALE` | warn after 48 h without publish | settings warning |
| Conflicting device states | `BACKUP_CONFLICT` | §11.3 merge/retry, then surface | "Two devices changed the vault — review" |
| Helper unavailable / crash | `HELPER_UNAVAILABLE` | §1.6 restart policy | "Vault is restarting…" |
| Phone unreachable | `PHONE_UNREACHABLE` | offer §6.C fallback | "iPhone unavailable — use Mac password" |
| Phone approval timeout | `APPROVAL_EXPIRED` | cancel request | "Approval expired" |
| Invalid phone signature | `SIGNATURE_INVALID` | reject; log device_id (not secret) | "Approval couldn't be verified" |
| Expired approval delivery | `APPROVAL_EXPIRED` | reject | retry affordance |
| Extension disconnected | `EXTENSION_LOST` | pending nm requests cancelled; fill not delivered | none (page-side timeout copy) |
| Backup provider unavailable | `BACKUP_UNAVAILABLE` | queue publication, backoff 1→5→15 min, warn at 48 h | settings badge only |
| Failed VK rotation | `ROTATION_FAILED` | §2.10 resume-or-rollback to old manifest; never a half-live mix | "Security update didn't finish — retrying" |
| Interrupted recovery | `RECOVERY_INCOMPLETE` | re-verify downloaded state on retry; idempotent | recovery wizard resumes |
| Partially completed import | `IMPORT_PARTIAL` | committed rows stay; report shows exact counts; re-import is idempotent | import report |

**User-facing vs internal:** the user sees the copy column; internal
diagnostics record code + state + counters. Crash reports from the helper
are disabled entirely (no crash reporter is linked); the main app's
diagnostics never include helper frames.

---

## 16. Security test plan

Every test below has a binary pass/fail. IDs are stable; the §19 gate
references them. Rust tests live in the helper crate; Swift tests in the
iOS repo; shared vectors in `src-tauri/vault-helper/tests/vectors/`.

### 16.1 Cryptography

| ID | Test | Expected |
|---|---|---|
| CR-01 | MP wrap → unwrap round trip | WrapPayload recovered byte-identical |
| CR-02 | RK wrap → unwrap round trip | same |
| CR-03 | Wrong MP (1-bit flip) | AEAD failure, `WRONG_CREDENTIAL`, no partial state |
| CR-04 | Wrong RK (valid checksum, wrong entropy) | same |
| CR-05 | Record ciphertext 1-byte tamper | `RECORD_CORRUPT`, no plaintext |
| CR-06 | AAD tamper (record_id, generation, schema swapped) | decryption fails |
| CR-07 | Nonce strategy audit: 2²⁰ seals, no nonce/key pair repeats | unique (statistical guard; construction uses random 192-bit) |
| CR-08 | VK rotation: full vault re-seal | old VK fails on every new record/wrap (`INTEGRITY_FAILURE`); new VK opens all |
| CR-09 | KDF downgrade attempt (header kdf_version-1) | refused |
| CR-10 | Wrap header tamper (salt/params swapped between files) | AEAD failure |
| CR-11 | Zeroization spot test: after lock, helper heap sampled for VK/RK sentinel patterns | not found (test build with canary secrets) |
| CR-12 | backup_auth present in all wrap types after rotation | recovered via MP, RK, and device envelope paths |

### 16.2 Registry

| ID | Test | Expected |
|---|---|---|
| RG-01 | Unsigned enroll entry appended | rejected, chain unchanged |
| RG-02 | Forged enroll (attacker key) | `SIGNATURE_INVALID` |
| RG-03 | Forged revocation of a live device | rejected (authorizer not enrolled/unauthorized) |
| RG-04 | Truncated chain (last 2 entries removed) | `REGISTRY_TRUNCATED` |
| RG-05 | Fork (two valid tips, same prev_hash) | `REGISTRY_FORK`, writes frozen, both tips surfaced |
| RG-06 | Rollback (older head presented) | rejected via persisted head |
| RG-07 | Wrong authorizer (device signs for another) | rejected |
| RG-08 | recovery_epoch with bad proof (wrong VK) | rejected |
| RG-09 | recovery_epoch binding stale manifest after devices saw newer | rejected on those devices; recovery UI shows bound generation |
| RG-10 | TLV canonicalization: reordered tags, padded integers, non-NFC strings | decode rejects or normalizes deterministically — byte-identical re-encode required |

### 16.3 Device approval

| ID | Test | Expected |
|---|---|---|
| DA-01 | Correct iPhone signature, all fields match | one fill released |
| DA-02 | Wrong iPhone (enrolled other / unenrolled) | `DEVICE_NOT_AUTHORIZED` |
| DA-03 | Approval naming a different Mac | `APPROVAL_MISMATCH` |
| DA-04 | Approval origin ≠ pending origin | `APPROVAL_MISMATCH` |
| DA-05 | Expired challenge (t > exp) | `APPROVAL_EXPIRED` |
| DA-06 | Replayed approval (same nonce) | `APPROVAL_REPLAY` |
| DA-07 | Duplicate delivery while pending | idempotent single release |
| DA-08 | Modified challenge (1-bit flip pre-signature) | `SIGNATURE_INVALID` |
| DA-09 | iPhone revoked mid-request | reject |
| DA-10 | Offline phone → timeout | fallback offered; no fill |
| DA-11 | Card fill with yesterday's password-fill presence | fresh presence demanded (`fill_card` action) |

### 16.4 Browser security

| ID | Test | Expected |
|---|---|---|
| BR-01 | exact `https://github.com` | candidate listed |
| BR-02 | `app.github.com` vs `github.com` (exact entry) | no autofill; "similar item" hint |
| BR-03 | same with `match:"domain"` entry | eligible |
| BR-04 | `github.com.evil.example` | no match (PSL boundary) |
| BR-05 | `xn--80ak6aa92e.com` (аррӏе-style lookalike) vs `apple.com` | refuse + warning |
| BR-06 | confusable skeleton collision (`g00gle.example`) | refuse + warning |
| BR-07 | stored https, page http | refuse (except localhost+`allow_http`) |
| BR-08 | cross-origin iframe fill attempt | only matching frame filled |
| BR-09 | opaque origin (`data:` URL tab) | refuse |
| BR-10 | replay consumed `request_id` | `APPROVAL_REPLAY` |
| BR-11 | origin/tab_url disagreement (extension lies about one) | reject |
| BR-12 | candidate query rate > 20/min | rate-limited |
| BR-13 | request for `ref` belonging to another origin | `APPROVAL_MISMATCH` (origin bound at authorize time) |
| BR-14 | locked vault | `{locked:true}`, no candidates |
| BR-15 | PSL drift: `github.io`, `co.uk`, `appspot.com` corpus | identical decisions Rust vs JS |

### 16.5 Capture safety

| ID | Test | Expected |
|---|---|---|
| CS-01 | vault window visible during recording | frames dropped (lifecycle log shows suppression) |
| CS-02 | vault content never appears in OCR corpus/index/search/timeline | corpus grep for canary strings: absent |
| CS-03 | typing into vault secure fields with keyboard recorder on | zero events |
| CS-04 | typing into Chrome/1Password/Safari password fields (secure input on) | zero events |
| CS-05 | ambiguous focus context | suppresses (fail toward not recording) |
| CS-06 | reveal requested with suppression unverifiable | `CAPTURE_UNSAFE`, nothing displayed |
| CS-07 | import wizard start → end | counter up for entire wizard; report view still suppressed |
| CS-08 | asset protocol: vault path fetch at runtime | rejected (existing smoke probe, wired into CI) |

### 16.6 Backup

| ID | Test | Expected |
|---|---|---|
| BK-01 | full publish → fresh-device restore | byte-identical vault after unwrap |
| BK-02 | object bit-flip in store | hash mismatch → `BACKUP_OBJECT_MISSING`, partial-restore path with exact count |
| BK-03 | object deleted | same |
| BK-04 | manifest generation -1 presented | `MANIFEST_ROLLBACK` |
| BK-05 | two-device forked manifests | both surfaced; user picks; loser archived |
| BK-06 | stale backup + fresher peer | peer preferred; backup floor only |
| BK-07 | provider down at publish | queued, backoff, local vault fully usable |
| BK-08 | interrupted upload mid-step-2 | orphans GC'd after grace; no manifest references missing objects |
| BK-09 | CAS race (two publishers) | loser merges and republishes at gen+1 |
| BK-10 | historical snapshot: old RK + old wrap + old object bytes after rotation | old snapshot still decrypts (documents the §12.7 limitation as tested behavior); **new** state refuses old RK |
| BK-11 | provider attempts manifest signed by non-enrolled key | devices reject regardless of server acceptance |

### 16.7 Recovery scenario tests

RC-01…RC-08 execute §12 scenarios 1–8 end-to-end against `FsBackupStore`
with two simulated devices, asserting: final vault contents equal,
registry chains valid, revoked/old material fails everywhere it must
(BK-10 pattern), exposure-set log correct in RC-08, and all user-facing
copy steps occur in order.

### 16.8 Cross-language vectors

Generated by a Rust CLI (`vault-helper/src/bin/gen_vectors.rs`) into
JSON+hex files; consumed by Rust tests and Swift XCTest:

| Vector family | Contents |
|---|---|
| XV-ECDSA | P-256 sign/verify, incl. high-S rejection |
| XV-ECDH/HPKE | envelope seal/open both directions (Rust-seal→Swift-open and vice versa) |
| XV-HKDF | all §2.9 info strings |
| XV-TLV | registry entries (all kinds), approval payloads, manifests: canonical bytes + expected hashes + signatures |
| XV-SAS | enrollment transcripts → 8-char SAS |
| XV-BIP39 | RK entropy ↔ 24 words (official reference vectors + ours) |
| XV-ORIGIN | canonicalization + matching corpus (shared with JS for BR-15) |
| XV-RECOVERY-EPOCH | VK + manifest → recovery_proof |

Any wire/crypto change bumps versions and regenerates vectors in the same
commit.

---

## 17. Dependency plan

Existing usable: `sha2`, `rand` (OsRng), `serde`/`serde_json`, `zeroize`-adjacent patterns are absent — see below. `sqlx` (with the rsa-advisory transitive, RUSTSEC-2023-0071) stays **out of the helper entirely** (the helper uses rusqlite).

### 17.1 New Rust crates — vault-helper

| Crate | Version | Purpose | Maintenance / audit status | Why existing deps can't | In helper | Processes secrets |
|---|---|---|---|---|---|---|
| `argon2` | 0.5.x | MP KDF | RustCrypto, widely reviewed; no known advisories | none present | yes | yes (MP, PK) |
| `chacha20poly1305` | 0.10.x | record/wrap AEAD | RustCrypto; no known advisories | none present | yes | yes |
| `hkdf` | 0.12.x | subkeys, KEKs | RustCrypto | sha2 alone is not HKDF | yes | yes |
| `p256` | 0.13.x (`ecdsa`, `ecdh`) | device signatures/verification | RustCrypto; constant-time via `elliptic-curve` | none present | yes | pubkeys/signatures only; private ops happen in SE |
| `hpke` | 0.12.x | device envelopes (RFC 9180) | small crate (rozbb); matches RFC 9180 vectors; **flagged for manual pre-merge review** (§17.4) | composing ECDH+HKDF+AEAD by hand is the worse alternative; CryptoKit interop requires exact RFC 9180 | yes | yes (WrapPayload) |
| `zeroize` | 1.x | secret erasure | de facto standard, audited transitively everywhere | none present | yes | yes |
| `secrecy` | 0.10.x | SecretBox wrappers | widely used, tiny | complements zeroize with type-level guards | yes | yes |
| `subtle` | 2.x | constant-time compares | RustCrypto | — | yes | tokens/secrets |
| `rusqlite` | 0.32.x, `bundled` | vault.db | mainstream; bundled SQLite avoids system-lib variance; helper keeps sqlx out | repo's sqlx would drag the async+rsa-advisory graph into the helper | yes | encrypted blobs only |
| `csv` | 1.x | Dashlane parsing | BurntSushi crate, stable, no history of soundness issues | none present | yes | yes (import rows) |
| `objc2`, `objc2-foundation` | 0.5/0.2 | LA + Security framework FFI glue | obj-c2 project, actively maintained | existing `objc` 0.2 in main graph is the legacy crate; helper uses the maintained successor | yes | no (prompt plumbing) |
| `security-framework` | 3.x | Keychain items, SecCode checks | rust-mobile; **known SE key-agreement FFI gap** (v0.3 review) → thin in-crate FFI shim for `SecKeyCopyKeyExchangeResult`, reviewed as our own unsafe | none present | yes | handles wrapped blobs only |
| `base32` | 0.5.x | enroll secret encoding | tiny, stable | base64 present but QR favors base32 | yes | enrollment secret (transient) |
| `getrandom` | (via rand) | CSPRNG | — | already present | yes | yes |

Total new helper crates: 14 (plus transitive). Target: helper tree
< 120 crates total (vs ~500 in main). Measured and reported in Phase B.

### 17.2 nm-host crate dependencies

`serde`, `serde_json` only (plus std). No crypto, no secrets logic beyond
pass-through + zeroize of the fill response (`zeroize` allowed, cheap).

### 17.3 Swift (iOS app) — zero new third-party packages

CryptoKit (HPKE, P-256, HKDF), LocalAuthentication, Security, Network/
URLSession with SPKI pinning (existing pattern in the iOS repo). BIP-39
wordlist asset + in-house codec (§2.4). This keeps the iOS supply chain
at Apple-only.

### 17.4 Vetting policy for new security dependencies

- The existing 759-crate `cargo vet` exemption baseline **must not** grow
  to absorb these. Each new helper crate gets either (a) an imported
  audit (`cargo vet import` from Mozilla/Google/bytecodealliance where
  available) or (b) a recorded manual review note in
  `src-tauri/supply-chain/audits.toml` covering: unsafe usage, FFI
  surface, panic behavior on adversarial input, and maintenance pulse.
- `hpke` is explicitly called out for manual review before Phase E code
  lands; if review fails, fallback is manual composition
  (p256-ECDH + hkdf + chacha20poly1305) with RFC 9180 vectors still
  enforced on the wire so Swift interop is unchanged.
- CI gate: `cargo tree -p source-vault-helper | grep -E "^.* rsa "` must
  be empty; **RUSTSEC-2023-0071 remains outside all vault/helper build
  graphs** and any change that pulls it in is a merge blocker
  (`scripts/audit-deps.sh` gains this check in Phase A).

### 17.5 npm

Extension (separate `extension/` package, not the app frontend): `tldts`
(PSL matching; processes origins, never secrets), `vite`+`typescript`
build only. No new dependencies in the app's `package.json`.

---

## 18. Implementation order

Each phase is independently committable, tested, and stops at its gate.
The 350-line file cap applies: vault code is organized as module
directories (`vault-helper/src/{crypto,storage,registry,ipc,ops,import}/…`).

### Phase A — helper skeleton
Workspace split; `SourceVaultHelper.app` bundle plumbing; signed-build
scripting; socket + framing + peer auth (both directions); lifecycle
(launch/idle/exit/crash-restart); `hello`/`get_state`/`lock`; CI checks:
helper dep count, rsa-absence, designated-requirement verification.
**No vault, no keys — synthetic echo service behind the real boundary.**
Gate: peer-auth rejects an unsigned clone binary; helper restart leaves
state machine at LOCKED; `cargo vet` passes with new audits recorded.

### Phase B — cryptographic core
VK/wraps/records/subkeys/zeroization; BIP-39 codec; TLV codec;
gen_vectors CLI; CR-01…CR-12, RG/RG-10, XV-Rust-side.
Gate: all green; `cargo fuzz` smoke run clean (10 min).

### Phase C — local Mac vault
rusqlite storage; header/manifest; record CRUD with synthetic records;
UNLOCKED lifecycle + auto-lock; LA presence per op; capture-safe
management UI (§14 wiring incl. fail-closed reveal); CS-01…CS-07.
Gate: synthetic vault usable end-to-end in the app; capture tests green.

### Phase D — recovery
MP set/change; RK generate/print/enter; rotation engine; recovery epoch;
RC-03…RC-07 vs FsBackupStore; BK-10 historical-snapshot test.
Gate: scenarios 3–7 rehearse green on synthetic data.

### Phase E — device identity + enrollment
SE keygen (both roles, both platforms); §5 protocol + ephemeral server;
SAS; envelopes; registry live; revocation + auto-rotation (RC-01/02);
DA vectors; iOS app vault screens.
Gate: two real devices enroll/revoke/rotate green on synthetic vaults.

### Phase F — remote encrypted backup
BackupStore trait + Fs + Http impls; publication/CAS/GC; provider
service (smallest possible: object store + manifest CAS + push relay
endpoint + recovery locator); BK-01…BK-11.
Gate: kill-both-devices rehearsal on a fresh machine, synthetic data.

### Phase G — iPhone remote approval
§6.5/§6.6 + §7.1 foreground + §7.2 APNs alert path; DA-01…DA-11;
fallback UX; APNs provider side on the Phase F service.
Gate: clamshell Mac + iPhone approval demo on synthetic vault; offline
fallback demonstrated.

### Phase H — Chrome extension
extension + nm-host + manifest install; canonicalization + PSL + policy
engine; fill/save/update; BR-01…BR-15 with synthetic sites (local test
server with IDN/punycode/iframe fixtures).
Gate: synthetic fills green; extension cannot enumerate (attempts logged
+ refused).

### Phase I — Dashlane importer
§10 end to end on synthetic fixtures; fuzz corpus + `cargo fuzz` CI
budget; import UI with capture suppression + honesty copy.
Gate: 10⁵-row synthetic import < 60 s, zero panics, report clean.

### Phase J — first real credential gate
Run §19. Nothing proceeds to real data on "seems to work".

---

## 19. First real credential release gate

Every item must be verifiably green, with the named evidence:

| # | Gate item | Evidence |
|---|---|---|
| 1 | helper-process isolation working | Phase A gate; `codesign` DR checks in CI |
| 2 | IPC peer authentication working | unsigned-clone rejection test |
| 3 | strict CSP still green | smoke harness: zero `CSP VIOLATION` in dev + bundle runs |
| 4 | vault excluded from asset protocol | asset_scope_guard tests + runtime probe (CS-08) |
| 5 | vault excluded from screen capture | CS-01/02/06 |
| 6 | vault excluded from OCR/indexing | CS-02 |
| 7 | password input suppression working | CS-03/04/05 |
| 8 | cryptographic tests green | CR-01…CR-12 |
| 9 | tamper tests green | CR-05/06/10, RG-01/02, BK-02 |
| 10 | registry tests green | RG-01…RG-10 |
| 11 | key rotation tests green | CR-08/12, RC-01/02/06/07 |
| 12 | recovery tests green | RC-01…RC-08 |
| 13 | remote backup restore tested | BK-01 + Phase F rehearsal log |
| 14 | no bulk-secret API | IPC catalog audit vs §1.5 — any new op reviewed against the never-list |
| 15 | no agent vault API | route/command audit: `/v1/agent`, Tauri commands, nm ops |
| 16 | dependency security review complete | §17.4 audits recorded; helper dep count reported; rsa-absence CI green; `cargo vet` clean |
| 17 | security logs verified clean | log scrape during full test run: no secret-class strings (canary sweep) |
| 18 | synthetic Dashlane import fuzzed | Phase I gate + corpus in repo |
| 19 | Chrome origin-matching tests green | BR-01…BR-15 |
| 20 | iPhone approval replay tests green | DA-06/07/08 |
| 21 | production build signing/hardened runtime verified | `codesign --verify --deep --strict`, runtime flag 0x10000, entitlement audit vs `macos-signing-and-hardening.md`; helper carries no `disable-library-validation` |

Only then may the first real credential be imported.

---

## 20. Source ID future integration

v1: Source ID is not required, not a recovery dependency, and its failure
cannot destroy vault access (v0.3 §10.5, C13). Reserved extension points,
designed now so integration later is additive:

1. **Registry authorizer namespace:** `authorizer` field (0x0C) already
   admits non-device authorities structurally (16 zero bytes =
   recovery_epoch today); a future entry kind 5 (`source_id_attest`) can
   bind a Source ID root signature as an *additional* authorizer without
   changing existing entries.
2. **Approval action space:** §6.5 `action` u8 has headroom; Source ID
   would add an approval path alongside — never replacing — LA and iPhone
   approval.
3. **Wrap slots:** `wraps/` gains a `sourceid.wrap` at most as *one more
   independent wrap of VK*, never the only wrap; MP and RK wraps are
   never removed by Source ID integration.

Hard separation invariant (must survive integration review): the Source
ID identity root never encrypts the vault, never appears in a wrap
derivation path, and Source-controlled infrastructure gains no ability to
unwrap VK or mint devices. Any future proposal violating this is a
backdoor and must be called one in review. The full Source ID recovery
protocol is out of scope here by instruction.

---

## 21. Open technical questions

Settled product decisions are not reopened (v0.3 §19). The following are
genuine platform/protocol choices that materially shape UX or security
and are not resolvable by pure engineering convention. Everything else
encountered during implementation must be resolved by standard practice
or escalated, not decided silently.

### OQ-1 — Macs without Secure Enclave

**Issue:** pre-T2 Intel Macs have no SE; device identity keys would fall
back to software Keychain keys (extractable on full compromise).
**Why it matters:** v0.3 prefers SE-backed keys "where platform APIs
permit"; the fallback tier is explicitly called weaker (§8.1) but whether
v1 *supports* such Macs at all is a product/security cut line.
**Options:** (a) require SE-capable hardware (all Apple Silicon, T2
Intel) for device identity — refuse vault setup otherwise; (b) allow a
software-key tier with a persistent "reduced device security" badge;
(c) allow software tier silently (rejected: dishonest).
**Security implications:** (b) creates a downgrade target — an attacker
who compromises setup-time code could steer enrollment to the weak tier;
(a) is simplest and honest but excludes old hardware.
**Recommendation:** (a) for v1, with the requirement stated in onboarding;
revisit only if real users are blocked. (The Mac in this repo's
development environment is Apple Silicon and supports SE.)

### OQ-2 — Approval relay when phone and Mac share no network

**Issue:** §7.2 APNs wakes the phone, but if the phone cannot reach the
Mac directly (cellular-only, restrictive NAT), the challenge fetch fails
and the user drops to the §6.C password fallback even though both devices
are online.
**Why it matters:** APNs reachability materially changes the approval UX
success rate in exactly the clamshell scenario the feature exists for.
**Options:** (a) v1 accepts the fallback (APNs wake + direct fetch only);
(b) relay opaque, end-to-end-protected challenge/approval envelopes
through the §11 provider (provider sees envelopes + timing, contents are
signed/encrypted between enrolled devices); (c) skip APNs entirely and
require direct reachability (rejected: worse UX than (a)).
**Security implications:** (b) adds no secret exposure (envelopes are
opaque TLV, signed per §6.5, confidentiality of origin/title protected by
an AEAD under an HPKE session key between the two devices) but enlarges
the provider's role from storage to mailbox — availability and traffic
analysis surface grow.
**Recommendation:** ship (a) in v1 with the §6.5 format already
relay-safe (it is — the TLV blob is opaque to the carrier); adopt (b) as
v1.1 if fallback frequency in practice justifies it. Decide at Phase G
review with measured fallback rates from dogfooding.

### OQ-3 — Minimum iOS version (CryptoKit HPKE floor)

**Issue:** CryptoKit's HPKE API requires iOS 17+. Supporting iOS 15–16
means hand-composing ECDH+HKDF+AEAD in Swift for envelope decapsulation —
a second crypto composition path on exactly the code that handles
WrapPayload.
**Why it matters:** v0.3 forbids custom primitives and duplicate crypto
paths without re-review (§18 rules); this is an Apple platform behavior
forcing a choice.
**Options:** (a) require iOS 17+ (single HPKE path both platforms);
(b) add the Swift-side manual composition with XV-ECDH/HPKE vectors
proving RFC 9180 equivalence; (c) lower security by simplifying the wire
format for old iOS (rejected outright).
**Security implications:** (b) is standard-primitive composition but
hand-rolled assembly — the highest-risk code in the iOS app; (a) risks
nothing cryptographically and merely narrows device support.
**Recommendation:** (a) iOS 17+ for v1. Revisit (b) only with a measured
user need and a dedicated crypto review of the Swift composition.

---

*End of specification. No implementation is authorized by this document.
The next artifact requires explicit owner approval plus, before release
to other users, independent security/crypto review (v0.3 §21).*
