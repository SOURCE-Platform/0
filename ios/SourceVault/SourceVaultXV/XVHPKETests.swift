// XV-HPKE-SE, the parts a simulator can run: CryptoKit at the exact §2.12
// suite (KEM 0x0010, KDF 0x0001, AEAD 0x0003) opens the RFC 9180 vector,
// and the v2 envelope's `info` is rebuilt from its parts. The Secure
// Enclave legs (Rust-seal → SE-open, Apple-seal → SE-open) need a device
// key: the §2.12 PoC (`poc/hpke-se`) and the device run cover them.

import CryptoKit
import XCTest

final class XVHPKETests: XCTestCase {
    private let suite = HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, kdf: .HKDF_SHA256, aead: .chaChaPoly)

    func testRfc9180VectorOpensAtTheExactSuite() throws {
        let file = try XV.load("rfc9180-p256-sha256-chacha20poly1305")
        let v = try XCTUnwrap(file["vector"] as? [String: Any])
        XCTAssertEqual(v["kem_id"] as? Int, 0x10)
        XCTAssertEqual(v["kdf_id"] as? Int, 0x01)
        XCTAssertEqual(v["aead_id"] as? Int, 0x03)
        XCTAssertEqual(v["mode"] as? Int, 0)
        let sk = try P256.KeyAgreement.PrivateKey(rawRepresentation: hexOf(v["skRm"]))
        XCTAssertEqual(sk.publicKey.x963Representation, hexOf(v["pkRm"]))
        var recipient = try HPKE.Recipient(privateKey: sk, ciphersuite: suite, info: hexOf(v["info"]), encapsulatedKey: hexOf(v["enc"]))
        let encryptions = try XCTUnwrap(v["encryptions"] as? [[String: Any]])
        XCTAssertFalse(encryptions.isEmpty)
        // In sequence: each open advances the context's nonce.
        for e in encryptions {
            let pt = try recipient.open(hexOf(e["ct"]), authenticating: hexOf(e["aad"]))
            XCTAssertEqual(pt, hexOf(e["pt"]))
        }
    }

    /// info = "ov0/envelope/v2" ‖ vault_id ‖ device_id ‖ enrollment_nonce
    /// (§22.10 binds the envelope to the enrollment). The seal/open below
    /// is CryptoKit against itself — a framing check, not one of the
    /// three XV-HPKE-SE legs (those are §2.12 / Phase E0 evidence).
    func testEnvelopeInfoAndSealOpen() throws {
        let file = try XV.load("xv_enroll")
        let info = try XCTUnwrap(file["envelope_info"] as? [String: Any])
        // The prefix is fixed here, not read from the file (review VER-I6).
        XCTAssertEqual(info["prefix"] as? String, "ov0/envelope/v2")
        let rebuilt = Data("ov0/envelope/v2".utf8) + hexOf(info["vault_id"]) + hexOf(info["device_id"]) + hexOf(info["enrollment_nonce"])
        XCTAssertEqual(rebuilt, hexOf(info["info"]))
        let payload = try XCTUnwrap(file["envelope_payload"] as? [String: Any])
        let recipient = P256.KeyAgreement.PrivateKey()
        var sender = try HPKE.Sender(recipientKey: recipient.publicKey, ciphersuite: suite, info: rebuilt)
        let ct = try sender.seal(hexOf(payload["tlv_v2"]))
        var opener = try HPKE.Recipient(privateKey: recipient, ciphersuite: suite, info: rebuilt, encapsulatedKey: sender.encapsulatedKey)
        XCTAssertEqual(try opener.open(ct), hexOf(payload["tlv_v2"]))
        XCTAssertEqual(sender.encapsulatedKey.count, 65, "uncompressed X9.63 enc")
        var wrongInfo = try HPKE.Recipient(privateKey: recipient, ciphersuite: suite, info: rebuilt.dropLast() + Data([0]), encapsulatedKey: sender.encapsulatedKey)
        XCTAssertThrowsError(try wrongInfo.open(ct), "another enrollment's info cannot open it")
    }
}
