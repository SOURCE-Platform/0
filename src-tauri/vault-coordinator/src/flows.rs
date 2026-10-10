//! The coordinator's flows (spec v0.4 §11.3, §11.5, §11.8): a staged
//! publication (create / publish / finalize) uploaded and posted; a sync
//! (offer → index → blobs → apply); `backup_now` with the §11.3 merge
//! loop; and the network half of total-loss recovery. Every provider
//! request is signed by the helper for exactly that request.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vault_proto::b64;
use vault_proto::crypto::hex;
use vault_proto::request::body_hash;

use crate::{code_of, ok, Failure, Helper, HttpResponse, Outcome, Transport};

/// §11.3: re-stage at most this many times after STATE_MOVED.
const MAX_MERGES: usize = 3;
const CHUNK: usize = 24 * 1024;

pub struct Flows<'a> {
    pub helper: &'a dyn Helper,
    pub transport: &'a dyn Transport,
}

impl Flows<'_> {
    /// Ask the helper to sign one request, then send it.
    pub fn call(&self, operation: &str, sha: Option<&str>, body: &[u8], expected: Option<&str>) -> Outcome<HttpResponse> {
        let mut f = json!({ "op": "sign_provider_request", "operation": operation, "body_sha256": hex::encode(body_hash(body)) });
        if let Some(s) = sha {
            f["sha256"] = json!(s);
        }
        if let Some(e) = expected {
            f["expected_state"] = json!(e);
        }
        let s = ok(self.helper.op(f))?;
        let (origin, method, path) = (s["origin"].as_str().unwrap_or(""), s["method"].as_str().unwrap_or(""), s["path"].as_str().unwrap_or(""));
        self.transport.send(origin, method, path, s["auth"].as_str(), body).map_err(|_| Failure::Unreachable)
    }

    fn pull(&self, session: &str, sha: &str) -> Outcome<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let r = ok(self.helper.op(json!({ "op": "stream_read", "session": session, "sha256": sha, "offset": out.len() })))?;
            out.extend(b64::decode(r["data"].as_str().unwrap_or("")).ok_or_else(|| Failure::Helper("TRANSFER_INVALID".into()))?);
            if r["eof"] == true {
                return Ok(out);
            }
        }
    }

    fn push(&self, session: &str, bytes: &[u8]) -> Outcome<()> {
        let sha = hex::encode(Sha256::digest(bytes));
        let s = ok(self.helper.op(json!({ "op": "stream_begin", "session": session, "sha256": sha, "size": bytes.len() })))?;
        let id = s["stream_id"].as_str().unwrap_or("").to_string();
        for (seq, chunk) in bytes.chunks(CHUNK).enumerate() {
            ok(self.helper.op(json!({ "op": "stream_write", "session": session, "stream_id": id, "seq": seq, "offset": seq * CHUNK, "data": b64::encode(chunk) })))?;
        }
        ok(self.helper.op(json!({ "op": "stream_end", "session": session, "stream_id": id }))).map(|_| ())
    }

    /// Upload a staged transition's blobs, post its body, report back.
    /// Every outcome after staging reaches the helper — success, a
    /// provider refusal, or an unreachable provider (status 0) — so it
    /// counts attempts (§11.3.2, RU-04) and keeps a transiently failed
    /// publication for a byte-identical re-send.
    pub fn run_publication(&self, p: &Value) -> Outcome<Value> {
        let session = p["session"].as_str().unwrap_or("");
        let upload = self.upload(p, session);
        let (status, code) = match &upload {
            Ok(_) => return upload,
            Err(Failure::Provider(s, c)) => (*s, c.clone()),
            Err(Failure::Unreachable) => (0, "BACKUP_UNAVAILABLE".to_string()),
            Err(_) => return upload, // a helper-side refusal: nothing was posted
        };
        let body = json!({ "error": code }).to_string();
        let _ = self.helper.op(json!({ "op": "backup_commit_result", "session": session, "status": status, "body": body }));
        upload
    }

    fn upload(&self, p: &Value, session: &str) -> Outcome<Value> {
        let expected = p["expected_state"].as_str();
        if p["kind"] != "create" {
            let mut page = 0;
            loop {
                let list = ok(self.helper.op(json!({ "op": "backup_blob_list", "session": session, "page": page })))?;
                for b in list["blobs"].as_array().into_iter().flatten() {
                    let sha = b["sha256"].as_str().unwrap_or("");
                    let bytes = self.pull(session, sha)?;
                    // §1.3: main verifies SHA-256 before upload.
                    if hex::encode(Sha256::digest(&bytes)) != sha {
                        return Err(Failure::Helper("TRANSFER_INVALID".into()));
                    }
                    let r = self.call("blob_put", Some(sha), &bytes, None)?;
                    if r.status != 200 {
                        return Err(Failure::Provider(r.status, code_of(&r.body)));
                    }
                }
                if list["more"] != true {
                    break;
                }
                page += 1;
            }
        }
        let tb = ok(self.helper.op(json!({ "op": "backup_transition_body", "session": session })))?;
        let body = match tb["body"].as_str() {
            Some(b) => b64::decode(b).ok_or_else(|| Failure::Helper("TRANSFER_INVALID".into()))?,
            None => self.pull(session, tb["stream"].as_str().unwrap_or(""))?,
        };
        let r = self.call("state_commit", None, &body, expected)?;
        if r.status != 200 {
            return Err(Failure::Provider(r.status, code_of(&r.body)));
        }
        ok(self.helper.op(json!({ "op": "backup_commit_result", "session": session, "status": 200, "body": String::from_utf8_lossy(&r.body) })))
    }

    /// Offer the provider's state and feed the helper what it asks for.
    /// Returns the helper's final answer (`up_to_date`, a merge report, or
    /// in RECOVERING the FR-01 preview).
    pub fn run_sync(&self) -> Outcome<Value> {
        self.sync_as(false)
    }

    /// `run_sync` started by the user: a key change that needs the master
    /// password may ask for it (§2.7). A background sync gets
    /// `MP_ADOPTION_REQUIRED` instead (review SEC-I3).
    pub fn run_sync_by_user(&self) -> Outcome<Value> {
        self.sync_as(true)
    }

    fn sync_as(&self, interactive: bool) -> Outcome<Value> {
        let s = self.call("state_get", None, b"", None)?;
        if s.status != 200 {
            return Err(Failure::Provider(s.status, code_of(&s.body)));
        }
        let offer = ok(self.helper.op(json!({ "op": "backup_state_offer", "state": String::from_utf8_lossy(&s.body) })))?;
        if offer["up_to_date"] == true {
            return Ok(offer);
        }
        let session = offer["session"].as_str().unwrap_or("").to_string();
        let mut need = strings(&offer["need"]);
        loop {
            for h in &need {
                let r = self.call("blob_get", Some(h), b"", None)?;
                if r.status != 200 {
                    let _ = self.helper.op(json!({ "op": "session_close", "session": session }));
                    return Err(Failure::Provider(r.status, code_of(&r.body)));
                }
                self.push(&session, &r.body)?;
            }
            let applied = ok(self.helper.op(json!({ "op": "backup_apply", "session": session, "interactive": interactive })))?;
            match applied["need"].as_array() {
                Some(n) if !n.is_empty() => need = strings(&applied["need"]),
                _ => return Ok(applied),
            }
        }
    }

    /// Publish the unlocked vault; on STATE_MOVED merge and re-stage, at
    /// most three times (§11.3), then `BACKUP_CONFLICT`.
    pub fn backup_now(&self) -> Outcome<Value> {
        self.backup_as(false)
    }

    /// `backup_now` started by the user ("Back up now").
    pub fn backup_now_by_user(&self) -> Outcome<Value> {
        self.backup_as(true)
    }

    fn backup_as(&self, interactive: bool) -> Outcome<Value> {
        for _ in 0..=MAX_MERGES {
            let prep = match ok(self.helper.op(json!({ "op": "backup_prepare" }))) {
                // §22.14: a store older than this Mac has seen publishes
                // nothing; syncing is what lets it catch up.
                Err(Failure::Helper(code)) if code == "VAULT_BEHIND" => {
                    let synced = self.sync_as(interactive)?;
                    return Ok(json!({ "nothing_to_publish": true, "behind": true, "sync": synced }));
                }
                other => other?,
            };
            if prep["nothing_to_publish"] == true {
                return Ok(prep);
            }
            match self.run_publication(&prep) {
                Err(Failure::Provider(409, code)) if code == "STATE_MOVED" => {
                    self.sync_as(interactive)?;
                }
                other => return other,
            }
        }
        Err(Failure::Conflict)
    }

    /// §11.5 locate (unauthenticated): the provider never sees the raw
    /// handle, only `handle_key`.
    pub fn locate(&self, origin: &str, handle_text: &str) -> Outcome<Vec<u8>> {
        let normalized = vault_proto::handle::normalize(handle_text).map_err(|_| Failure::Helper("INVALID_INPUT".into()))?;
        let body = json!({ "handle_key": hex::encode(vault_proto::handle::handle_key(&normalized)) }).to_string();
        let r = self.transport.send(origin, "POST", "/v2/recover/locate", None, body.as_bytes()).map_err(|_| Failure::Unreachable)?;
        if r.status == 200 {
            Ok(r.body)
        } else {
            Err(Failure::Provider(r.status, code_of(&r.body)))
        }
    }

    /// Total-loss recovery, network half (§11.8): locate → the helper's
    /// `recovery_begin` (its panel collects the MP/RK) → download and
    /// verify → FR-01 preview. `recovery_finish` runs after the user saw
    /// the preview.
    pub fn recovery_start(&self, origin: &str, handle_text: &str, kind: &str) -> Outcome<Value> {
        let locate = self.locate(origin, handle_text)?;
        ok(self.helper.op(json!({ "op": "recovery_begin", "kind": kind, "handle": handle_text, "locate_response": String::from_utf8_lossy(&locate) })))?;
        self.run_sync()
    }

    pub fn recovery_finish(&self) -> Outcome<Value> {
        let staged = ok(self.helper.op(json!({ "op": "recovery_complete" })))?;
        self.run_publication(&staged)
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|h| h.as_str().map(String::from)).collect()
}
