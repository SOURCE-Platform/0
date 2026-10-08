// The peer wire (annex revision 3, A.1–A.3) read independently of the
// engine: strict envelope and body checks, the `OV0OBJ02` identity
// fields, the canonical Kahn order, and the heads digest.

import Foundation

enum PeerWire {
    struct Bad: Error {}

    static func need(_ ok: Bool) throws { if !ok { throw Bad() } }

    /// Every field present, ascending, non-empty, of the stated width
    /// (0 = a minimal integer).
    static func check(_ e: TLV.Entry, _ widths: [UInt8: Int]) throws {
        try need(e.tags == widths.keys.sorted())
        for f in e {
            let w = widths[f.tag]!
            if w == 0 { _ = try TLV.uint(f.value) } else { try need(f.value.count == w) }
        }
    }

    static func request(_ b: Data) throws -> TLV.Entry {
        let e = try TLV.entry(b)
        try check(e, [1: 0, 2: 16, 3: 16, 4: 16, 5: 0, 6: 32, 7: 0, 8: 16])
        try need(TLV.uint(e.value(1)!) == 1 && TLV.uint(e.value(5)!) <= 0xFFFF)
        return e
    }

    static func response(_ b: Data) throws -> TLV.Entry {
        let e = try TLV.entry(b)
        try check(e, [1: 0, 2: 16, 3: 16, 4: 16, 5: 32, 6: 0, 7: 32, 8: 0])
        try need(TLV.uint(e.value(1)!) == 1 && TLV.uint(e.value(6)!) <= 4)
        return e
    }

    static let emptyBody = Data([0x00, 0, 0, 0, 1, 0xFF])

    static func empty(_ b: Data) throws { try need(b == emptyBody) }

    static func hello(_ b: Data) throws {
        let d = try TLV.document(b)
        try need(d.count == 1)
        try check(d[0], [1: 0, 2: 32, 3: 0, 4: 32, 5: 8192])
    }

    static func headsRequest(_ b: Data) throws {
        let d = try TLV.document(b)
        try need(d.count == 1)
        try check(d[0], [1: d[0].value(1)?.count ?? -1])
        let buckets = [UInt8](d[0].value(1)!)
        try need(!buckets.isEmpty && zip(buckets, buckets.dropFirst()).allSatisfy { $0 < $1 })
    }

    static func ids(_ v: Data, max: Int) throws -> [Data] {
        try need(!v.isEmpty && v.count % 32 == 0 && v.count / 32 <= max)
        let ids = stride(from: 0, to: v.count, by: 32).map { Data(v.dropFirst($0).prefix(32)) }
        try need(zip(ids, ids.dropFirst()).allSatisfy { $0.lexicographicallyPrecedes($1) })
        return ids
    }

    static func revsGetRequest(_ b: Data) throws {
        let d = try TLV.document(b)
        try need(d[0].isEmpty && d.count <= 513)
        var last: Data?
        for e in d.dropFirst() {
            try need(e.tags == [1] || e.tags == [1, 2])
            let rid = e.value(1)!
            try need(rid.count == 16)
            if let h = e.value(2) { _ = try ids(h, max: 64) }
            if let l = last { try need(l.lexicographicallyPrecedes(rid)) }
            last = rid
        }
    }

    static func stateRequest(_ b: Data) throws {
        let d = try TLV.document(b)
        if d[0].tags == [1] {
            try need(d.count == 1)
            _ = try TLV.uint(d[0].value(1)!)
            return
        }
        try check(d[0], [2: 32])
        try need((2...65).contains(d.count))
        var last: Data?
        for e in d.dropFirst() {
            try check(e, [1: 32, 2: 0]) // the offset is always present
            if let l = last { try need(l.lexicographicallyPrecedes(e.value(1)!)) }
            last = e.value(1)!
        }
    }

    static func putCounts(_ b: Data) throws {
        let d = try TLV.document(b)
        try need(d.count == 1)
        try check(d[0], [1: 0, 2: 0, 3: 0])
    }

    static func status(_ b: Data) throws {
        let d = try TLV.document(b)
        try need(d.count == 1)
        try check(d[0], [1: 16, 2: d[0].value(2)?.count ?? -1, 3: 0, 4: 0, 5: 32])
        try need(!d[0].value(2)!.isEmpty)
    }

    struct Revision { let record: Data, id: Data, parents: [Data] }

    /// The identity fields of an `OV0OBJ02` object (§3.7), every length
    /// consumed exactly.
    static func object(_ o: Data) throws -> Revision {
        var c = Data(o)
        func take(_ n: Int) throws -> Data {
            try need(c.count >= n)
            defer { c = Data(c.dropFirst(n)) }
            return Data(c.prefix(n))
        }
        func len() throws -> Int { try take(4).reduce(0) { $0 << 8 | Int($1) } }
        try need(try take(8) == Data("OV0OBJ02".utf8))
        _ = try take(1 + 1 + 4 + 8 + 16)
        let record = try take(16), id = try take(32)
        let n = Int(try take(1)[0])
        try need(n <= 8)
        let parents = try (0..<n).map { _ in try take(32) }
        try need(zip(parents, parents.dropFirst()).allSatisfy { $0.lexicographicallyPrecedes($1) })
        _ = try take(24); _ = try take(try len()); _ = try take(24); _ = try take(try len()); _ = try take(20)
        try need(c.isEmpty)
        return Revision(record: record, id: id, parents: parents)
    }

    /// Kahn's algorithm emitting the smallest ready id; parents outside the
    /// set count as satisfied (annex A.3.4).
    static func canonical(_ revs: [(id: Data, parents: [Data])]) -> [Data] {
        let ids = Set(revs.map(\.id))
        var waiting = Dictionary(uniqueKeysWithValues: revs.map { ($0.id, $0.parents.filter(ids.contains).count) })
        var children: [Data: [Data]] = [:]
        for r in revs { for p in r.parents where ids.contains(p) { children[p, default: []].append(r.id) } }
        var ready = waiting.filter { $0.value == 0 }.map(\.key)
        var out: [Data] = []
        while !ready.isEmpty {
            ready.sort { $0.lexicographicallyPrecedes($1) }
            let id = ready.removeFirst()
            out.append(id)
            for c in children[id] ?? [] {
                waiting[c]! -= 1
                if waiting[c] == 0 { ready.append(c) }
            }
        }
        return out
    }

    /// A `peer_revs_get` answer: objects grouped by ascending record, each
    /// record in canonical order, then unavailable entries.
    static func revsBatch(_ b: Data, put: Bool = false) throws -> [Revision] {
        let d = try TLV.document(b)
        if put { try need(d[0].isEmpty) } else { try check(d[0], [1: 0]); try need(TLV.uint(d[0].value(1)!) <= 1) }
        var revs: [Revision] = []
        var unavailable = false
        for e in d.dropFirst() {
            if e.tags == [1], !unavailable {
                revs.append(try object(e.value(1)!))
            } else {
                try need(!put && e.tags == [2, 5] && e.value(2)!.count == 16 && e.value(5)!.count == 1)
                unavailable = true
            }
        }
        var i = 0
        var last: Data?
        while i < revs.count {
            let group = revs[i...].prefix { $0.record == revs[i].record }
            if let l = last { try need(l.lexicographicallyPrecedes(revs[i].record)) }
            try need(group.map(\.id) == canonical(group.map { ($0.id, $0.parents) }))
            last = revs[i].record
            i += group.count
        }
        return revs
    }

    /// §22.8 heads digest: 256 buckets (first byte of SHA-256(record_id));
    /// per record in ascending id: record_id ‖ u16be count ‖ heads ascending.
    static func headsDigest(_ records: [(Data, [Data])]) -> Data {
        var buckets = (0..<256).map { _ in Data() }
        for (id, heads) in records.sorted(by: { $0.0.lexicographicallyPrecedes($1.0) }) {
            let b = Int(id.sha256[0])
            buckets[b] += id + Data([UInt8(heads.count >> 8), UInt8(heads.count & 0xFF)])
            for h in heads.sorted(by: { $0.lexicographicallyPrecedes($1) }) { buckets[b] += h }
        }
        return buckets.reduce(Data()) { $0 + $1.sha256 }
    }
}
