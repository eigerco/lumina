//
//  LuminaDemoTests.swift
//  LuminaDemoTests
//
//  Created by zwolin on 14/02/2025.
//

let CI_GRPC_URL = "http://localhost:19090"
let NODE_0_ADDR = "celestia1t52q7uqgnjfzdh3wx5m5phvma3umrq8k6tq2p9"

import Testing
@testable import LuminaDemo

struct GrpcClientTest {
    
    @Test func getMinGasPrice() async throws {
        let client = try await GrpcClient(url: CI_GRPC_URL)
        let price = try await client.getMinGasPrice()
        assert(price > 0)
    }


    @Test func authParams() async throws {
        let client = try await GrpcClient(url: CI_GRPC_URL)
        let _ = try await client.getAuthParams()
    }
    
    @Test func getAccount() async throws {
        let client = try await GrpcClient(url: CI_GRPC_URL)
        let account = try await client.getAccount(account: NODE_0_ADDR)

        switch account {
        case .base(let baseAccount):
            let addressStr = try AddressObject.create(address: baseAccount.address).asString()
            
            assert(addressStr == NODE_0_ADDR)
        default :
            Issue.record("failed")
        }
    }
    
    @Test func getBalance() async throws {
        let client = try await GrpcClient(url: CI_GRPC_URL)
        let balance = try await client.getBalance(address: NODE_0_ADDR, denom: "utia")
        assert(balance.amount > 0)
        
        let allBalances = try await client.getAllBalances(address: NODE_0_ADDR)
        assert(allBalances.isEmpty == false)
        for balance in allBalances {
            assert(balance.amount > 0)
            assert(balance.denom != "")
        }
        
        let allSpendable = try await client.getSpendableBalances(address: NODE_0_ADDR)
        assert(allSpendable.isEmpty == false)
        for spendable in allSpendable {
            assert(spendable.amount > 0)
            assert(spendable.denom != "")
        }
        
        let totalSupply = try await client.getTotalSupply()
        assert(totalSupply.isEmpty == false)
        for supply in totalSupply {
            assert(supply.amount > 0)
            assert(supply.denom != "")
        }
    }
    
    @Test func getBlock() async throws {
        let client = try await GrpcClient(url: CI_GRPC_URL)
        
        let latestBlock = try await client.getLatestBlock()
        let height = Int64(latestBlock.header.height.value)
        let block = try await client.getBlockByHeight(height: height)
        
        assert(latestBlock.header.dataHash == block.header.dataHash)
    }
    
    @Test func getBlobParams() async throws {
        let client = try await GrpcClient(url: CI_GRPC_URL)
        
        let params = try await client.getBlobParams()
        assert(params.gasPerBlobByte > 0)
        assert(params.govMaxSquareSize > 0)
    }
}
