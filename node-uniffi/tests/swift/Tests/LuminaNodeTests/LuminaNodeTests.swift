import XCTest
@testable import LuminaNode

final class LuminaNodeTests: XCTestCase {
    func testRequestHeader() async throws {
        let config = NodeConfig(basePath: nil, network: .mainnet, bootnodes: nil,
                                syncingWindowSecs: nil, pruningDelaySecs: nil,
                                batchSize: nil, ed25519SecretKeyBytes: nil)
        let node = try LuminaNode(config: config)

        try await node.start()
        try await node.waitConnectedTrusted()

        let headStr = try await node.requestHeadHeader()
        let headData = headStr.data(using: .utf8)!;
        let head = try JSONSerialization.jsonObject(with: headData) as! [String: Any]
        let headHeader = head["header"] as! [String: Any]
        let headHeight = UInt64(headHeader["height"] as! String)!

        let prevStr = try await node.requestHeaderByHeight(height: headHeight - 1)
        let prevData = prevStr.data(using: .utf8)!;
        let prev = try JSONSerialization.jsonObject(with: prevData) as! [String: Any]
        let prevHash = ((prev["commit"] as! [String: Any])["block_id"] as! [String: Any])["hash"] as! String

        let expectedPrevHash = (headHeader["last_block_id"] as! [String: Any])["hash"] as! String
        XCTAssertEqual(prevHash, expectedPrevHash)
    }
}
