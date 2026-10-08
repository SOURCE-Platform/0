// XV-PEER, the independent side (wire annex A.5): the engine's committed
// exchanges, carriage, empty body, heads digest, state commitment and
// canonical batch, checked with CryptoKit/Security only; every invalid
// case refused. Synthetic vectors.

import XCTest

final class XVPeerTests: XCTestCase {
    private var file: [String: Any] = [:]

    override func setUpWithError() throws { file = try XV.load("xv_peer") }

    private var keys: [String: Any] { file["keys"] as! [String: Any] }

    func testKeysDeriveFromTheirScalars() throws {
        XCTAssertEqual(try ECDSA.publicKey(scalar: hexOf(keys["phone_scalar_dev_only"])), hexOf(keys["phone_pub"]))
        XCTAssertEqual(try ECDSA.publicKey(scalar: hexOf(keys["mac_scalar_dev_only"])), hexOf(keys["mac_pub"]))
    }

    func testEveryExchangeVerifies() throws {
        let exchanges = try XCTUnwrap(file["exchanges"] as? [[String: Any]])
        XCTAssertEqual(Set(exchanges.map { $0["operation"] as! Int }), [1, 2, 3, 4, 5, 6, 9])
        for x in exchanges {
            let name = x["name"] as! String
            let q = x["request"] as! [String: Any], a = x["response"] as! [String: Any]
            let req = try PeerWire.request(hexOf(q["tlv"]))
            XCTAssertEqual(TLV.encode(req), hexOf(q["tlv"]))
            let reqPrehash = sha(Data() + "ov0/peer/request/v1", hexOf(q["tlv"]))
            XCTAssertEqual(reqPrehash, hexOf(q["prehash"]), name)
            XCTAssertEqual(req.value(2), hexOf(keys["vault_id"]))
            XCTAssertEqual(req.value(3), hexOf(keys["phone_device_id"]))
            XCTAssertEqual(req.value(4), hexOf(keys["mac_device_id"]))
            XCTAssertEqual(try TLV.uint(req.value(5)!), UInt64(x["operation"] as! Int))
            XCTAssertEqual(req.value(6), hexOf(q["body"]).sha256, "body_sha256")
            XCTAssertTrue(ECDSA.isLowS(hexOf(q["signature"])))
            XCTAssertTrue(ECDSA.verify(prehash: reqPrehash, signature: hexOf(q["signature"]), publicKey: hexOf(keys["phone_pub"])), name)
            XCTAssertFalse(ECDSA.verify(prehash: reqPrehash, signature: hexOf(q["signature"]), publicKey: hexOf(keys["mac_pub"])))

            let resp = try PeerWire.response(hexOf(a["tlv"]))
            let respPrehash = sha(Data() + "ov0/peer/response/v1", hexOf(a["tlv"]))
            XCTAssertEqual(respPrehash, hexOf(a["prehash"]))
            XCTAssertNotEqual(respPrehash, reqPrehash)
            XCTAssertEqual(resp.value(3), hexOf(keys["mac_device_id"]))
            XCTAssertEqual(resp.value(4), hexOf(keys["phone_device_id"]))
            XCTAssertEqual(resp.value(5), reqPrehash, "the response names the request it answers")
            XCTAssertEqual(try TLV.uint(resp.value(6)!), UInt64(x["status"] as! Int))
            XCTAssertEqual(resp.value(7), hexOf(a["body"]).sha256)
            XCTAssertTrue(ECDSA.isLowS(hexOf(a["signature"])))
            XCTAssertTrue(ECDSA.verify(prehash: respPrehash, signature: hexOf(a["signature"]), publicKey: hexOf(keys["mac_pub"])), name)
            try bodies(op: x["operation"] as! Int, status: x["status"] as! Int, request: hexOf(q["body"]), response: hexOf(a["body"]))
        }
    }

    /// Each body as its operation (A.3).
    private func bodies(op: Int, status: Int, request: Data, response: Data) throws {
        if status != 0 { return try PeerWire.empty(response) }
        switch op {
        case 1: try PeerWire.hello(request); try PeerWire.hello(response)
        case 2:
            try PeerWire.stateRequest(request)
            let d = try TLV.document(response)
            if (try TLV.document(request))[0].tags == [1] {
                XCTAssertEqual(d.count, 1)
                try state(XCTUnwrap(d[0].value(1)))
            } else {
                try PeerWire.check(d[0], [1: 0])
                for e in d.dropFirst() {
                    if e.tags == [1, 5] { try PeerWire.check(e, [1: 32, 5: 1]) } else { try PeerWire.check(e, [1: 32, 2: 0, 3: 0, 4: e.value(4)?.count ?? -1]) }
                }
            }
        case 3:
            try PeerWire.headsRequest(request)
            let d = try TLV.document(response)
            XCTAssertTrue(d[0].tags == [1] || d[0].tags == [1, 2])
            for e in d.dropFirst() {
                if e.tags == [1, 5] { try PeerWire.check(e, [1: 16, 5: 1]) } else { XCTAssertEqual(e.tags, [1, 2]); _ = try PeerWire.ids(e.value(2)!, max: 64) }
            }
        case 4:
            try PeerWire.revsGetRequest(request)
            let revs = try PeerWire.revsBatch(response)
            XCTAssertEqual(Set(revs.map(\.record)).count, 2, "two records in one batch")
        case 5: _ = try PeerWire.revsBatch(request, put: true); try PeerWire.putCounts(response)
        case 6: try PeerWire.empty(request); try PeerWire.status(response)
        default: XCTFail("operation \(op) answered with status 0")
        }
    }

    /// A.3.2: the state re-encoded in a fixed key order; its commitment
    /// recomputes from the fields alone.
    private func state(_ json: Data) throws {
        let text = try XCTUnwrap(String(data: json, encoding: .utf8))
        let keys = ["generation", "vk_generation", "state_commit", "manifest", "checkpoint", "recovery_auth"]
        let positions = try keys.map { try XCTUnwrap(text.range(of: "\"\($0)\":")?.lowerBound) }
        XCTAssertEqual(positions, positions.sorted(), "A.3.2 key order")
        XCTAssertFalse(text.contains(" ") || text.contains("\n"), "no whitespace")
        let obj = try XCTUnwrap(JSONSerialization.jsonObject(with: json) as? [String: Any])
        let b64url = { (s: String) -> Data in
            var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
            while t.count % 4 != 0 { t += "=" }
            return Data(base64Encoded: t)!
        }
        XCTAssertEqual((obj["recovery_auth"] as? [Any])?.count, 0)
        let tlv = TLV.encode([
            (1, hexOf((file["state"] as! [String: Any])["vault_id"])),
            (2, TLV.uintBytes(UInt64(obj["generation"] as! Int))),
            (3, b64url(obj["manifest"] as! String).sha256),
            (4, b64url(obj["checkpoint"] as! String).sha256),
            (5, sha(Data() + "ov0/recovery-auth-set/v2")),
        ])
        let commit = sha(Data() + "ov0/vault-state/v2", tlv)
        XCTAssertEqual(commit, hexOf(obj["state_commit"]))
        XCTAssertEqual(commit, hexOf((file["state"] as! [String: Any])["state_commit"]))
    }

    func testEmptyBodyZeroAndCarriage() throws {
        let empty = file["empty_body"] as! [String: Any]
        XCTAssertEqual(hexOf(empty["hex"]), PeerWire.emptyBody)
        XCTAssertEqual(PeerWire.emptyBody.sha256, hexOf(empty["sha256"]))
        XCTAssertEqual(hexOf(file["zero_integer"]), Data([0]))
        XCTAssertEqual(TLV.uintBytes(0), Data([0]))
        let c = try TLV.entry(hexOf((file["carriage"] as! [String: Any])["hex"]))
        XCTAssertEqual(c.tags, [1, 2, 3])
        let hello = ((file["exchanges"] as! [[String: Any]])[0])["request"] as! [String: Any]
        XCTAssertEqual(c.value(1), hexOf(hello["tlv"]))
        XCTAssertEqual(c.value(2), hexOf(hello["signature"]))
        XCTAssertEqual(c.value(3), hexOf(hello["body"]))
    }

    func testHeadsDigestRecomputes() throws {
        let d = file["heads_digest"] as! [String: Any]
        let records = (d["records"] as! [[String: Any]]).map { (hexOf($0["record_id"]), ($0["heads"] as! [String]).map { Data(hex: $0) }) }
        XCTAssertTrue(records.contains { $0.1.count == 2 })
        XCTAssertEqual(Set(records.map { Int($0.0.sha256[0]) }), [d["bucket"] as! Int], "one bucket")
        let digest = PeerWire.headsDigest(records)
        XCTAssertEqual(digest, hexOf(d["digest"]))
        XCTAssertEqual(digest.sha256, hexOf(d["digest_sha256"]))
        let e = d["empty_bucket"] as! Int
        XCTAssertEqual(digest.subdata(in: e * 32..<e * 32 + 32), Data().sha256)
    }

    func testCanonicalOrderDiffersFromDepthFirst() throws {
        let b = file["revs_batch"] as! [String: Any]
        let parse = { (v: Any?) in (v as! [[String: Any]]).map { (id: hexOf($0["revision_id"]), parents: ($0["parents"] as! [String]).map { Data(hex: $0) }) } }
        let canonical = parse(b["canonical_record_1"]), depthFirst = parse(b["depth_first_record_1"])
        XCTAssertEqual(PeerWire.canonical(canonical), canonical.map(\.id))
        XCTAssertEqual(PeerWire.canonical(depthFirst), canonical.map(\.id), "the order is a function of the graph")
        XCTAssertNotEqual(depthFirst.map(\.id), canonical.map(\.id))
    }

    func testEveryInvalidCaseIsRefused() throws {
        let cases = try XCTUnwrap(file["invalid"] as? [[String: Any]])
        XCTAssertGreaterThanOrEqual(cases.count, 15)
        for c in cases {
            let b = hexOf(c["hex"])
            let decode: (Data) throws -> Void
            switch c["decoder"] as! String {
            case "request_tlv": decode = { _ = try PeerWire.request($0) }
            case "heads_req": decode = PeerWire.headsRequest
            case "put_counts": decode = PeerWire.putCounts
            case "revs_get_req": decode = PeerWire.revsGetRequest
            case "state_req": decode = PeerWire.stateRequest
            case "empty_body": decode = PeerWire.empty
            case "revs_batch": decode = { _ = try PeerWire.revsBatch($0) }
            default: return XCTFail("unknown decoder \(c["decoder"]!)")
            }
            XCTAssertThrowsError(try decode(b), "\(c["rule"]!) must be refused")
        }
    }
}
