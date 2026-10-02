// §2.12 Path A bridge: CryptoKit HPKE (RFC 9180 DHKEM(P-256, HKDF-SHA256) /
// HKDF-SHA256 / ChaCha20-Poly1305) against Secure Enclave P-256 keys, over a
// C ABI; public keys are 65-byte X9.63. No policy and no key material here:
// only opaque, device-bound SE blobs are kept. Errors are codes; no logging.

import CryptoKit
import Foundation
import LocalAuthentication
import Security

private let OK: Int32 = 0
private let ERR_ARG: Int32 = -1
private let ERR_KEYCHAIN: Int32 = -2
private let ERR_SE: Int32 = -3
private let ERR_CRYPTO: Int32 = -4
private let ERR_BUFFER: Int32 = -5
private let ERR_NO_BIOMETRY: Int32 = -6 // no usable biometry: the caller offers the MP
private let ERR_AUTH: Int32 = -7 // the user declined or failed the biometric check

private let suite = HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, kdf: .HKDF_SHA256, aead: .chaChaPoly)
private typealias Out = UnsafeMutablePointer<UInt8>?
private typealias OutLen = UnsafeMutablePointer<Int>?

private func data(_ p: UnsafePointer<UInt8>?, _ n: Int) -> Data? {
    guard n > 0 else { return n == 0 ? Data() : nil }
    return p.map { Data(bytes: $0, count: n) }
}

private func emit(_ src: Data, _ out: Out, _ cap: Int, _ len: OutLen) -> Int32 {
    guard let out, let len, src.count <= cap else { return out == nil || len == nil ? ERR_ARG : ERR_BUFFER }
    src.copyBytes(to: out, count: src.count)
    len.pointee = src.count
    return OK
}

/// SE key blobs by tag, in the login keychain (the data-protection one needs
/// an entitlement the gate/test binaries lack; Phase E report).
private func query(_ tag: String, _ role: String) -> [String: Any] {
    [kSecClass as String: kSecClassGenericPassword,
     kSecAttrService as String: "com.racker.zero.vault.se-\(role)",
     kSecAttrAccount as String: tag]
}

private func store(_ blob: Data, _ tag: String, _ role: String) -> Int32 {
    SecItemDelete(query(tag, role) as CFDictionary)
    var add = query(tag, role)
    add[kSecValueData as String] = blob
    add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    return SecItemAdd(add as CFDictionary, nil) == errSecSuccess ? OK : ERR_KEYCHAIN
}

private func loadBlob(_ tag: String, _ role: String) -> Data? {
    var q = query(tag, role)
    q[kSecReturnData as String] = true
    var item: CFTypeRef?
    guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess else { return nil }
    return item as? Data
}

private func loadAgree(_ tag: String, _ ctx: LAContext? = nil) -> SecureEnclave.P256.KeyAgreement.PrivateKey? {
    loadBlob(tag, "agreement").flatMap { try? SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: $0, authenticationContext: ctx) }
}

private func loadSign(_ tag: String) -> SecureEnclave.P256.Signing.PrivateKey? {
    loadBlob(tag, "signing").flatMap { try? SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: $0) }
}

private func stored(_ blob: Data, _ pub: Data, _ tag: UnsafePointer<CChar>, _ role: String, _ out: Out, _ outLen: OutLen) -> Int32 {
    let rc = store(blob, String(cString: tag), role)
    return rc == OK ? emit(pub, out, 65, outLen) : rc
}

/// An SE agreement key under `tag`. `bio`: the Enclave requires the current
/// Touch ID fingerprints for every use (`.biometryCurrentSet`, owner decision
/// 2026-10-01); a login password never opens it.
private func createAgree(_ tag: UnsafePointer<CChar>?, _ bio: Bool, _ out: Out, _ outLen: OutLen) -> Int32 {
    guard let tag, SecureEnclave.isAvailable else { return ERR_SE }
    let flags: SecAccessControlCreateFlags = bio ? [.privateKeyUsage, .biometryCurrentSet] : [.privateKeyUsage]
    guard let ac = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, flags, nil),
          let key = try? SecureEnclave.P256.KeyAgreement.PrivateKey(compactRepresentable: false, accessControl: ac) else { return ERR_SE }
    return stored(key.dataRepresentation, key.publicKey.x963Representation, tag, "agreement", out, outLen)
}

@_cdecl("ov0_se_key_create")
public func ov0_se_key_create(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    createAgree(tag, false, out, outLen)
}

@_cdecl("ov0_se_key_create_bio")
public func ov0_se_key_create_bio(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    createAgree(tag, true, out, outLen)
}

/// 1 if the Enclave refuses the agreement key for want of the user (biometry-
/// bound), 0 if usable silently, an error otherwise (the LA check then stays).
@_cdecl("ov0_se_key_needs_user")
public func ov0_se_key_needs_user(_ tag: UnsafePointer<CChar>?) -> Int32 {
    let ctx = LAContext()
    ctx.interactionNotAllowed = true
    guard let tag, let key = loadAgree(String(cString: tag), ctx) else { return ERR_SE }
    do { _ = try key.sharedSecretFromKeyAgreement(with: P256.KeyAgreement.PrivateKey().publicKey); return 0 } catch is LAError { return 1 } catch { return ERR_CRYPTO }
}

@_cdecl("ov0_se_key_public")
public func ov0_se_key_public(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag, let key = loadAgree(String(cString: tag)) else { return ERR_SE }
    return emit(key.publicKey.x963Representation, out, 65, outLen)
}

@_cdecl("ov0_se_key_delete")
public func ov0_se_key_delete(_ tag: UnsafePointer<CChar>?) -> Int32 {
    guard let tag else { return ERR_ARG }
    SecItemDelete(query(String(cString: tag), "agreement") as CFDictionary)
    SecItemDelete(query(String(cString: tag), "signing") as CFDictionary)
    return OK
}

@_cdecl("ov0_se_sign_create")
public func ov0_se_sign_create(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag, SecureEnclave.isAvailable, let key = try? SecureEnclave.P256.Signing.PrivateKey() else { return ERR_SE }
    return stored(key.dataRepresentation, key.publicKey.x963Representation, tag, "signing", out, outLen)
}

@_cdecl("ov0_se_sign_public")
public func ov0_se_sign_public(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag, let key = loadSign(String(cString: tag)) else { return ERR_SE }
    return emit(key.publicKey.x963Representation, out, 65, outLen)
}

/// A digest the caller computed (§2.7 prehash): `signature(for: Digest)`
/// signs it as-is, where `signature(for: Data)` would hash it again.
struct PrehashedSHA256: Digest {
    static var byteCount: Int { 32 }
    private let bytes: [UInt8]
    init?(_ d: Data) { guard d.count == 32 else { return nil }; bytes = Array(d) }
    func withUnsafeBytes<R>(_ body: (UnsafeRawBufferPointer) throws -> R) rethrows -> R { try bytes.withUnsafeBytes(body) }
    func makeIterator() -> Array<UInt8>.Iterator { bytes.makeIterator() }
    var description: String { "PrehashedSHA256(32 bytes)" }
}

/// r‖s over a 32-byte digest; the caller normalizes to low-S (§2.7).
@_cdecl("ov0_se_sign_digest")
public func ov0_se_sign_digest(
    _ tag: UnsafePointer<CChar>?, _ digest: UnsafePointer<UInt8>?, _ digestLen: Int,
    _ out: UnsafeMutablePointer<UInt8>?, _ outLen: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let tag, let d = data(digest, digestLen), let digest = PrehashedSHA256(d) else { return ERR_ARG }
    guard let key = loadSign(String(cString: tag)) else { return ERR_SE }
    guard let sig = try? key.signature(for: digest) else { return ERR_CRYPTO }
    return emit(sig.rawRepresentation, out, 64, outLen)
}

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
    guard let pubData = data(pub65, pubLen), pubData.count == 65, let infoData = data(info, infoLen),
          let ptData = data(pt, ptLen), let aadData = data(aad, aadLen),
          let recipient = try? P256.KeyAgreement.PublicKey(x963Representation: pubData) else { return ERR_ARG }
    guard var sender = try? HPKE.Sender(recipientKey: recipient, ciphersuite: suite, info: infoData),
          let ct = try? sender.seal(ptData, authenticating: aadData) else { return ERR_CRYPTO }
    let rc = emit(sender.encapsulatedKey, outEnc, 65, outEncLen)
    return rc == OK ? emit(ct, outCt, ctCap, outCtLen) : rc
}

/// HPKE open with the SE agreement key (DH inside the Enclave); a
/// biometry-bound key asks for Touch ID with `reason`. Declined or failed →
/// `ERR_AUTH`; the Enclave will not use it for any other reason (no sensor,
/// lid closed, none enrolled, locked out) → `ERR_NO_BIOMETRY`.
@_cdecl("ov0_hpke_open_se_auth")
public func ov0_hpke_open_se_auth(
    _ tag: UnsafePointer<CChar>?, _ reason: UnsafePointer<CChar>?,
    _ info: UnsafePointer<UInt8>?, _ infoLen: Int,
    _ enc: UnsafePointer<UInt8>?, _ encLen: Int,
    _ ct: UnsafePointer<UInt8>?, _ ctLen: Int,
    _ outPt: UnsafeMutablePointer<UInt8>?, _ ptCap: Int, _ outPtLen: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let tag, let reason, let infoData = data(info, infoLen),
          let encData = data(enc, encLen), let ctData = data(ct, ctLen) else { return ERR_ARG }
    guard (try? P256.KeyAgreement.PublicKey(x963Representation: encData)) != nil else { return ERR_CRYPTO }
    let ctx = LAContext(); ctx.localizedReason = String(cString: reason)
    guard let key = loadAgree(String(cString: tag), ctx) else { return ERR_SE }
    var recipient: HPKE.Recipient
    do {
        recipient = try HPKE.Recipient(privateKey: key, ciphersuite: suite, info: infoData, encapsulatedKey: encData)
    } catch let e as LAError where e.code == .userCancel || e.code == .authenticationFailed {
        return ERR_AUTH
    } catch {
        return ERR_NO_BIOMETRY
    }
    guard var pt = try? recipient.open(ctData, authenticating: Data()) else { return ERR_CRYPTO }
    defer { pt.resetBytes(in: 0..<pt.count) } // §22.2 (a): the Swift copy of the VK is zeroed
    return emit(pt, outPt, ptCap, outPtLen)
}
