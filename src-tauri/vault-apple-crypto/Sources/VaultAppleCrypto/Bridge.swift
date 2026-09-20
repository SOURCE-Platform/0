// §2.12 Path A bridge — Apple CryptoKit HPKE against a Secure-Enclave
// resident P-256 key agreement key, over a C ABI.
//
// Suite (fixed, RFC 9180): DHKEM(P-256, HKDF-SHA256) / HKDF-SHA256 /
// ChaCha20-Poly1305 = KEM 0x0010, KDF 0x0001, AEAD 0x0003.
// Public keys are the 65-byte uncompressed X9.63 form (0x04 ‖ X ‖ Y).
//
// The bridge holds no policy and no key material: the SE private key
// never leaves the Enclave (only its opaque, device-bound blob is stored
// in the keychain, and CryptoKit performs the decapsulation DH inside
// the Enclave). Errors are codes; nothing secret is logged.

import CryptoKit
import Foundation
import Security

private let OK: Int32 = 0
private let ERR_ARG: Int32 = -1
private let ERR_KEYCHAIN: Int32 = -2
private let ERR_SE: Int32 = -3
private let ERR_CRYPTO: Int32 = -4
private let ERR_BUFFER: Int32 = -5

private let suite = HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, kdf: .HKDF_SHA256, aead: .chaChaPoly)

private func data(_ p: UnsafePointer<UInt8>?, _ n: Int) -> Data? {
    guard n >= 0 else { return nil }
    guard n > 0 else { return Data() }
    guard let p else { return nil }
    return Data(bytes: p, count: n)
}

private func emit(_ src: Data, _ out: UnsafeMutablePointer<UInt8>?, _ cap: Int, _ len: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let out, let len else { return ERR_ARG }
    guard src.count <= cap else { return ERR_BUFFER }
    src.copyBytes(to: out, count: src.count)
    len.pointee = src.count
    return OK
}

// MARK: - Secure Enclave key storage (opaque blob, keyed by tag)

private func query(_ tag: String) -> [String: Any] {
    [kSecClass as String: kSecClassGenericPassword,
     kSecAttrService as String: "com.racker.zero.vault.se-agreement",
     kSecAttrAccount as String: tag]
}

private func loadKey(_ tag: String) -> SecureEnclave.P256.KeyAgreement.PrivateKey? {
    var q = query(tag)
    q[kSecReturnData as String] = true
    var item: CFTypeRef?
    guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess, let blob = item as? Data else { return nil }
    return try? SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: blob)
}

/// Create an SE-resident P-256 key agreement key, store its opaque blob
/// under `tag`, and return the 65-byte public key.
@_cdecl("ov0_se_key_create")
public func ov0_se_key_create(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag, SecureEnclave.isAvailable else { return ERR_SE }
    let name = String(cString: tag)
    guard let key = try? SecureEnclave.P256.KeyAgreement.PrivateKey() else { return ERR_SE }
    SecItemDelete(query(name) as CFDictionary)
    var add = query(name)
    add[kSecValueData as String] = key.dataRepresentation
    add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    guard SecItemAdd(add as CFDictionary, nil) == errSecSuccess else { return ERR_KEYCHAIN }
    return emit(key.publicKey.x963Representation, out, 65, outLen)
}

@_cdecl("ov0_se_key_public")
public func ov0_se_key_public(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag, let key = loadKey(String(cString: tag)) else { return ERR_SE }
    return emit(key.publicKey.x963Representation, out, 65, outLen)
}

@_cdecl("ov0_se_key_delete")
public func ov0_se_key_delete(_ tag: UnsafePointer<CChar>?) -> Int32 {
    guard let tag else { return ERR_ARG }
    SecItemDelete(query(String(cString: tag)) as CFDictionary)
    return OK
}

/// Evidence helper (PoC only): the stored representation of an SE key.
/// It is an opaque, device-bound blob — not a private scalar — and there
/// is no API on `SecureEnclave.P256.KeyAgreement.PrivateKey` returning
/// raw key bytes (`rawRepresentation` exists only on software keys).
@_cdecl("ov0_se_key_blob")
public func ov0_se_key_blob(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ cap: Int, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag, let key = loadKey(String(cString: tag)) else { return ERR_SE }
    return emit(key.dataRepresentation, out, cap, outLen)
}

// MARK: - HPKE (CryptoKit)

/// CryptoKit `HPKE.Sender` to a 65-byte recipient public key.
@_cdecl("ov0_hpke_seal")
public func ov0_hpke_seal(
    _ pub65: UnsafePointer<UInt8>?, _ pubLen: Int,
    _ info: UnsafePointer<UInt8>?, _ infoLen: Int,
    _ pt: UnsafePointer<UInt8>?, _ ptLen: Int,
    _ aad: UnsafePointer<UInt8>?, _ aadLen: Int,
    _ outEnc: UnsafeMutablePointer<UInt8>?, _ outEncLen: UnsafeMutablePointer<Int>?,
    _ outCt: UnsafeMutablePointer<UInt8>?, _ ctCap: Int, _ outCtLen: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let pubData = data(pub65, pubLen), pubData.count == 65,
          let infoData = data(info, infoLen), let ptData = data(pt, ptLen),
          let aadData = data(aad, aadLen) else { return ERR_ARG }
    guard let recipient = try? P256.KeyAgreement.PublicKey(x963Representation: pubData) else { return ERR_ARG }
    guard var sender = try? HPKE.Sender(recipientKey: recipient, ciphersuite: suite, info: infoData),
          let ct = try? sender.seal(ptData, authenticating: aadData) else { return ERR_CRYPTO }
    let rc = emit(sender.encapsulatedKey, outEnc, 65, outEncLen)
    return rc == OK ? emit(ct, outCt, ctCap, outCtLen) : rc
}

/// CryptoKit `HPKE.Recipient` whose private key is the SE key at `tag`:
/// the decapsulation DH runs inside the Secure Enclave.
@_cdecl("ov0_hpke_open_se")
public func ov0_hpke_open_se(
    _ tag: UnsafePointer<CChar>?,
    _ info: UnsafePointer<UInt8>?, _ infoLen: Int,
    _ enc: UnsafePointer<UInt8>?, _ encLen: Int,
    _ ct: UnsafePointer<UInt8>?, _ ctLen: Int,
    _ aad: UnsafePointer<UInt8>?, _ aadLen: Int,
    _ outPt: UnsafeMutablePointer<UInt8>?, _ ptCap: Int, _ outPtLen: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let tag, let infoData = data(info, infoLen), let aadData = data(aad, aadLen),
          let encData = data(enc, encLen), let ctData = data(ct, ctLen) else { return ERR_ARG }
    guard let key = loadKey(String(cString: tag)) else { return ERR_SE }
    guard var recipient = try? HPKE.Recipient(privateKey: key, ciphersuite: suite, info: infoData, encapsulatedKey: encData),
          let pt = try? recipient.open(ctData, authenticating: aadData) else { return ERR_CRYPTO }
    return emit(pt, outPt, ptCap, outPtLen)
}

/// Software-key variant, used only for known-answer tests against the
/// published RFC 9180 vectors (no SE key can be imported from a vector).
@_cdecl("ov0_hpke_open_sw")
public func ov0_hpke_open_sw(
    _ sk32: UnsafePointer<UInt8>?, _ skLen: Int,
    _ info: UnsafePointer<UInt8>?, _ infoLen: Int,
    _ enc: UnsafePointer<UInt8>?, _ encLen: Int,
    _ ct: UnsafePointer<UInt8>?, _ ctLen: Int,
    _ aad: UnsafePointer<UInt8>?, _ aadLen: Int,
    _ outPt: UnsafeMutablePointer<UInt8>?, _ ptCap: Int, _ outPtLen: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let skData = data(sk32, skLen), let infoData = data(info, infoLen), let aadData = data(aad, aadLen),
          let encData = data(enc, encLen), let ctData = data(ct, ctLen) else { return ERR_ARG }
    guard let key = try? P256.KeyAgreement.PrivateKey(rawRepresentation: skData) else { return ERR_ARG }
    guard var recipient = try? HPKE.Recipient(privateKey: key, ciphersuite: suite, info: infoData, encapsulatedKey: encData),
          let pt = try? recipient.open(ctData, authenticating: aadData) else { return ERR_CRYPTO }
    return emit(pt, outPt, ptCap, outPtLen)
}
