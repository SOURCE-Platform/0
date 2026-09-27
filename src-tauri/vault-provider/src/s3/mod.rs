//! S3 backends for the three provider stores (spec v0.4 §11.1–§11.2):
//! blobs `If-None-Match: *`, the state object and handle claims by
//! `If-Match` on the previous ETag (ETags are concurrency tokens only),
//! nonces and rate-limit slots create-only. Horizontal scaling is safe
//! because every CAS lives in S3.

pub mod client;
pub mod sigv4;
pub mod stores;

pub use client::{S3Client, S3Config};
