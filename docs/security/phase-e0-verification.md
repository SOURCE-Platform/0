# Phase E0 Verification Report — HPKE / Secure Enclave PoC (§2.12 Path A)

Date: 2026-09-20. Spec: `credential-vault-implementation-spec.md` v0.3.1
§2.12 (Path A) and its hard pre-gate to Phase E.

Scope: the interoperability PoC only. **Path A succeeded, so Path B was
not implemented and not evaluated.** Nothing else from Phase E was
started: no device enrollment, no production envelopes, no registry
enrollment flows, no iPhone approval, no provider work, no extension, no
real credentials. All data is synthetic.

Status: **PASS — all three required demonstrations green with a real
Secure-Enclave-resident key, at the exact suite.**

## 1. Suite and encodings (as required)

| Item | Value | Evidence |
|---|---|---|
| KEM | DHKEM(P-256, HKDF-SHA256) — 0x0010 | `HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, …)`; Rust `hpke::kem::DhP256HkdfSha256` |
| KDF | HKDF-SHA256 — 0x0001 | `.HKDF_SHA256` / `hpke::kdf::HkdfSha256` |
| AEAD | ChaCha20-Poly1305 — 0x0003 | `.chaChaPoly` / `hpke::aead::ChaCha20Poly1305` |
| Public key | 65-byte uncompressed X9.63 (`0x04 ‖ X ‖ Y`) | SE key's `x963Representation` is 65 bytes, first byte `0x04`, parsed directly by Rust `PublicKey::from_bytes`; asserted on both platforms |
| Encapsulated key | 65 bytes | asserted in (a) and (b); vector's `enc` is 65 bytes |
| Mode | base (0) | both sides |

## 2. Hardware, OS, toolchain

| Role | Detail |
|---|---|
| Mac | MacBook Air (Mac14,2, Apple M2), macOS 26.x, `SecureEnclave.isAvailable == true` |
| iPhone | iPhone 13 mini (iPhone14,4, A15), iOS 27.0, `SecureEnclave.isAvailable == true` |
| Swift | Apple Swift 6.3.3, target arm64-apple-macosx26.0 |
| Rust | `hpke` 0.12.0 (features `p256`, `std`), `p256` 0.13 |
| Bridge | `src-tauri/vault-apple-crypto` — SwiftPM **static** library, C ABI, **148 lines** (spec cap: ≤ 200), linked into a Rust test binary |

## 3. Results

### (a) Rust `hpke` seal → CryptoKit `HPKE.Recipient` open with the SE key — PASS

`tests/path_a.rs::rust_seal_opens_in_cryptokit_with_secure_enclave_key`.
Rust seals to the SE key's 65-byte public key; CryptoKit opens it with
`HPKE.Recipient(privateKey: SecureEnclave.P256.KeyAgreement.PrivateKey, …)`,
so the decapsulation DH runs **inside the Enclave**. Also asserted:
wrong `info` fails, and a single flipped ciphertext byte fails.

### (b) CryptoKit `HPKE.Sender` seal → Rust `hpke` open — PASS

`tests/path_a.rs::cryptokit_seal_opens_in_rust`. Rust generates the
recipient key (software, since the vector/recipient side must be openable
in Rust), CryptoKit seals to its 65-byte key, Rust opens.

### (c) CryptoKit Sender → CryptoKit Recipient with an SE key on iPhone — PASS

On-device run (`src-tauri/poc/hpke-ios`), verbatim output:

```text
iPhone14,4 · iOS 27.0
suite: DHKEM(P-256,HKDF-SHA256)/HKDF-SHA256/ChaCha20Poly1305
SecureEnclave.isAvailable: true
SE public key: 65 bytes, first byte 0x4
SE stored blob: 284 bytes (opaque, device-bound)
enc: 65 bytes; ct: 53 bytes
(c) CryptoKit seal → CryptoKit open with SE key: PASS
tampered ciphertext refused: PASS
RFC 9180 vector (CFRG 5f503c5) opens on iOS: PASS
```

### RFC 9180 vector / interoperability evidence — PASS

`tests/path_a.rs::rfc9180_vector_opens_in_both_implementations` and the
iOS line above. The vector is the **official CFRG/RFC 9180** entry for
this suite (mode base, kem 0x0010, kdf 0x0001, aead 0x0003), vendored to
`src-tauri/poc/hpke-se/vectors/rfc9180-p256-sha256-chacha20poly1305.json`
with provenance: CFRG test vectors at commit `5f503c5`, obtained from the
`hpke` 0.12.0 crate's `test-vectors-5f503c5.json`
(SHA-256 `61fc662f01996cd06d713dacf5e133167bd309a1f329442d53f1e21a47b3ede6`);
`skRm` 32 B, `pkRm` 65 B, `enc` 65 B; first four encryptions kept.

- Rust `hpke` opens the vector's sequence-0 ciphertext to the expected
  plaintext, and derives `pkRm` from `skRm`.
- Apple CryptoKit opens the **same vector bytes** to the same plaintext on
  macOS and on iOS.
- Cross-implementation interop (each side opening the other's fresh
  output) covers the encryption direction, which the published vectors
  cannot pin down because encapsulation is randomized.
- Sequence numbers 1+ were not replayed: the single-shot APIs on both
  sides do not expose the running context. The suite's key schedule is
  nevertheless pinned by the sequence-0 KAT and by two-way interop.

### Non-exportability of the agreement private key — PASS

`tests/path_a.rs::secure_enclave_key_is_not_exportable`:

- The key is created as `SecureEnclave.P256.KeyAgreement.PrivateKey`;
  Apple's type exposes **no** `rawRepresentation` (that exists only on
  software `P256` keys), so there is no API that returns the scalar.
- What is persisted is `dataRepresentation`: an **opaque 284-byte,
  device-bound blob**, not a 32-byte scalar. It does not parse as a P-256
  private key, and no 32-byte window inside it is a scalar that derives
  the key's public point (exhaustively checked).
- The same blob still decapsulates through CryptoKit — the DH happens in
  the Enclave.
- After `ov0_se_key_delete`, the key is unusable.
- Keychain storage uses `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`.

## 4. The bridge (§2.12 shape)

`src-tauri/vault-apple-crypto/Sources/VaultAppleCrypto/Bridge.swift`
(148 lines, static library, C ABI, no policy):

| Symbol | Purpose |
|---|---|
| `ov0_se_key_create` / `ov0_se_key_public` / `ov0_se_key_delete` | SE key lifecycle, keyed by keychain tag (§2.12 "SE key load by Keychain tag") |
| `ov0_hpke_seal(pub65, info, pt, aad) → (enc, ct)` | CryptoKit `HPKE.Sender` |
| `ov0_hpke_open_se(tag, info, enc, ct, aad) → pt` | CryptoKit `HPKE.Recipient` with the SE key |
| `ov0_hpke_open_sw` | software-key open, **vectors/KAT only** |
| `ov0_se_key_blob` | PoC evidence helper (returns the opaque stored blob) |

Deviation from the §2.12 sketch: the seal/open entry points also take
`aad`, because RFC 9180 (and the published vectors) authenticate
additional data per message. `ov0_hpke_open_sw` and `ov0_se_key_blob` are
PoC-only surfaces; Phase E should ship the three SE/HPKE entry points
plus key lifecycle, and drop the evidence helper.

Linking note for Phase E: the static bridge links into a Rust binary with
`-L/usr/lib/swift`, `-Wl,-rpath,/usr/lib/swift`, the CryptoKit/Foundation/
Security frameworks, and the OS Swift runtime dylibs. Proven here by the
Rust test binary; the helper will need the same link flags (and the
runtime is OS-provided on macOS 14+, the §2.12 floor).

## 5. What was committed

- `src-tauri/vault-apple-crypto/` — the bridge (Phase E will consume it).
- `src-tauri/poc/hpke-se/` — Rust PoC: FFI wrapper, tests, vendored
  vector, build script. **Its own cargo workspace on purpose**: the
  `hpke`/`p256` dependencies are *not* added to the shipping helper, so
  the supply-chain surface and `cargo vet` state are unchanged. Phase E
  must vet `hpke` under §17.4 if production code adopts it.
- `src-tauri/poc/hpke-ios/` — the on-device harness and its run script.
- This report.

No shipping code changed, so no gate re-run was required; the file-length
audit and a `cargo check` of the src-tauri workspace were run to confirm
the repo is unaffected.

## 6. Conclusion and remaining pre-Phase-E items

**Path A is viable and proven on real hardware at the exact suite.**
Path B was not implemented (§2.12: it is never the default, and only
follows a documented impossibility of A). The §2.12 hard pre-gate is
satisfied: (a), (b) and (c) are green with a real SE key, cross-checked
against the official RFC 9180 vector on macOS and iOS.

Still open before **full** Phase E is authorized:

1. **Argon2id calibration gate remains open.** The tuple
   `m=64 MiB, t=3, p=1` stays **provisional**; the supported-floor
   measurement on an **A12-class iPhone (XS/XR generation)** has not been
   done. iPhone 13 mini (A15) measured at median 89 ms / worst 124 ms
   (`argon2-calibration.md`); that does not close the gate and must not be
   described as closed or waived.
2. Release-gate items unchanged: real-printer exercise (item 29), Argon2
   freeze evidence (item 30).
3. CS-03 keystroke-suppression re-verification when macOS keystroke
   capture exists.
4. Phase E proper (SE device identities, enrollment, envelopes) still
   requires separate authorization.
