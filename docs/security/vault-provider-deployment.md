# Vault backup service: setup guide

Status: the service is built and tested on this Mac against local storage.
It is **not deployed**. Deploying it needs your accounts and two decisions
from you (below). No real password is ever stored by it in readable form.

## In plain words

The backup service is a small program that stores **locked copies** of your
vault. It can never read your passwords: everything it holds is scrambled
before it leaves your Mac, and only your devices, your master password or
your Recovery Key can unscramble it.

To run it you need two things:

1. **Storage** — an Amazon S3 "bucket" (an online folder) that holds the
   locked copies.
2. **A place to run the program** — any service that runs a standard
   container (for example AWS App Runner, Fly.io, Render or Railway). It
   gives the service a web address such as `https://vault.yourdomain.com`.

Then the SOURCE app is told that web address (it is built into the app, so
a fake address can never trick it), and backups start automatically.

## Decisions needed from you

- **The web address (origin)** of the service, e.g. `https://vault.example.com`.
  It is compiled into the vault helper's allowlist
  (`vault-helper/src/storage/header.rs`, `RELEASE_ORIGINS`). Until it is set,
  release builds refuse to create or recover a vault rather than guess.
- **Which hosting service** runs the container, and the AWS account/region
  for the bucket.

## Setup steps (technical)

### 1. S3 bucket (spec v0.4 §11.1)

- Create a dedicated bucket in one region.
- **Block Public Access**: on (all four settings).
- **Versioning**: on.
- **Lifecycle rules**:
  - prefix `v2/nonces/` → expire current objects after 2 days;
  - prefix `v2/ratelimit/` → expire current objects after 2 days;
  - abort incomplete multipart uploads after 1 day;
  - expire non-current versions after 30 days.
- The service needs S3 **conditional writes** (`If-None-Match: *` and
  `If-Match` on PUT). Conditional DELETE is used only for the rare
  handle-claim rollback of a losing `create`.

### 2. IAM

A role (or user) scoped to that bucket only: `s3:GetObject`,
`s3:PutObject`, `s3:DeleteObject`, `s3:ListBucket`. Prefer a role attached
to the container host over long-lived keys.

### 3. Locate pepper

The only secret the service holds. It has **no vault authority**; it only
makes answers for unknown recovery names look like real ones.

```bash
openssl rand -hex 32
```

Store it in the host's secret manager as `PROVIDER_PEPPER`.

### 4. Container

Build from the `src-tauri` directory, since the service uses two sibling crates:

```bash
docker build -f vault-provider/Dockerfile -t vault-provider .
```

Environment:

| Variable | Value |
|---|---|
| `PROVIDER_ORIGIN` | the public origin, e.g. `https://vault.example.com` |
| `PROVIDER_PEPPER` | 64 hex characters (step 3) |
| `PROVIDER_STORE` | `s3` |
| `S3_BUCKET`, `AWS_REGION` | the bucket and its region |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` | from the role, or injected by the host |
| `PROVIDER_PORT` | optional, default 8787 |

The service speaks plain HTTP; the host terminates TLS with a normal
public certificate (the app uses standard certificate checks, no pinning).

### 5. Garbage collection

Run once a day, as a scheduled job with the same environment:

```bash
vault-provider gc
```

It keeps the current and two previous states of every vault, everything
they reference, and anything younger than 7 days.

### 6. Logging

The service logs the route, status, vault id and key id only — never
request or response bodies.

## What was tested

- **Server logic** (`vault-provider-core`, 21 tests):
  - create, publish, finalize;
  - replays;
  - handle claims under crash and race;
  - the master-password recovery throttle across two instances;
  - fake recovery lookups;
  - garbage collection.
- **HTTP layer** (`vault-provider`): a real loopback server. Recovery lookup, the generic "not authorized" answer, unknown routes, oversized bodies and the `Date` header are all correct.
- **S3 request signing**: compared with an independent Python implementation of AWS Signature V4.
- **Supply chain:** the service has its own lockfile (186 crates, kept separate from the Mac app and the vault helper). `cargo audit` found no known vulnerabilities.
- **Not yet tested:** a real S3 bucket. That needs your AWS account (step 1).
