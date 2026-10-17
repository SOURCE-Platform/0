// The v1 hardware floor (spec §2.3, §19 row 30; owner decision
// 2026-09-21): A15-class iPhones or newer. The Argon2id tuple
// (m=64 MiB, t=3, p=1) was measured on the slowest supported device, the
// iPhone 13 mini; older hardware was never measured, so the app refuses
// to run there instead of claiming support nobody has tested.
//
// A15 arrived with model identifier family "iPhone14,x" (iPhone 13 series,
// SE 3rd generation, iPhone 14 / 14 Plus); every later family is newer.
// Anything that is not an iPhone identifier is refused (fail closed).

import Foundation

enum DeviceFloor {
    /// "iPhone14,4" on a device; the simulated model in the Simulator.
    static var modelIdentifier: String {
        #if targetEnvironment(simulator)
        // Only the Simulator may name its model this way (final review
        // VER-O11): on a device the variable is never trusted.
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] {
            return simulated
        }
        #endif
        var info = utsname()
        uname(&info)
        return withUnsafeBytes(of: &info.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
    }

    static func supported(_ identifier: String) -> Bool {
        guard identifier.hasPrefix("iPhone") else { return false }
        let family = identifier.dropFirst("iPhone".count).split(separator: ",").first.flatMap { Int($0) }
        return (family ?? 0) >= 14
    }

    static let isSupported = supported(modelIdentifier)
}
