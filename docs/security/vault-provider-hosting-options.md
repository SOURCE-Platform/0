# Where the vault backup could live — hosting options

Date: 2026-09-29. Status: research for an owner decision; nothing is
deployed. Plain-language summary first, technical detail after.

## The short version

**The backup server does not need to be trusted with secrets.** It only
ever holds locked (encrypted) data and public keys; it never sees your
master password, your Recovery Key or the vault key, and every device
checks what it sends back (that checking was hardened again in spec
v0.4.1). So choosing a host is about three other things:

1. **Not losing the backup** (durability).
2. **Being reachable** when a device syncs or when you recover.
3. **One precise technical ability**: when two devices save at the same
   moment, the server must be able to say "only replace the current
   version if it is still the one you saw" (a *compare-and-swap*). Without
   it, two devices could overwrite each other.

**Recommendation.** Start with a small conventional setup — a tiny server
plus a storage service that supports compare-and-swap (for example
Cloudflare R2 or Amazon S3). It is the cheapest to run, the easiest to
secure and monitor, and the easiest for a pen tester to assess.
Decentralized storage can be added later for the bulk of the locked data,
because the provider was built with the "current version" record kept
separate from the locked data blobs. A fully decentralized setup is
possible but not recommended for the first release (reasons below).

## What the provider actually stores

| Part | Size | What it needs |
|---|---|---|
| Locked data blobs (records, wraps, envelopes, registry, index) | nearly all of the storage | write-once by content hash; read; delete old ones (garbage collection) |
| The vault's "current version" record | one small object per vault | compare-and-swap (`If-Match` / `If-None-Match`), strong consistency |
| Recovery-name claims, replay nonces, rate-limit slots | tiny objects | create-only and compare-and-swap, conditional delete |
| The provider program itself | one small container | runs 24/7, holds its storage credentials and a server-side secret (the pepper) |

The code already separates these (`StateStore`, `BlobStore`, `OpsStore`
in `vault-provider-core`), so the parts can live in different places.

## Options

### A. Conventional (recommended to start)

A small container host (e.g. Fly.io, Hetzner, DigitalOcean, AWS) running
`vault-provider`, with an S3-compatible bucket that supports conditional
writes:

- **Amazon S3** — supports `If-None-Match: *` and `If-Match` conditional
  writes.
- **Cloudflare R2** — documents conditional `PutObject` with `If-Match` /
  `If-None-Match` and strong read-after-write consistency; no egress fees.
  A community question asks how R2 behaves for concurrent conditional
  writers, so this should be confirmed by a test before relying on it.

Pros: cheap (a few dollars a month at this scale), mature, easy to monitor
and back up, well understood by pen testers. Cons: a single company can
see traffic metadata (when devices sync, object sizes) and could lose or
withhold data — which is why the design never relies on the host for
confidentiality and why the printed Recovery Key sheet carries a freshness
checkpoint.

### B. Hybrid — decentralized storage for the locked data

Keep the tiny "current version" record and the claims/rate-limit objects
on a strongly consistent store (option A), and put the locked data blobs
on a decentralized network:

- **Sia** — the Sia Foundation's `s3d` S3-compatible renter documents that
  `PutObject` honors `If-Match` / `If-None-Match` "evaluated atomically with
  the write". Data is erasure-coded across independent hosts. You run the
  renter yourself (it needs a Sia wallet and storage contracts), which is
  extra operational work.
- **Storj** — S3-compatible; its compatibility table lists `PutObject`,
  versioning and object lock but does not document conditional writes, so
  it would only be suitable for the write-once blobs, not the
  compare-and-swap parts.
- **Filebase** — S3-compatible front end; its public docs do not state
  conditional-write support or which network backs a given bucket.

Pros: no single company holds all the locked data; often cheaper per GB.
Cons: more moving parts, slower reads for recovery, and availability
depends on the network's economics. Worth doing once the first version is
stable.

### C. Fully decentralized

- **Akash Network** (decentralized container hosting): storage persists
  only for the lifetime of a lease and is lost if the deployment moves or
  the lease ends, so it cannot be the durable store; and the host operator
  can read the container's credentials. Usable only as compute in front of
  durable storage elsewhere.
- **Internet Computer (ICP)**: canister smart contracts give replicated,
  atomic state and storage (about $0.43 per GiB-month). It could host the
  whole provider, but it would be a rewrite of `vault-provider` as a
  canister and a new security review.
- **Arweave / IPFS pinning / Filecoin**: content-addressed and often
  permanent or public. Permanence is a poor fit: the design deletes old
  wraps on purpose (an old Recovery Key opens only the retired copies), and
  permanent storage would keep them forever.

## What a pen test should cover, whichever option is chosen

The provider's own API (authentication, replay, rate limits, claims), the
storage credentials and the pepper on the host, denial of service
(availability of recovery), and — most important — the devices' handling of
a *malicious* provider, which is where the v0.4.1 review findings were.

## Sources

- [AWS S3 conditional writes (announcement write-up)](https://simonwillison.net/2024/Nov/26/s3-conditional-writes/)
- [Cloudflare R2 S3 extensions](https://developers.cloudflare.com/r2/api/s3/extensions/) and [R2 release notes](https://developers.cloudflare.com/r2/platform/changelog); [community question on concurrent conditional writes](https://community.cloudflare.com/t/are-r2-conditional-putobject-predicates-atomic-for-concurrent-writers/960811)
- [Sia Foundation s3d](https://github.com/SiaFoundation/s3d)
- [Storj S3 compatibility](https://storj.dev/dcs/api/s3/s3-compatibility)
- [Filebase S3 API overview](https://filebase.com/docs/s3-api/overview)
- [Akash persistent storage discussion](https://github.com/orgs/akash-network/discussions/328)
- [Internet Computer canister storage](https://docs.internetcomputer.org/building-apps/canister-management/storage)
