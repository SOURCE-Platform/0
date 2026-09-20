// swift-tools-version:5.9
import PackageDescription

/// §2.12 Path A bridge: Apple CryptoKit HPKE + Secure Enclave key access,
/// exposed over a C ABI. No policy, no key material beyond SE references.
let package = Package(
    name: "VaultAppleCrypto",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "VaultAppleCrypto", type: .static, targets: ["VaultAppleCrypto"])
    ],
    targets: [
        .target(name: "VaultAppleCrypto", path: "Sources/VaultAppleCrypto")
    ]
)
