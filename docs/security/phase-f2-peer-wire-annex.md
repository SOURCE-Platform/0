# Phase F.2 — peer wire annex (spec §22.8)

Date: 2026-10-01. **Revision 3** (after the spec review of revision 1, `d9a2f13`, and
the bounded re-review of revision 2, `6260127`). **Status: candidate; no F.2c peer code before it closes.**
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
  authentication is answered with signed status 4. *(Erratum, 2026-10-09,
  review SPEC-I7 of `9d7fc3d`: status 4 is for **bodies**. The same rule
  broken in `request_tlv` fails the §22.8 canonical parse and is an
  unsigned `403` (`PEER_AUTH_INVALID`); broken in a response's
  `response_tlv` or body, the requester reads it as "unable to verify".)*
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
  evaluates on an authenticated request — every size, count or quota cap —
  is the signed status 2** (§22.8), never `413`; only the per-sender
  request rate is the unsigned `429`.
- Main binds this route only on private-network interfaces and refuses
  public source addresses (§22.8). Main never builds a vault response.

### A.2.2 IPC (Mac main → helper)

- **Inline** when the body is **≤ 24 KiB** (so the whole JSON frame,
  base64 included, stays under the §1.3 64 KiB cap): `peer_serve
  {request_tlv, signature, body}` → `{response_tlv, signature, body}` (a
  response body ≤ 24 KiB), or `{refused: 403 | 429 | 503}`.
- **Large request:** `peer_serve_begin {request_tlv, signature, size}`.
  The helper runs the whole §22.8 receiver order except the body hash —
  canonical parse, vault, receiver, signature, time, **rate limit**,
  replay, who may speak, the state gate — before it accepts a single body
  byte. It answers `{session, need: [body_sha256]}`, **or** `{refused:
  403 | 429 | 503}`, **or** a complete signed response with status 1
  (op 5 while COMPROMISED; ops 2–5 while behind, §22.14), status 2 (`size`
  over 1 MiB or another cap) or status 4, carrying the empty body. A
  stream that fails (`TRANSFER_INVALID`, staging deleted) is `{refused:
  403}`. Main streams the body with `stream_begin` /
  `stream_write` / `stream_end` (§1.3; cap 1 MiB). Then `peer_serve
  {session}`: the helper **re-checks who may speak and the state gate**,
  verifies the body hash (mismatch → `{refused: 403}`), and **consumes**
  the session (single use).
- **Large response** (> 24 KiB): `{session, response_tlv, signature,
  stream: sha256, size}`, read with `stream_read {session, sha256,
  offset}`; `session_close` ends it. An inline request may get a session
  for its response this way.
- A **peer session** is its own §1.3 session kind: at most two open (a
  third `peer_serve_begin` gets the signed status 2); idle 60 s; allowed
  in the §22.8 serving states — so the §1.3 stream ops are allowed in
  those states **for a peer session only** (§1.5, §13.2); it never changes the reported
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

*(Erratum, 2026-10-09, review SPEC-I1: the **request** carries the empty
body — the Mac never uses a requester's floors (§22.8 "Direction"), and
the shipped responder answers any other request body with status 4. The
§22.8 table's "floors … and the heads digest →" names the response.)*
Response, header entry only: `{0x01 registry_seq (u64),
0x02 registry_head (32 B), 0x03 committed_generation (u64; 0x00 before
the first commit), 0x04 committed_manifest_hash (32 B; zeros before the
first commit), 0x05 heads_digest (8192 B)}`.

### A.3.2 `peer_state` (2) — two modes

The Mac keeps the **verified `state_get` body** (≤ 64 KiB) with `seen`, in
the same commit that accepts it (review SPEC-I4). After its own
publication commits (the provider returns no body) it has none until its
next `state_get`, and declines meanwhile.

- **State mode.** Request header `{0x01 have_generation}`. Response
  header `{0x01 state}` — that body **re-encoded from its verified fields
  only** (review SEC-O2 / VER-I16: UTF-8 JSON, keys in the order
  `generation`, `vk_generation`, `state_commit` (hex), `manifest`,
  `checkpoint` (base64), `recovery_auth` [`class`, `pub`, `salt`], no
  whitespace; *(clarified 2026-10-09, SPEC-I3: `manifest` and `checkpoint`
  are base64url without padding; each `recovery_auth` item is
  `{"class": 2 (mp) | 3 (rk) (§11.2 class codes), "pub": hex 65 B, "salt": hex 16 B}`, ordered
  by class; `generation` and `vk_generation` are also checked against the
  verified manifest, SPEC-O7)* the requester recomputes `state_commit` from the fields, so
  the encoding carries no trust) — or status 3 when its
  committed generation ≤ `have_generation` or it holds no body. The phone
  never reads status 3 as "up to date" (it only means "not from me").
- **Objects mode.** Request header `{0x02 state_commit}` (no
  `have_generation`), then ≤ 64 entries `{0x01 sha256, 0x02 offset}`
  (`offset` u64, **always present**, `0x00` for the start), ascending by
  `sha256`. If `state_commit` is not the
  responder's current `seen.state_commit` → status 3. Response header
  `{0x01 complete}`, then per object, in request order, either `{0x01
  sha256, 0x02 offset, 0x03 total_len, 0x04 bytes}` (a chunk; ≤ 4 MiB per
  chunk; an object may span several requests — **byte ranges**, so an
  8 MiB index fits the caps; a chunk that ends before `total_len` does
  not by itself set `complete = 0`) or an unavailable entry `{0x01 sha256, 0x05
  reason}`. Only objects referenced by that committed state (manifest,
  checkpoint, index, index-listed blobs) are served, and only if the full
  object's SHA-256 matches; the requester reassembles, checks the hash,
  and verifies everything as a provider state (§22.5, v0.4.1 order) —
  provisional.

### A.3.3 `peer_heads` (3) — whole buckets

- Request header `{0x01 buckets}` (1–256 distinct bucket numbers, **one
  byte each**, ascending).
- Response header `{0x01 complete}`, then, **bucket by bucket in the
  requested order, each bucket whole**, one entry per record in ascending
  `record_id`: `{0x01 record_id, 0x02 heads}` (`heads`: 1–64 **concatenated** 32-byte
  `revision_id`s, ascending). *(Clarified 2026-10-09: the header's
  `0x02 buckets` ascend and are a subset of the request.)* An empty requested bucket simply has no
  entries; the response header carries `0x02 buckets` — the bucket
  numbers fully covered — so empty and omitted buckets are told apart.
  The responder stops at the first bucket that does not fit. A bucket that does not fit is left out entirely and
  `complete = 0`; the requester re-asks the buckets it did not receive.
- **Servable heads.** A record's heads are the heads of its **servable
  subgraph** — the revisions the responder may serve under the §22.7
  freshness rule — so `peer_heads`, `peer_revs_get` and the heads digest
  agree. A record with no servable revision is absent from all three. A
  record with more than 64 heads is reported as `{0x01 record_id, 0x05
  reason = 4}` and left to the provider.

### A.3.4 `peer_revs_get` (4)

- Request: empty header entry, then ≤ 512 entries `{0x01 record_id, 0x02
  have_heads (absent when none; ≤ 64 concatenated 32-byte ids,
  ascending)}`, ascending `record_id`.
- *(Clarified 2026-10-09, SPEC-I5/I6: only requested records appear; the
  unavailable entries `{0x02 record_id, 0x05 reason}` come **after all
  objects**, ascending by `record_id`.)*
- Response: header `{0x01 complete}`, then the revisions **grouped by
  record in ascending `record_id`; within a record in the order of Kahn's
  algorithm that always emits the smallest ready `revision_id`** (the
  canonical order; a batch in any other order is `FORMAT_INVALID`, so
  both implementations must produce exactly this):
  `{0x01 object}` (the §3.7 `OV0OBJ02` bytes). A record's closure is
  never split: the responder stops at the first closure that does not
  fit and sets `complete = 0`; one that would not fit even in an
  otherwise empty response (> 2,000 revisions or over the response cap)
  is `{0x02 record_id, 0x05 reason = 1}`; one withheld by the freshness rule is reason 3.

### A.3.5 `peer_revs_put` (5)

- Request: empty header entry, then objects in the same canonical order.
  An object larger than **1 MiB − 4 KiB** cannot travel on the peer path
  (the request cap is 1 MiB) and goes through the provider.
- Response header `{0x01 admitted, 0x02 waiting, 0x03 refused}` (u64). A
  LOCKED receiver stores the objects in its bounded inbox and answers
  `{0x01 0x00, 0x02 n, 0x03 0x00}`.

### A.3.6 `peer_status` (6)

Request: empty body. Response header `{0x01 vault_id, 0x02 registry (the
§4 registry file bytes of the responder's local chain), 0x03
committed_seq, 0x04 committed_generation, 0x05 committed_manifest_hash}`.

## A.4 Provisioning and state

- **`peer_endpoint`** (added to the §5.2 bundle by main, JSON):
  `{spki_sha256: hex (64 chars), token: base64url without padding of 32
  bytes from the OS RNG, port: u16, host_hints: [string] (IP literals of
  the Mac's private-network addresses, at most 8)}`. The phone stores the
  pin and token in its Keychain (`WhenUnlockedThisDeviceOnly`, §2.8).

- **`pending_remote.target_device_ids`** (review SPEC-B2; replaces a
  single `target_device_id` in §11.3.2): one entry per revocation in the
  pending change; each stays until **that** revocation settles or is
  redone. **`awaiting_redo` entries are per target** —
  `revocation(device_id)` rather than the bare kind (§11.3.2) — so
  redoing one revocation clears only its own entry and a second
  revocation never erases the first's. Who may speak reads the whole
  list.
- **Behind while LOCKED.** The §22.14 floor check needs no vault key, so a
  LOCKED Mac evaluates it before serving too: a behind Mac answers only
  `peer_hello` and `peer_status` in every state.
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
CryptoKit-only Swift target (§22.2): **one request and one response body
per operation** (including the `peer_revs_put` counts and the LOCKED
reply, and the `peer_status` body); one `PeerRequest` and one
`PeerResponse` with prehashes and a low-S signature under a fixed test
key; the HTTP carriage entry; the empty body and its hash; a zero integer;
a heads digest with two records in one bucket (one with two heads) and an
empty bucket; a `peer_revs_get` batch whose canonical order differs from
depth-first order; the absent-offset form as an invalid case; both `peer_state` modes, including a byte-range chunk; a
`complete = 0` page and an unavailable entry; a status-4 response; a
two-record `peer_revs_get` batch in canonical order; one invalid case per
A.1 rule.

## A.6 Tests added (also listed in spec §22.16 and §19 item 31)

| ID | Test | Expected |
|---|---|---|
| PW-01 | carriage, inline and streamed, both directions, at the 24 KiB boundary | identical results; no frame over 64 KiB |
| PW-02 | body validation | every A.1 rule → status 4, nothing applied |
| PW-03a | streamed request with a bad envelope, a replay, an unknown or revoked sender, COMPROMISED op 5, `size` over the cap | answered at `peer_serve_begin` (refusal or signed status 1/2), before any body byte |
| PW-03b | an orphan `stream_begin`; a revocation landing while the body streams; a failed stream | refused at completion (`403`); session single use |
| PW-04 | objects mode | only objects of the named committed state; never bytes whose hash differs; wrong `state_commit` → status 3; byte ranges reassemble an 8 MiB index |
| PW-05 | paging | whole buckets; `complete = 0` re-asks converge; unavailable items never repeat as truncation |
| PW-06 | token scope | peer token only on the peer route, header only; SOURCE Mobile token refused there; token bound to `sender_device_id`; public source address refused; all before any helper call |
| PW-07 | replay cache | survives a restart |
| PW-08 | peer sessions | at most two; idle 60 s; no state overlay; lock aborts |
| PW-09 | rate limit and per-exchange cap | rate: unsigned `429`, counted after the signature (a forged sender spends nothing); exchange cap: signed status 2 |
| PW-10 | two pending revocations, both adopted away, one redone | neither target may speak; the other's redo warning stays |
| PW-11 | a behind Mac, LOCKED and UNLOCKED | ops 2–5 get status 1; hello and status are answered |
| PW-12 | refusal mappings and token lifecycle | `413` over the transport maximum; `503` not serving / cannot sign; a revoked phone's token still reaches `peer_status` |
