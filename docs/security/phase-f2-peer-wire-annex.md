# Phase F.2 — peer wire annex (spec §22.8)

Date: 2026-10-01. **Revision 2** (after the spec review of revision 1,
`d9a2f13`). **Status: candidate; no F.2c peer code before it closes.**
Normative once accepted. It refines the bodies of the §22.8 operations and
their carriage; it changes nothing §22.8 fixes (the signed
`PeerRequest` / `PeerResponse` envelopes, the verification order, who may
speak, the gating and the caps). Everything below carries ciphertext and
public data only.

## A.1 Encoding

- **Envelopes and carriage entries** (`request_tlv`, `response_tlv`, the
  HTTP carriage entry) are single §4.2 **Entries** — `Field* ‖ 0xFF`, no
  `0x00` wrapper — exactly like `ProviderRequest` (§11.4); prehashes are
  over those bytes.
- **Operation bodies** are §4.2 **Documents** (`0x00 ‖ Len(u32be) ‖
  Entry*`). The first entry is the **header entry**; list items follow,
  one entry each, in the order stated. An **empty body** is the Document
  with one empty header entry, `00 00000001 FF`; its SHA-256 is what
  `body_sha256` carries. (A "zero-length HTTP body" — used only for
  unsigned refusals — is a different thing.)
- Integers are minimal big-endian; **zero is the single byte `0x00`**
  (never empty, as `tlv.rs` encodes). Flags (`complete`) are u8 0/1.
  `have_generation`, `committed_generation`, `committed_seq`,
  `registry_seq` are u64. `head_count` inside the heads digest is a
  **fixed** 2-byte big-endian u16.
- Byte strings are raw: `record_id` 16 B (uuid), `revision_id`, hashes and
  heads 32 B, device ids 16 B. A list that could be empty is an **absent
  field**, never an empty one (§4.2).
- Within an entry tags are ascending, each at most once. An unknown tag, a
  missing required tag, a wrong fixed length, an empty value, trailing
  bytes or a list out of its stated order is `FORMAT_INVALID` (status 4)
  and nothing is applied. An unknown `operation` code after successful
  authentication is answered with signed status 4.
- **Heads digest** (§22.8): an empty bucket's digest is `SHA-256("")`.
  The heads it covers are the responder's **servable** heads (§A.3.3).
- A response with status 1–4 carries the empty body.

## A.2 Carriage

### A.2.1 HTTP (phone → Mac main app)

- `POST /v1/vault/peer` on the Mac main app's **long-lived mobile server**
  (TLS key `mobile/key.pem`), TLS pinned to `peer_endpoint.spki_sha256`;
  `Content-Type: application/octet-stream`.
- **Peer token (own scope, review SPEC-B1).** `Authorization: Bearer
  <token>` with the `peer_endpoint` token — **header only** (a query-string
  token is refused). Peer tokens live in their **own store**, separate from
  `mobile_devices`: main keeps `SHA-256(token)` with the registry
  `device_id` it was issued to and compares in constant time. A peer token
  is accepted **only** on `POST /v1/vault/peer`; a SOURCE Mobile token
  (including a camera-free one) is refused there, and a peer token is
  refused on every other route. Main checks that the request's
  `sender_device_id` (read from `request_tlv`, before any helper call) is
  the device the token was issued to.
- **Token lifecycle (review SPEC-I8).** A peer token survives that
  device's revocation — `peer_status` is how a revoked phone learns of it
  (§22.9) — and is removed only by an explicit "Forget this device" on the
  Mac, or when that device re-enrolls under a new identity.
- **Discovery (review SPEC-I7).** `peer_endpoint` carries `host_hints`
  (the Mac's private-network addresses at enrollment) and `port`; the
  phone may also browse mDNS `_source-vault._tcp`. Both are untrusted
  hints: the SPKI pin decides.
- Request body: one carriage Entry `{0x01 request_tlv, 0x02 signature
  (64 B), 0x03 body}`. Response `200`: `{0x01 response_tlv, 0x02
  signature, 0x03 body}`.
- **Unsigned refusals** (zero-length HTTP body), all read by the phone as
  "unable to verify": `401` token missing, wrong, from another scope, or
  not issued to `sender_device_id` (main, before any helper call); `413`
  an HTTP body over **1 MiB + 4 KiB** (main, before any helper call);
  `403` any authentication failure the helper reports, and a body that
  does not hash to `body_sha256`; `429` the per-sender rate limit; `503`
  the helper is not in a serving state, or cannot sign right now
  (Keychain / Secure Enclave unavailable, §1.6). **Every cap the helper
  evaluates on an authenticated request is the signed status 2** (§22.8),
  never `413`.
- Main binds this route only on private-network interfaces and refuses
  public source addresses (§22.8). Main never builds a vault response.

### A.2.2 IPC (Mac main → helper)

- **Inline** when the body is **≤ 24 KiB** (so the whole JSON frame,
  base64 included, stays under the §1.3 64 KiB cap): `peer_serve
  {request_tlv, signature, body}` → `{response_tlv, signature, body}` (a
  response body ≤ 24 KiB), or `{refused: 403 | 429 | 503}`.
- **Large request:** `peer_serve_begin {request_tlv, signature}`. The
  helper runs the whole §22.8 receiver order except the body hash —
  canonical parse, vault, receiver, signature, time, **rate limit**,
  replay, who may speak, the state gate (and refuses op 5 while
  COMPROMISED) — before it accepts a single body byte → `{session, need:
  [body_sha256]}`. Main streams the body with `stream_begin` /
  `stream_write` / `stream_end` (§1.3; cap 1 MiB). Then `peer_serve
  {session}`: the helper **re-checks who may speak and the state gate**,
  verifies the body hash (mismatch → `{refused: 403}`), and **consumes**
  the session (single use).
- **Large response** (> 24 KiB): `{session, response_tlv, signature,
  stream: sha256, size}`, read with `stream_read {session, sha256,
  offset}`; `session_close` ends it. An inline request may get a session
  for its response this way.
- A **peer session** is its own §1.3 session kind: at most two open; idle
  60 s; allowed in the §22.8 serving states; it never changes the reported
  state (no BACKING_UP / SYNCING overlay); **lock aborts it** (staging
  deleted, §1.3).
- The rate limit (60 requests per minute per sender) is counted **after
  the signature verifies**, so a forged `sender_device_id` cannot spend a
  real phone's quota. The per-exchange cap of §22.8 is applied as **64 MiB
  of response bodies per sender in any 10 minutes**.

## A.3 Operation bodies

Items a responder cannot serve are never silently dropped: they are listed
as **unavailable** entries with a reason, so `complete = 0` only ever
means "truncated by a cap — ask again", and an unavailable item is fetched
from the provider instead. Reasons (u8): 1 too large for the peer path,
2 not held / bytes differ, 3 withheld by the §22.7 freshness rule, 4 more
than 64 heads.

### A.3.1 `peer_hello` (1)

Request and response, header entry only: `{0x01 registry_seq (u64),
0x02 registry_head (32 B), 0x03 committed_generation (u64; 0x00 before
the first commit), 0x04 committed_manifest_hash (32 B; zeros before the
first commit), 0x05 heads_digest (8192 B)}`.

### A.3.2 `peer_state` (2) — two modes

The Mac keeps the **verified `state_get` body** (≤ 64 KiB) with `seen`, in
the same commit that accepts it (review SPEC-I4). After its own
publication commits (the provider returns no body) it has none until its
next `state_get`, and declines meanwhile.

- **State mode.** Request header `{0x01 have_generation}`. Response
  header `{0x01 state}` — that body, byte for byte — or status 3 when its
  committed generation ≤ `have_generation` or it holds no body. The phone
  never reads status 3 as "up to date" (it only means "not from me").
- **Objects mode.** Request header `{0x01 have_generation, 0x02
  state_commit}`, then ≤ 64 entries `{0x01 sha256, 0x02 offset (u64,
  absent = 0)}`, ascending by `sha256`. If `state_commit` is not the
  responder's current `seen.state_commit` → status 3. Response header
  `{0x01 complete}`, then per object, in request order, either `{0x01
  sha256, 0x02 offset, 0x03 total_len, 0x04 bytes}` (a chunk; ≤ 4 MiB per
  chunk; an object may span several requests — **byte ranges**, so an
  8 MiB index fits the caps) or an unavailable entry `{0x01 sha256, 0x05
  reason}`. Only objects referenced by that committed state (manifest,
  checkpoint, index, index-listed blobs) are served, and only if the full
  object's SHA-256 matches; the requester reassembles, checks the hash,
  and verifies everything as a provider state (§22.5, v0.4.1 order) —
  provisional.

### A.3.3 `peer_heads` (3) — whole buckets

- Request header `{0x01 buckets}` (1–256 distinct bucket numbers,
  ascending).
- Response header `{0x01 complete}`, then, **bucket by bucket in the
  requested order, each bucket whole**, one entry per record in ascending
  `record_id`: `{0x01 record_id, 0x02 heads}` (`heads`: 1–64 ascending
  `revision_id`s). A bucket that does not fit is left out entirely and
  `complete = 0`; the requester re-asks the buckets it did not receive.
- **Servable heads.** A record's heads are reported after the §22.7
  freshness rule: local-only heads the responder may not serve are
  omitted, so `peer_heads`, `peer_revs_get` and the heads digest agree. A
  record with more than 64 heads is reported as `{0x01 record_id, 0x05
  reason = 4}` and left to the provider.

### A.3.4 `peer_revs_get` (4)

- Request: empty header entry, then ≤ 512 entries `{0x01 record_id, 0x02
  have_heads (absent when none; ≤ 64, ascending)}`, ascending
  `record_id`.
- Response: header `{0x01 complete}`, then the revisions **grouped by
  record in ascending `record_id`; within a record in topological order,
  ties broken by ascending `revision_id`** (a canonical order; the
  receiver additionally checks only that parents precede children):
  `{0x01 object}` (the §3.7 `OV0OBJ02` bytes). A record's closure is
  never split: one that does not fit is left out with `complete = 0`; one
  that can never fit (> 2,000 revisions or > 8 MiB) is `{0x02 record_id,
  0x05 reason = 1}`; one withheld by the freshness rule is reason 3.

### A.3.5 `peer_revs_put` (5)

- Request: empty header entry, then objects in the same canonical order.
  An object larger than **1 MiB − 4 KiB** cannot travel on the peer path
  (the request cap is 1 MiB) and goes through the provider.
- Response header `{0x01 admitted, 0x02 waiting, 0x03 refused}` (u64). A
  LOCKED receiver stores the objects in its bounded inbox and answers
  `{0x00, n, 0x00}`.

### A.3.6 `peer_status` (6)

Request: empty body. Response header `{0x01 vault_id, 0x02 registry (the
§4 registry file bytes of the responder's local chain), 0x03
committed_seq, 0x04 committed_generation, 0x05 committed_manifest_hash}`.

## A.4 State

- **`pending_remote.target_device_ids`** (review SPEC-B2; replaces a
  single `target_device_id` in §11.3.2): one entry per revocation in the
  pending change; each stays until **that** revocation settles or is
  redone. Awaiting-redo is tracked per target, not per operation kind, so
  a second revocation never erases the first's. Who may speak reads the
  whole list.
- **Replay cache:** a `peer_replay (sender BLOB, n BLOB, received_at
  INTEGER, PRIMARY KEY (sender, n))` table added in the `user_version = 3`
  migration (§22.7), written **before** the body is processed, pruned by
  local receive time beyond 600 s. A restored older `vault.db` is not the
  "cannot load" case of §22.8 (it loads; it is merely older), and §22.14
  governs it.
- **Re-pairing** after a new mobile-server key means a full §5
  re-enrollment of the phone as a new device identity (with the usual
  revocation and rotation of the old one, §22.3).

## A.5 Vectors (XV-PEER)

Committed JSON+hex vectors, checked by the Rust engine and by the
CryptoKit-only Swift target (§22.2): one `PeerRequest` and one
`PeerResponse` with prehashes and a low-S signature under a fixed test
key; the HTTP carriage entry; the empty body and its hash; a zero integer;
a heads digest with two records in one bucket (one with two heads) and an
empty bucket; both `peer_state` modes, including a byte-range chunk; a
`complete = 0` page and an unavailable entry; a status-4 response; a
two-record `peer_revs_get` batch in canonical order; one invalid case per
A.1 rule.

## A.6 Tests added (also listed in spec §22.16 and §19 item 31)

| ID | Test | Expected |
|---|---|---|
| PW-01 | carriage, inline and streamed, both directions, at the 24 KiB boundary | identical results; no frame over 64 KiB |
| PW-02 | body validation | every A.1 rule → status 4, nothing applied |
| PW-03 | streamed request: bad envelope, replay, unknown or revoked sender, COMPROMISED op 5, an orphan `stream_begin`, a revocation landing while the body streams | refused before any body byte, or at completion; session single use |
| PW-04 | objects mode | only objects of the named committed state; never bytes whose hash differs; wrong `state_commit` → status 3; byte ranges reassemble an 8 MiB index |
| PW-05 | paging | whole buckets; `complete = 0` re-asks converge; unavailable items never repeat as truncation |
| PW-06 | token scope | peer token only on the peer route, header only; SOURCE Mobile token refused there; token bound to `sender_device_id`; public source address refused; all before any helper call |
| PW-07 | replay cache | survives a restart |
| PW-08 | peer sessions | at most two; idle 60 s; no state overlay; lock aborts |
| PW-09 | rate limit and per-exchange cap | counted after the signature; a forged sender spends nothing |
| PW-10 | two pending revocations, one adopted away | neither target may speak (`target_device_ids`) |
