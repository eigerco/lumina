//
//  IdentityRetention.swift
//  LuminaDemo
//
//  Created by mikolaj.florkiewicz on 2025-09-17.
//

import Testing
import Foundation
@testable import LuminaDemo

struct IdentityRetentionTests {
    @Test
    func identityRetention() async throws {
        let tempDir = NSTemporaryDirectory();
        let config = NodeConfig(
            basePath: tempDir,
            network: Network.arabica,
            bootnodes: nil,
            pruningWindowSecs: nil,
            batchSize: nil,
            ed25519SecretKeyBytes: nil
        )
        print(tempDir)

        let node0 = try LuminaNode(config: config);
        let _ = try await node0.start();
        let peerId0 = try await node0.localPeerId();
        let _ = try await node0.stop();
        
        let node1 = try LuminaNode(config: config);
        let _ = try await node1.start();
        let peerId1 = try await node1.localPeerId();
        let _ = try await node1.stop();
        
        assert(peerId0 == peerId1)
        
    }
}
