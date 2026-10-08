// The CryptoKit-only cross-check (spec v0.5 §22.2): an independent Swift
// reading of the committed XV vectors. It links neither the engine nor
// the app — only Foundation, CryptoKit and Security — so the Rust engine
// is checked against a second implementation, not against itself.

import CryptoKit
import Foundation
import Security
import XCTest

enum XV {
    /// One committed vector file, bundled as a test resource.
    static func load(_ stem: String) throws -> [String: Any] {
        let bundle = Bundle(for: Marker.self)
        let url = try XCTUnwrap(bundle.url(forResource: stem, withExtension: "json"), "\(stem).json is bundled")
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    }

    private final class Marker {}
}

extension Data {
    init(hex: String) {
        precondition(hex.count % 2 == 0, "even hex")
        var out = Data(capacity: hex.count / 2)
        var i = hex.startIndex
        while i < hex.endIndex {
            let j = hex.index(i, offsetBy: 2)
            out.append(UInt8(hex[i..<j], radix: 16)!)
            i = j
        }
        self = out
    }

    var hex: String { map { String(format: "%02x", $0) }.joined() }
    var sha256: Data { Data(SHA256.hash(data: self)) }

    static func + (l: Data, r: String) -> Data { l + Data(r.utf8) }
}

func hexOf(_ any: Any?) -> Data { Data(hex: any as! String) }

func sha(_ parts: Data...) -> Data {
    var h = SHA256()
    parts.forEach { h.update(data: $0) }
    return Data(h.finalize())
}

/// §4.2 TLV, read strictly and independently of `tlv.rs`: Field = tag u8
/// ‖ len u32be ‖ value; Entry = Field* ‖ 0xFF with tags strictly ascending
/// and never 0x00 or 0xFF; Document = 0x00 ‖ len u32be ‖ Entry*.
enum TLV {
    struct Bad: Error {}

    typealias Entry = [(tag: UInt8, value: Data)]

    /// One entry from the front of `bytes`; returns it and what follows.
    static func entryPrefix(_ bytes: Data) throws -> (Entry, Data) {
        var b = Data(bytes)
        var fields: Entry = []
        while true {
            guard let tag = b.first else { throw Bad() }
            b = b.dropFirst()
            if tag == 0xFF { return (fields, Data(b)) }
            guard tag != 0x00, b.count >= 4 else { throw Bad() }
            let len = b.prefix(4).reduce(0) { $0 << 8 | Int($1) }
            b = b.dropFirst(4)
            guard b.count >= len else { throw Bad() }
            if let last = fields.last, last.tag >= tag { throw Bad() }
            fields.append((tag, Data(b.prefix(len))))
            b = b.dropFirst(len)
        }
    }

    static func entry(_ bytes: Data) throws -> Entry {
        let (e, rest) = try entryPrefix(bytes)
        guard rest.isEmpty else { throw Bad() }
        return e
    }

    static func document(_ bytes: Data) throws -> [Entry] {
        let b = Data(bytes)
        guard b.first == 0x00, b.count >= 5 else { throw Bad() }
        let len = b.dropFirst().prefix(4).reduce(0) { $0 << 8 | Int($1) }
        var rest = Data(b.dropFirst(5))
        guard rest.count == len else { throw Bad() }
        var out: [Entry] = []
        while !rest.isEmpty {
            let (e, r) = try entryPrefix(rest)
            out.append(e)
            rest = r
        }
        return out
    }

    static func encode(_ e: Entry) -> Data {
        var out = Data()
        for f in e {
            out.append(f.tag)
            withUnsafeBytes(of: UInt32(f.value.count).bigEndian) { out.append(contentsOf: $0) }
            out.append(f.value)
        }
        out.append(0xFF)
        return out
    }

    /// Minimal big-endian; zero is the single byte 0x00.
    static func uint(_ v: Data) throws -> UInt64 {
        guard !v.isEmpty, v.count <= 8, v.count == 1 || v.first != 0 else { throw Bad() }
        return v.reduce(0) { $0 << 8 | UInt64($1) }
    }

    static func uintBytes(_ v: UInt64) -> Data {
        if v == 0 { return Data([0]) }
        var d = withUnsafeBytes(of: v.bigEndian) { Data($0) }
        while d.first == 0 { d = d.dropFirst() }
        return Data(d)
    }
}

extension Array where Element == (tag: UInt8, value: Data) {
    func value(_ t: UInt8) -> Data? { first { $0.tag == t }?.value }
    var tags: [UInt8] { map(\.tag) }
}

/// ECDSA P-256 over a 32-byte prehash (Security, the X9.62 digest
/// algorithm), plus the low-S rule the engine applies.
enum ECDSA {
    static let halfOrder = Data(hex: "7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8")

    static func publicKey(scalar: Data) throws -> Data {
        try P256.Signing.PrivateKey(rawRepresentation: scalar).publicKey.x963Representation
    }

    static func isLowS(_ sig: Data) -> Bool {
        let s = sig.suffix(32)
        return s.lexicographicallyPrecedes(halfOrder) || s == halfOrder
    }

    /// Verifies r‖s over `prehash` under an uncompressed public key.
    static func verify(prehash: Data, signature: Data, publicKey: Data) -> Bool {
        guard signature.count == 64, prehash.count == 32,
              let sig = try? P256.Signing.ECDSASignature(rawRepresentation: signature) else { return false }
        let attrs: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeyClass: kSecAttrKeyClassPublic]
        guard let key = SecKeyCreateWithData(publicKey as CFData, attrs as CFDictionary, nil) else { return false }
        return SecKeyVerifySignature(key, .ecdsaSignatureDigestX962SHA256, prehash as CFData, sig.derRepresentation as CFData, nil)
    }
}
