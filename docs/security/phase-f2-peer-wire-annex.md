# Phase F.2 — peer wire annex (spec §22.8)

Date: 2026-09-30. **Status: candidate for review; no F.2c peer code before
it closes.** Normative once accepted. It refines the bodies of the §22.8
operations and their carriage; it changes nothing §22.8 fixes (the signed
`PeerRequest` / `PeerResponse` envelopes, the verification order, who may
speak, the gating and the caps). Everything below carries ciphertext and
public data only.

## A.1 Conventions

- Every body is a §4.2 TLV **Document** (`Tag 0x00 ‖ Len ‖ Entry*`). The
  first entry is the body's **header entry**; list items follow as one
  entry each, in the order stated. Within an entry, tags are ascending and
  each appears at most once; an unknown tag, a missing required tag, a
  wrong fixed length, trailing bytes or a list out of order is
  `FORMAT_INVALID` (status 4) and nothing is applied.
- `record_id` is the 16-byte uuid; `revision_id`, hashes and
  `registry_head` are 32 bytes; device ids 16 bytes; integers minimal
  big-endian.
- An empty body is the Document with one empty header entry
  (`00 00000001 FF`); its SHA-256 is what `body_sha256` carries.
- Sizes: request body ≤ 1 MiB, response body ≤ 8 MiB (§22.8). Where a
  response would exceed its cap, the responder sends a complete prefix
  and sets `complete = 0` in the header entry; the requester asks again.

## A.2 Carriage

### A.2.1 HTTP (phone → Mac main app)

- `POST /v1/vault/peer` on the Mac's long-lived mobile server, TLS pinned
  to the SPKI hash from `peer_endpoint` (§22.8), header
  `Authorization: Bearer <token>`, `Content-Type:
  application/octet-stream`.
- Request body: one TLV entry `{0x01 request_tlv, 0x02 signature (64 B),
  0x03 body}`. Response `200`: `{0x01 response_tlv, 0x02 signature,
  0x03 body}`.
- Unsigned refusals (§22.8): `401` wrong or missing bearer token (checked
  by main before any helper call); `403` any authentication failure the
  helper reports; `429` the per-sender rate; `413` over a size cap. All
  with an empty body. Main returns what the helper decided and never
  builds a vault response itself.
- Main binds this route only on private-network interfaces and refuses
  public source addresses (§22.8).

### A.2.2 IPC (Mac main → helper)

- Small request (body ≤ 48 KiB): `peer_serve {request_tlv, signature,
  body}` (base64 fields) → `{response_tlv, signature, body}` or
  `{refused: 403 | 429}`.
- Large request: `peer_serve_begin {request_tlv, signature}` — the helper
  runs the whole §22.8 receiver order **except the body hash** (canonical
  parse, vault, receiver, signature, time, replay, who may speak) before
  it accepts a single body byte — → `{session, need: [body_sha256]}`;
  main streams the body with `stream_begin` / `stream_write` /
  `stream_end` (§1.3 rules; cap 1 MiB); then `peer_serve {session}`.
- Large response (> 48 KiB): `{response_tlv, signature, stream: sha256,
  size}`, read with `stream_read`; `session_close` ends it.
- A peer session is its own §1.3 session kind: at most two open; idle
  60 s; allowed in the §22.8 serving states; it never changes the
  reported state (no BACKING_UP / SYNCING overlay).

## A.3 Operation bodies

### A.3.1 `peer_hello` (1)

Request and response, header entry only:

| Tag | Field |
|---|---|
| 0x01 | `registry_seq` — committed registry head seq (u64) |
| 0x02 | `registry_head` (32 B) |
| 0x03 | `committed_generation` (u64; 0 before the first commit) |
| 0x04 | `committed_manifest_hash` (32 B; zeros before the first commit) |
| 0x05 | `heads_digest` (8192 B, §22.8) |

### A.3.2 `peer_state` (2) — split into two modes

- **State mode.** Request header `{0x01 have_generation}`. Response header
  `{0x01 state (bytes: the provider's `state_get` response body for the
  responder's committed state, verbatim, ≤ 64 KiB)}`, or status 3 when
  the responder's committed generation ≤ `have_generation` or it cannot
  forward byte-for-byte.
- **Objects mode.** Request header `{0x01 have_generation, 0x02
  state_commit (32 B)}` followed by one entry per wanted object
  `{0x01 sha256}` (≤ 64, ascending). Response header `{0x01 complete}`
  followed by `{0x01 sha256, 0x02 bytes}` per object, in request order,
  only for objects referenced by the committed state named by
  `state_commit` (its manifest, checkpoint, index, and index-listed
  blobs). A responder serves an object only if `SHA-256(bytes)` equals the
  requested hash; otherwise it omits it and sets `complete = 0`. The
  requester verifies everything as a provider state (§22.5, v0.4.1 order)
  and treats the result as provisional.

### A.3.3 `peer_heads` (3)

- Request header `{0x01 buckets}`: 1–256 distinct bucket numbers as bytes,
  ascending.
- Response header `{0x01 complete}`, then one entry per record in those
  buckets, ascending `record_id`: `{0x01 record_id, 0x02 heads}` where
  `heads` is 1–64 concatenated `revision_id`s, ascending. A record with
  more than 64 heads is frozen by §3.2 and sent with its first 64 and
  `complete = 0`.

### A.3.4 `peer_revs_get` (4)

- Request: header `{}`, then ≤ 512 entries `{0x01 record_id, 0x02
  have_heads}` (`have_heads`: 0–64 concatenated ids, ascending), ascending
  `record_id`.
- Response: header `{0x01 complete}`, then one entry per revision
  `{0x01 object}` (the §3.7 `OV0OBJ02` object bytes), **parents before
  children**, a record's closure (§22.7) never split: a record whose
  closure does not fit is left out whole and `complete = 0`. ≤ 2,000
  revisions. Local-only revisions only under the §22.7 freshness rule.

### A.3.5 `peer_revs_put` (5)

- Request: the same shape as the `peer_revs_get` response.
- Response header `{0x01 admitted, 0x02 waiting, 0x03 refused}` (u64
  counts). A LOCKED receiver stores the objects in its bounded inbox and
  answers `{0x01 0, 0x02 n, 0x03 0}`.

### A.3.6 `peer_status` (6)

- Request: empty body.
- Response header: `{0x01 vault_id, 0x02 registry (the §4 registry file
  bytes of the responder's local chain), 0x03 committed_seq, 0x04
  committed_generation, 0x05 committed_manifest_hash}`.

## A.4 Provisioning and state

- **`peer_endpoint`** (added to the §5.2 bundle by main): `{spki_sha256:
  hex, token: base64 (32 random bytes)}`. Main stores only
  `SHA-256(token)` with the paired device id and compares in constant
  time; a new mobile-server key means re-pairing. The phone keeps both
  items in its Keychain (`WhenUnlockedThisDeviceOnly`, §2.8). The token
  authorizes reaching the route, never anything in the vault.
- **Replay cache** (§22.8): a `peer_replay (sender BLOB, n BLOB, t INTEGER,
  PRIMARY KEY (sender, n))` table in `vault.db`, written **before** the
  body is processed, pruned beyond 600 s; the engine uses the same table.
- **`pending_remote.target_device_id`** (§11.3.2): set by `revoke_device`
  in the same journal as the revocation; kept while the revocation is
  pending and while it awaits redo; cleared when it settles. Who-may-speak
  reads it.
- **Rate limit:** 60 requests per minute per sender, in helper memory
  (reset on restart is acceptable: the cap limits load, not authority).

## A.5 Vectors (XV-PEER)

Committed JSON+hex vectors, checked by the Rust engine and by the
CryptoKit-only Swift target (§22.2): one `PeerRequest` and one
`PeerResponse` with their prehashes and a low-S signature under a fixed
test key; a `heads_digest` over three records (one with two heads); one
body of each operation; one invalid case per rule in A.1.

## A.6 Tests added

PW-01 carriage (inline and streamed, both directions); PW-02 body
validation (every A.1 rule → status 4, nothing applied); PW-03 a large
request whose envelope fails is refused before any body byte is accepted;
PW-04 `peer_state` objects mode serves only objects of the named committed
state and never bytes whose hash differs; PW-05 `complete = 0` paging;
PW-06 bearer-token and source-address refusals before any helper call;
PW-07 replay cache survives a restart.
