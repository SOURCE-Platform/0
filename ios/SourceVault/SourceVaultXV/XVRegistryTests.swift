// XV-TLV and XV-ECDSA, read without the engine: registry entry hashes
// and signing inputs recomputed from the canonical bytes, signatures
// verified under the device key, and the low-S rule. Synthetic vectors.

import XCTest

final class XVRegistryTests: XCTestCase {
    /// entry_hash = SHA-256("ov0/registry/entry/v1" ‖ tlv); sign_input =
    /// SHA-256("ov0/registry/sign/v1" ‖ tlv without its terminal 0x10
    /// signature field); the signature verifies under device A's key.
    func testRegistryEntriesHashAndVerify() throws {
        let file = try XV.load("xv_tlv")
        let deviceA = try ECDSA.publicKey(scalar: hexOf(file["device_a_scalar"]))
        let vectors = try XCTUnwrap(file["vectors"] as? [[String: Any]])
        XCTAssertEqual(vectors.count, 4)
        for v in vectors {
            let tlv = hexOf(v["tlv"])
            let entry = try TLV.entry(tlv)
            XCTAssertEqual(TLV.encode(entry), tlv, "canonical: re-encoding reproduces the bytes")
            XCTAssertEqual(sha(Data() + "ov0/registry/entry/v1", tlv), hexOf(v["entry_hash"]), "\(v["note"]!)")
            guard let signature = v["signature"] as? String else {
                XCTAssertTrue(v["sign_input"] is NSNull, "recovery_epoch is never signed")
                XCTAssertNil(entry.value(0x10))
                continue
            }
            XCTAssertEqual(entry.last?.tag, 0x10, "the signature is the terminal field")
            XCTAssertEqual(entry.value(0x10), Data(hex: signature))
            let signInput = sha(Data() + "ov0/registry/sign/v1", TLV.encode(Array(entry.dropLast())))
            XCTAssertEqual(signInput, hexOf(v["sign_input"]))
            XCTAssertTrue(ECDSA.isLowS(Data(hex: signature)))
            XCTAssertTrue(ECDSA.verify(prehash: signInput, signature: Data(hex: signature), publicKey: deviceA))
        }
    }

    /// The low-S signature verifies; its high-S twin is mathematically
    /// valid but refused by the low-S rule.
    func testEcdsaLowSRule() throws {
        let file = try XV.load("xv_ecdsa")
        let pub = hexOf(file["pubkey_uncompressed"])
        XCTAssertEqual(try ECDSA.publicKey(scalar: hexOf(file["signing_scalar_dev_only"])), pub)
        let digest = hexOf(file["digest"])
        XCTAssertEqual(digest, sha(Data() + "ov0/xv/ecdsa/v1"))
        let low = hexOf(file["signature_low_s"]), high = hexOf(file["signature_high_s_twin_must_reject"])
        XCTAssertTrue(ECDSA.isLowS(low) && ECDSA.verify(prehash: digest, signature: low, publicKey: pub))
        XCTAssertFalse(ECDSA.isLowS(high), "the twin is high-S")
        XCTAssertEqual(low.prefix(32), high.prefix(32), "same r")
    }
}
