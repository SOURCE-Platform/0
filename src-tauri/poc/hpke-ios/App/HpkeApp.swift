import CryptoKit
import SwiftUI

/// Phase E0 (c): on-device evidence that Apple CryptoKit HPKE opens with a
/// Secure-Enclave-resident P-256 agreement key at the required suite, plus
/// the official RFC 9180 known-answer test for the same suite on iOS.
/// Synthetic data only.
struct ContentView: View {
    @State private var log = "Tap Run."

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("HPKE / Secure Enclave PoC").font(.headline)
            ScrollView { Text(log).font(.system(.footnote, design: .monospaced)).textSelection(.enabled) }
            HStack {
                Button("Run") { log = run() }.buttonStyle(.borderedProminent)
                Button("Copy") { UIPasteboard.general.string = log }
            }
        }.padding()
    }

    func device() -> String {
        var s = utsname(); uname(&s)
        let model = withUnsafePointer(to: &s.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: 1) { String(cString: $0) }
        }
        return "\(model) · iOS \(UIDevice.current.systemVersion)"
    }

    func run() -> String {
        var out = [device()]
        let suite = HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, kdf: .HKDF_SHA256, aead: .chaChaPoly)
        out.append("suite: DHKEM(P-256,HKDF-SHA256)/HKDF-SHA256/ChaCha20Poly1305")
        out.append("SecureEnclave.isAvailable: \(SecureEnclave.isAvailable)")
        do {
            // (c) CryptoKit Sender → CryptoKit Recipient with an SE key.
            let se = try SecureEnclave.P256.KeyAgreement.PrivateKey()
            let pub = se.publicKey.x963Representation
            out.append("SE public key: \(pub.count) bytes, first byte 0x\(String(pub[0], radix: 16))")
            out.append("SE stored blob: \(se.dataRepresentation.count) bytes (opaque, device-bound)")
            let info = Data("ov0/e0-poc/v1".utf8)
            var sender = try HPKE.Sender(recipientKey: se.publicKey, ciphersuite: suite, info: info)
            let ct = try sender.seal(Data("synthetic device envelope payload (c)".utf8))
            out.append("enc: \(sender.encapsulatedKey.count) bytes; ct: \(ct.count) bytes")
            var recipient = try HPKE.Recipient(privateKey: se, ciphersuite: suite, info: info,
                                               encapsulatedKey: sender.encapsulatedKey)
            let pt = try recipient.open(ct)
            let ok = String(data: pt, encoding: .utf8) == "synthetic device envelope payload (c)"
            out.append("(c) CryptoKit seal → CryptoKit open with SE key: \(ok ? "PASS" : "FAIL")")
            // Tamper check: a flipped ciphertext byte must not open.
            var bad = Data(ct); bad[0] ^= 1
            var r2 = try HPKE.Recipient(privateKey: se, ciphersuite: suite, info: info,
                                        encapsulatedKey: sender.encapsulatedKey)
            let tamper = (try? r2.open(bad)) == nil
            out.append("tampered ciphertext refused: \(tamper ? "PASS" : "FAIL")")
        } catch {
            out.append("(c) FAILED: \(error)")
        }
        out.append(kat(suite: suite))
        return out.joined(separator: "\n")
    }

    /// RFC 9180 known-answer test on iOS (software recipient key: a vector
    /// private key cannot be imported into the Secure Enclave).
    func kat(suite: HPKE.Ciphersuite) -> String {
        guard let url = Bundle.main.url(forResource: "vector", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let v = root["vector"] as? [String: Any],
              let skRm = (v["skRm"] as? String).flatMap(hex),
              let enc = (v["enc"] as? String).flatMap(hex),
              let info = (v["info"] as? String).flatMap(hex),
              let encs = v["encryptions"] as? [[String: String]], let first = encs.first,
              let ct = first["ct"].flatMap(hex), let aad = first["aad"].flatMap(hex),
              let want = first["pt"].flatMap(hex) else { return "KAT: vector unreadable" }
        do {
            let key = try P256.KeyAgreement.PrivateKey(rawRepresentation: skRm)
            var r = try HPKE.Recipient(privateKey: key, ciphersuite: suite, info: info, encapsulatedKey: enc)
            let pt = try r.open(ct, authenticating: aad)
            return "RFC 9180 vector (CFRG 5f503c5) opens on iOS: \(pt == want ? "PASS" : "FAIL")"
        } catch {
            return "RFC 9180 vector FAILED on iOS: \(error)"
        }
    }

    func hex(_ s: String) -> Data? {
        var out = Data(); var idx = s.startIndex
        while idx < s.endIndex, let next = s.index(idx, offsetBy: 2, limitedBy: s.endIndex) {
            guard let b = UInt8(s[idx..<next], radix: 16) else { return nil }
            out.append(b); idx = next
        }
        return out
    }
}

@main
struct HpkePoCApp: App {
    var body: some Scene { WindowGroup { ContentView() } }
}
