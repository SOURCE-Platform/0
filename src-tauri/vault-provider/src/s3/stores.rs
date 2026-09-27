//! `StateStore`, `BlobStore` and `OpsStore` on S3 (§11.2 key layout).

use vault_proto::crypto::hex;
use vault_provider_core::stores::{BlobStore, Etag, OpsStore, StateStore, StoreError, StoreResult};

use super::client::{S3Client, S3Response};

fn state_key(vid: &[u8; 16]) -> String {
    format!("v2/vaults/{}/state", hex::encode(vid))
}

fn blob_key(vid: &[u8; 16], sha: &[u8; 32]) -> String {
    format!("v2/vaults/{}/blobs/{}", hex::encode(vid), hex::encode(sha))
}

fn h(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

impl S3Client {
    fn get(&self, key: &str) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        let r = self.request("GET", key, &[], &[], b"").map_err(|_| StoreError)?;
        match r.status {
            200 => Ok(Some((r.body, Etag(r.etag.ok_or(StoreError)?)))),
            404 => Ok(None),
            _ => Err(StoreError),
        }
    }

    /// Conditional PUT: `None` = create-only, `Some(etag)` = replace-if-match.
    fn put(&self, key: &str, bytes: &[u8], expect: Option<&Etag>) -> StoreResult<Option<Etag>> {
        let cond = match expect {
            None => h("if-none-match", "*"),
            Some(e) => h("if-match", &e.0),
        };
        let r = self.request("PUT", key, &[], &[cond], bytes).map_err(|_| StoreError)?;
        cas_result(r)
    }

    fn list(&self, prefix: &str) -> StoreResult<Vec<(String, String)>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut q = vec![h("list-type", "2"), h("prefix", prefix)];
            if let Some(t) = &token {
                q.push(h("continuation-token", t));
            }
            let r = self.request("GET", "", &q, &[], b"").map_err(|_| StoreError)?;
            if r.status != 200 {
                return Err(StoreError);
            }
            let xml = String::from_utf8_lossy(&r.body).to_string();
            for c in xml.split("<Contents>").skip(1) {
                if let (Some(k), Some(t)) = (tag(c, "Key"), tag(c, "LastModified")) {
                    out.push((k, t));
                }
            }
            token = tag(&xml, "NextContinuationToken");
            if tag(&xml, "IsTruncated").as_deref() != Some("true") || token.is_none() {
                return Ok(out);
            }
        }
    }
}

fn cas_result(r: S3Response) -> StoreResult<Option<Etag>> {
    match r.status {
        200 => Ok(Some(Etag(r.etag.ok_or(StoreError)?))),
        409 | 412 => Ok(None), // condition failed (or a concurrent conditional write)
        _ => Err(StoreError),
    }
}

/// The text of the first `<name>…</name>` (S3 XML is simple and flat here).
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'"))
}

/// ISO 8601 `2026-09-27T15:15:18.000Z` → Unix seconds.
fn iso_secs(s: &str) -> Option<u64> {
    let d = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, da, hh, mi, ss) = (d(0, 4)?, d(5, 7)?, d(8, 10)?, d(11, 13)?, d(14, 16)?, d(17, 19)?);
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mi * 60 + ss).ok()
}

impl StateStore for S3Client {
    fn load(&self, vid: &[u8; 16]) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        self.get(&state_key(vid))
    }
    fn create(&self, vid: &[u8; 16], bytes: &[u8]) -> StoreResult<Option<Etag>> {
        self.put(&state_key(vid), bytes, None)
    }
    fn replace(&self, vid: &[u8; 16], bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>> {
        self.put(&state_key(vid), bytes, Some(etag))
    }
    fn delete(&self, vid: &[u8; 16], etag: &Etag) -> StoreResult<bool> {
        // §11.3.1 C4 rollback: conditional DELETE (`If-Match`).
        let r = self.request("DELETE", &state_key(vid), &[], &[h("if-match", &etag.0)], b"").map_err(|_| StoreError)?;
        match r.status {
            200 | 204 => Ok(true),
            409 | 412 => Ok(false),
            _ => Err(StoreError),
        }
    }
}

impl BlobStore for S3Client {
    fn put_if_absent(&self, vid: &[u8; 16], sha: &[u8; 32], bytes: &[u8]) -> StoreResult<bool> {
        Ok(self.put(&blob_key(vid, sha), bytes, None)?.is_some())
    }
    fn get(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<Option<Vec<u8>>> {
        Ok(S3Client::get(self, &blob_key(vid, sha))?.map(|(b, _)| b))
    }
    fn size(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<Option<u64>> {
        let r = self.request("HEAD", &blob_key(vid, sha), &[], &[], b"").map_err(|_| StoreError)?;
        match r.status {
            200 => r.length.map(Some).ok_or(StoreError),
            404 => Ok(None),
            _ => Err(StoreError),
        }
    }
    fn list(&self, vid: &[u8; 16]) -> StoreResult<Vec<([u8; 32], u64)>> {
        let prefix = format!("v2/vaults/{}/blobs/", hex::encode(vid));
        Ok(S3Client::list(self, &prefix)?
            .into_iter()
            .filter_map(|(k, t)| Some((hex::decode_array::<32>(k.rsplit('/').next()?)?, iso_secs(&t)?)))
            .collect())
    }
    fn delete(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<()> {
        let r = self.request("DELETE", &blob_key(vid, sha), &[], &[], b"").map_err(|_| StoreError)?;
        if matches!(r.status, 200 | 204 | 404) { Ok(()) } else { Err(StoreError) }
    }
}

impl OpsStore for S3Client {
    fn get(&self, key: &str) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        S3Client::get(self, key)
    }
    fn create(&self, key: &str, bytes: &[u8]) -> StoreResult<Option<Etag>> {
        self.put(key, bytes, None)
    }
    fn replace(&self, key: &str, bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>> {
        self.put(key, bytes, Some(etag))
    }
    fn delete(&self, key: &str) -> StoreResult<()> {
        let r = self.request("DELETE", key, &[], &[], b"").map_err(|_| StoreError)?;
        if matches!(r.status, 200 | 204 | 404) { Ok(()) } else { Err(StoreError) }
    }
    fn list_prefix(&self, prefix: &str) -> StoreResult<Vec<String>> {
        Ok(S3Client::list(self, prefix)?.into_iter().map(|(k, _)| k).collect())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn xml_and_dates() {
        let xml = "<ListBucketResult><Contents><Key>v2/a</Key><LastModified>2026-09-27T15:15:18.000Z</LastModified></Contents><IsTruncated>false</IsTruncated></ListBucketResult>";
        assert_eq!(super::tag(xml, "Key").as_deref(), Some("v2/a"));
        assert_eq!(super::iso_secs("2026-09-27T15:15:18.000Z"), Some(1_790_522_118));
    }
}
