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
            try carriage(q)
            try carriage(a)
            try bodies(op: x["operation"] as! Int, status: x["status"] as! Int, request: hexOf(q["body"]), response: hexOf(a["body"]))
        }
    }

    /// `{0x01 tlv, 0x02 signature, 0x03 body}` (annex A.2.1).
    private func carriage(_ part: [String: Any]) throws {
        let c = try TLV.entry(hexOf(part["carriage"]))
        XCTAssertEqual(c.tags, [1, 2, 3])
        XCTAssertEqual(c.value(1), hexOf(part["tlv"]))
        XCTAssertEqual(c.value(2), hexOf(part["signature"]))
        XCTAssertEqual(c.value(3), hexOf(part["body"]))
    }

    /// Each body as its operation (A.3, with the 2026-10-09 errata).
    private func bodies(op: Int, status: Int, request: Data, response: Data) throws {
        if status != 0 { return try PeerWire.empty(response) }
        switch op {
        case 1: try PeerWire.empty(request); try PeerWire.hello(response)
        case 2:
            try PeerWire.stateRequest(request)
            if (try TLV.document(request))[0].tags == [1] {
                let d = try TLV.document(response)
                XCTAssertEqual(d.count, 1)
                XCTAssertEqual(d[0].tags, [1])
                try state(XCTUnwrap(d[0].value(1)))
            } else {
                let page = try PeerWire.objectsResponse(response)
                XCTAssertFalse(page.complete, "a truncated page")
                XCTAssertLessThan(page.items, (try TLV.document(request)).count - 1, "fewer answered than asked")
            }
        case 3: try PeerWire.headsRequest(request); try PeerWire.headsResponse(response)
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
        let b64url = { (s: String) throws -> Data in try XCTUnwrap(base64URL(s), "base64url without padding") }
        // §11.2: SHA-256("ov0/recovery-auth-set/v2" ‖ (class ‖ pub ‖ salt)
        // per entry, ordered by class); mp = 2, rk = 3.
        let items = try XCTUnwrap(obj["recovery_auth"] as? [[String: Any]])
        XCTAssertEqual(items.map { $0["class"] as? Int }, [2, 3])
        let auth = items.reduce(Data() + "ov0/recovery-auth-set/v2") { acc, e in
            acc + Data([UInt8(e["class"] as! Int)]) + hexOf(e["pub"]) + hexOf(e["salt"])
        }.sha256
        XCTAssertEqual(auth, hexOf((file["state"] as! [String: Any])["recovery_auth_digest"]))
        let tlv = TLV.encode([
            (1, hexOf((file["state"] as! [String: Any])["vault_id"])),
            (2, TLV.uintBytes(UInt64(obj["generation"] as! Int))),
            (3, try b64url(obj["manifest"] as! String).sha256),
            (4, try b64url(obj["checkpoint"] as! String).sha256),
            (5, auth),
        ])
        let commit = sha(Data() + "ov0/vault-state/v2", tlv)
        XCTAssertEqual(commit, hexOf(obj["state_commit"]))
        XCTAssertEqual(commit, hexOf((file["state"] as! [String: Any])["state_commit"]))
    }

    func testEmptyBodyAndZero() throws {
        let empty = file["empty_body"] as! [String: Any]
        XCTAssertEqual(hexOf(empty["hex"]), PeerWire.emptyBody)
        XCTAssertEqual(PeerWire.emptyBody.sha256, hexOf(empty["sha256"]))
        XCTAssertEqual(hexOf(file["zero_integer"]), Data([0]))
        XCTAssertEqual(TLV.uintBytes(0), Data([0]))
    }

    func testHeadsDigestRecomputes() throws {
        let d = file["heads_digest"] as! [String: Any]
        let records = (d["records"] as! [[String: Any]]).map { (hexOf($0["record_id"]), ($0["heads"] as! [String]).map { Data(hex: $0) }) }
        XCTAssertTrue(records.contains { $0.1.count == 2 } && records.contains { $0.1.count > 64 })
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
        let canonical = parse(b["canonical_record_1"]), fifo = parse(b["fifo_record_1"]), depthFirst = parse(b["depth_first_record_1"])
        XCTAssertEqual(PeerWire.canonical(canonical), canonical.map(\.id))
        XCTAssertEqual(PeerWire.canonical(depthFirst), canonical.map(\.id), "the order is a function of the graph")
        XCTAssertEqual(PeerWire.canonical(fifo), canonical.map(\.id))
        XCTAssertEqual(Set([canonical.map(\.id), fifo.map(\.id), depthFirst.map(\.id)]).count, 3, "three different orders")
        XCTAssertNotEqual(canonical.map(\.id), canonical.map(\.id).sorted { $0.lexicographicallyPrecedes($1) }, "not plain id order")
    }

    func testEveryInvalidCaseIsRefused() throws {
        let cases = try XCTUnwrap(file["invalid"] as? [[String: Any]])
        XCTAssertGreaterThanOrEqual(cases.count, 26)
        for c in cases {
            let b = hexOf(c["hex"])
            let decode: (Data) throws -> Void
            switch c["decoder"] as! String {
            case "request_tlv": decode = { _ = try PeerWire.request($0) }
            case "heads_req": decode = PeerWire.headsRequest
            case "put_counts": decode = PeerWire.putCounts
            case "revs_get_req": decode = PeerWire.revsGetRequest
            case "state_req": decode = PeerWire.stateRequest
            case "document": decode = { _ = try TLV.document($0) }
            case "heads_resp": decode = PeerWire.headsResponse
            case "objects_resp": decode = { _ = try PeerWire.objectsResponse($0) }
            case "status4_body": decode = PeerWire.empty
            case "revs_batch": decode = { _ = try PeerWire.revsBatch($0) }
            default: return XCTFail("unknown decoder \(c["decoder"]!)")
            }
            XCTAssertThrowsError(try decode(b), "\(c["rule"]!) must be refused")
        }
    }

    /// A valid answer the checks must accept (ascent is per bucket).
    func testExtraValidBodiesDecode() throws {
        for c in try XCTUnwrap(file["valid_extra"] as? [[String: Any]]) {
            XCTAssertEqual(c["decoder"] as? String, "heads_resp")
            XCTAssertNoThrow(try PeerWire.headsResponse(hexOf(c["hex"])), "\(c["rule"]!)")
        }
    }
}
