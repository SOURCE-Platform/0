//! Simulated multi-Mac helpers (§18 Phase F clarification): enrollment
//! by an existing Mac and joining from the provider.

use super::*;

impl Mac {
    /// Simulated Mac-to-Mac enrollment (§18 Phase F clarification: the
    /// protocol under test is multi-writer publish, merge and revocation;
    /// Mac-to-Mac enrollment is unspecified): this Mac appends an enroll
    /// entry for `other`, seals its envelope, commits locally and publishes.
    pub fn enroll(&mut self, cloud: &Cloud, other: &Mac) {
        self.enroll_device(cloud, &other.dev);
    }

    /// Enroll any device identity (a Mac, or a synthetic phone) and publish.
    pub fn enroll_device(&mut self, cloud: &Cloud, other: &dyn DeviceIdentity) {
        use vault_helper::crypto::wrap::DeviceEnvelopePayload;
        use vault_helper::registry::build;
        let vid = self.vid();
        let st = self.registry();
        let entry = build::enroll(&st, &self.dev, other).unwrap();
        let after = log::append(&self.dir, &vid, &st, entry, &EpochPolicy::CheckpointAnchored).unwrap();
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).unwrap();
        let payload = DeviceEnvelopePayload {
            vk: SecretBytes::new(*self.vk.as_ref().unwrap().expose()),
            wrapped_at: now(),
            vk_generation: self.store().header.vk_generation,
        };
        let env = envelope::seal_envelope(&other.agree_pub(), &vid, &other.device_id(), &nonce, &payload).unwrap();
        envelope::write_envelope(&self.dir, &other.device_id(), &env).unwrap();
        self.store.as_mut().unwrap().set_registry_head(after.head).unwrap();
        self.publish(cloud).expect("publish enrollment");
    }

    /// A device with no local vault materializes the committed state from
    /// its own envelope (§4.7 / §2.10 catch-up).
    pub fn join(&mut self, cloud: &Cloud, vid: [u8; 16]) {
        use vault_helper::sync::join;
        use vault_proto::request::ProviderRequest;
        let none = BTreeSet::new();
        let scope = SignScope { put_blobs: &none, staged: None };
        let get = |op: Operation, blob: Option<[u8; 32]>| {
            let req = SignRequest { operation: op, blob, body_sha256: body_hash(b""), expected_state: None };
            let (pr, h): (ProviderRequest, String) = sign::sign(ORIGIN, vid, Key::Device(&self.dev), &req, &scope, 0, now()).unwrap();
            cloud.send(&pr.method, &pr.path, Some(&h), b"")
        };
        let r = get(Operation::StateGet, None);
        assert_eq!(r.status, 200, "{}", err(&r));
        let remote = remote::parse(&r.body).unwrap();
        let index_bytes = get(Operation::BlobGet, Some(remote.manifest.object_index_hash)).body;
        let index = vault_helper::backup::index::ObjectIndex::decode(&index_bytes).unwrap();
        let mut blobs = HashMap::new();
        for h in index.blobs() {
            blobs.insert(h, get(Operation::BlobGet, Some(h)).body);
        }
        let tag = self.dev.key_tag().to_string();
        let open = move |f: &envelope::DeviceEnvelopeFile| envelope::open_envelope(&tag, &vid, f);
        let vault = self.dir.join("vault");
        let (store, vk) = join::join(&vault, &remote, &index, &blobs, self.dev.device_id(), &open).expect("join");
        self.dir = vault;
        self.store = Some(store);
        self.vk = Some(vk);
    }

    pub fn rk_fresh(&self) -> SecretBytes<32> {
        random_secret()
    }
}
