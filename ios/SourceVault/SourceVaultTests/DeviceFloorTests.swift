// §19 row 30: the A15 floor, by model identifier.

import XCTest
@testable import SourceVault

final class DeviceFloorTests: XCTestCase {
    func testA15AndNewerOnly() {
        for ok in ["iPhone14,4", "iPhone14,6", "iPhone14,7", "iPhone15,2", "iPhone17,1", "iPhone18,3"] {
            XCTAssertTrue(DeviceFloor.supported(ok), ok)
        }
        for refused in ["iPhone13,1", "iPhone12,8", "iPhone11,8", "iPad14,1", "arm64", "MacBookPro18,3", "iPhone", "iPhonex,1", ""] {
            XCTAssertFalse(DeviceFloor.supported(refused), refused)
        }
    }
}
