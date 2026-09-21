// PoC-only surfaces, kept out of the production bridge (§2.12):
// software-key HPKE open (RFC 9180 known-answer tests) and the SE blob
// reader (non-exportability evidence). Not shipped, not linked by the
// helper; compiled only by this PoC's build script.
import CryptoKit
import Foundation
import Security

private let suite = HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, kdf: .HKDF_SHA256, aead: .chaChaPoly)

private func data(_ p: UnsafePointer<UInt8>?, _ n: Int) -> Data? {
    guard n >= 0 else { return nil }
    guard n > 0 else { return Data() }
    guard let p else { return nil }
    return Data(bytes: p, count: n)
}

private func emit(_ src: Data, _ out: UnsafeMutablePointer<UInt8>?, _ cap: Int, _ len: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let out, let len, src.count <= cap else { return -5 }
    src.copyBytes(to: out, count: src.count)
    len.pointee = src.count
    return 0
}

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
          let encData = data(enc, encLen), let ctData = data(ct, ctLen),
          let key = try? P256.KeyAgreement.PrivateKey(rawRepresentation: skData) else { return -1 }
    guard var r = try? HPKE.Recipient(privateKey: key, ciphersuite: suite, info: infoData, encapsulatedKey: encData),
          let pt = try? r.open(ctData, authenticating: aadData) else { return -4 }
    return emit(pt, outPt, ptCap, outPtLen)
}

@_cdecl("ov0_se_key_blob")
public func ov0_se_key_blob(_ tag: UnsafePointer<CChar>?, _ out: UnsafeMutablePointer<UInt8>?, _ cap: Int, _ outLen: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let tag else { return -1 }
    var q: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                            kSecAttrService as String: "com.racker.zero.vault.se-agreement",
                            kSecAttrAccount as String: String(cString: tag),
                            kSecReturnData as String: true]
    var item: CFTypeRef?
    guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess, let blob = item as? Data else { return -3 }
    q.removeAll()
    return emit(blob, out, cap, outLen)
}
