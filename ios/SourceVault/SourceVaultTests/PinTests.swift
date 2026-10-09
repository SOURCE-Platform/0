// The peer pin (spec v0.5 §22.8): SHA-256 of the Mac key's SPKI DER. The
// app rebuilds the SPKI from the raw P-256 point; CryptoKit's own
// `derRepresentation` is the same SubjectPublicKeyInfo, so the two must
// agree. Other key types are refused. Synthetic keys only.

import CryptoKit
import Security
import XCTest
@testable import SourceVault

final class PinTests: XCTestCase {
    private func secKey(_ x963: Data, bits: Int) throws -> SecKey {
        let attrs: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeyClass: kSecAttrKeyClassPublic, kSecAttrKeySizeInBits: bits]
        return try XCTUnwrap(SecKeyCreateWithData(x963 as CFData, attrs as CFDictionary, nil))
    }

    func testThePinIsTheSPKIHash() throws {
        let key = P256.Signing.PrivateKey().publicKey
        let expected = SHA256.hash(data: key.derRepresentation).map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(PeerClient.spkiSHA256(of: try secKey(key.x963Representation, bits: 256)), expected)
    }

    func testOtherKeyTypesAreRefused() throws {
        let p384 = P384.Signing.PrivateKey().publicKey
        XCTAssertNil(PeerClient.spkiSHA256(of: try secKey(p384.x963Representation, bits: 384)))
    }
}
