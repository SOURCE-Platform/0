// The app's engine wrapper against the real vault-ffi (simulator): a fresh
// directory reports "uninitialized", Mac-only ops are refused, and a lock
// on an empty vault is harmless. Synthetic data only.

import XCTest
@testable import SourceVault

final class EngineTests: XCTestCase {
    func testAFreshEngineIsUninitializedAndRefusesMacOps() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let engine = try XCTUnwrap(VaultEngine(dir: dir, services: VaultServices()))
        let state = expectation(description: "get_state")
        engine.call(["op": "get_state"]) { answer in
            XCTAssertEqual(answer["state"] as? String, "uninitialized")
            state.fulfill()
        }
        let refused = expectation(description: "setup_vault")
        engine.call(["op": "setup_vault"]) { answer in
            XCTAssertEqual(answer["error"] as? String, "UNKNOWN_OP")
            refused.fulfill()
        }
        wait(for: [state, refused], timeout: 10)
        engine.lock()
    }
}
