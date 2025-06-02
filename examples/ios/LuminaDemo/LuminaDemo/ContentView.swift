//
//  ContentView.swift
//  LuminaDemo
//
//  Created by zwolin on 14/02/2025.
//

import SwiftUI
import Logging
import P256K

final class StaticSigner : UniffiSigner {
    let sk : P256K.Signing.PrivateKey
    
    init(sk: P256K.Signing.PrivateKey) {
        self.sk = sk
    }
    
    func sign(doc: SignDoc) async throws -> UniffiSignature {
        let messageData = protoEncodeSignDoc(signDoc: doc);
        let signature = try! sk.signature(for: messageData)
        return try! UniffiSignature (bytes: signature.compactRepresentation)
    }
}

@MainActor
class LuminaViewModel: ObservableObject {
    @Published var error: Error?
    @Published var isStarting: Bool = false
    @Published var isRunning: Bool = false
    @Published var network: Network = .mocha
    @Published var connectedPeers: UInt64 = 0
    @Published var trustedPeers: UInt64 = 0
    @Published var maybeNetworkHeight: UInt64?
    @Published var syncedRanges: [BlockRange] = []
    
    private var node: LuminaNode?
    private var statsTimer: Timer?
    
    deinit {
        statsTimer?.invalidate()
    }
    
    func startNode(_ network: Network) async {
        isStarting = true;
        
        let paths = FileManager.default.urls(
            for: .cachesDirectory, in: .userDomainMask)
        let cacheDir = paths[0].path
        
        let config = NodeConfig(
            basePath: cacheDir,
            network: network,
            bootnodes: nil,
            syncingWindowSecs: nil,
            pruningDelaySecs: nil,
            batchSize: nil,
            ed25519SecretKeyBytes: nil
        )
        
        do {
            let grpcClient = try await GrpcClient(url: "https://rpc-celestia.alphab.ai:9090")
            let params = try await grpcClient.getAuthParams()
            Logger(label: "GrpcTest").info("Got auth params: \(String(describing: params))")
            
            let address = try! parseBech32Address(bech32Address:"celestia1t52q7uqgnjfzdh3wx5m5phvma3umrq8k6tq2p9")
            let sk = try! P256K.Signing.PrivateKey(dataRepresentation: try! "393fdb5def075819de55756b45c9e2c8531a8c78dd6eede483d3440e9457d839".bytes
            )
            
            let pk = sk.publicKey
            let signer = StaticSigner(sk: sk);
            let txclient = try await TxClient(url: "http://192.168.1.11:19090", accountAddress: address, accountPubkey: pk.dataRepresentation, signer: signer)
            
            let data = "Hello, World".data(using: .utf8)!
            let ns = try Namespace(version: 0, id: "foo".data(using: .utf8)!)
            let blob = try Blob(namespace: ns, data: data, appVersion: AppVersion.v1)
            
            let submit = try await txclient.submitBlobs(blobs: [blob], config: nil)
            
            Logger(label: "GrpcTest").info("Submitted: \(submit)")
            
            self.node = try LuminaNode(config: config)
            let _ = try await node!.start()
            isRunning = await node!.isRunning();
            
            Logger(label: "LuminaDemo").info("node spun up: \(isRunning)")
            
            statsTimer = pollStats()
        } catch {
            isStarting = false;
            self.error = error
        }
    }
    
    func stopNode() async {
        statsTimer?.invalidate()
        statsTimer = nil
        
        isStarting = false;
        isRunning = false
        connectedPeers = 0
        trustedPeers = 0
        
        maybeNetworkHeight = nil
        syncedRanges = []
        
        do {
            try await node?.stop()
            node = nil
        } catch {
            self.error = error
        }
    }
    
    private func updateStats() async {
        do {
            if let peerInfo = try await node?.peerTrackerInfo() {
                connectedPeers = peerInfo.numConnectedPeers
                trustedPeers = peerInfo.numConnectedTrustedPeers
            }
            
            if let syncInfo = try await node?.syncerInfo() {
                
                maybeNetworkHeight = syncInfo.subjectiveHead
                syncedRanges = syncInfo.storedHeaders
            }
        } catch {
            self.error = error
        }
    }
    
    private func pollStats() -> Timer {
        return Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) {
            [weak self] _ in
            Task { @MainActor [weak self] in
                await self?.updateStats()
            }
        }
    }
    
    func refreshRunningState() async {
        isRunning = await node?.isRunning() ?? false
    }
}

struct ContentView: View {
    @StateObject private var viewModel = LuminaViewModel()
    
    var body: some View {
        VStack(spacing: 20) {
            
            if let error = viewModel.error {
                Text("Error: \(error.localizedDescription)")
                    .foregroundColor(.red)
                    .padding()
                    .background(Color(.systemGray6))
                    .cornerRadius(10)
            }
            
            if !viewModel.isRunning {
                if viewModel.isStarting {
                    // hide the network selection when waiting for the node to start up
                    Text("Starting...").font(.title);
                    ProgressView()
                } else {
                    Text("Choose network").font(.title)
                    NetworkSelection(viewModel: viewModel)
                }
            } else {
                Text("Hello Lumina!")
                    .font(.title)
                VStack(spacing: 15) {
                    StatusCard(
                        connectedPeers: viewModel.connectedPeers,
                        trustedPeers: viewModel.trustedPeers,
                        maybeNetworkHeight: viewModel.maybeNetworkHeight,
                        network: viewModel.network,
                        syncedRanges: viewModel.syncedRanges
                    )
                    
                    HStack(spacing: 20) {
                        Button("Stop") {
                            Task {
                                await viewModel.stopNode()
                            }
                        }
                        .buttonStyle(.bordered)
                    }
                }
            }
        }
        .padding()
        .task {
            await viewModel.refreshRunningState()
        }
    }
}

struct StatusCard: View {
    let connectedPeers: UInt64
    let trustedPeers: UInt64
    let maybeNetworkHeight: UInt64?
    let network: Network
    let syncedRanges: [BlockRange]
    
    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Text("Connected Peers")
                    .foregroundColor(.secondary)
                Spacer()
                Text("\(connectedPeers)")
                    .fontWeight(.medium)
            }
            
            HStack {
                Text("Trusted Peers")
                    .foregroundColor(.secondary)
                Spacer()
                Text("\(trustedPeers)")
                    .fontWeight(.medium)
            }
            Divider()
            HStack {
                Text("Network Height")
                    .foregroundColor(.secondary)
                Spacer()
                Text(maybeNetworkHeight.map(String.init) ?? "")
                    .fontWeight(.medium)
            }
            HStack {
                Text("Network")
                    .foregroundColor(.secondary)
                Spacer()
                Text("\(network)")
                    .fontWeight(.medium)
            }
            Divider()
            Text("Synced Ranges")
                .font(.subheadline)
            ForEach(syncedRanges) { range in
                HStack{
                    Text("\(range.start)")
                    Spacer()
                    Text("\(range.end)")
                }.padding(.horizontal, 40)
            }
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(10)
    }
}

struct NetworkSelection: View {
    @ObservedObject var viewModel: LuminaViewModel
    
    var body: some View {
        List {
            ForEach(
                [
                    Network.mainnet, .arabica, .mocha,
                    .custom(NetworkId(id: "private")),
                ], id: \.self
            ) { network in
                Button {
                    Task {
                        await viewModel.startNode(network)
                    }
                } label: {
                    HStack {
                        Text(network.description)
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
    }
}

extension Network: CustomStringConvertible {
    public var description: String {
        switch self {
        case .mainnet: return "Mainnet"
        case .arabica: return "Arabica"
        case .mocha: return "Mocha"
        case .custom(let id): return "Custom: \(id.id)"
        }
    }
}

extension BlockRange: Identifiable {
    public var id: Self { self }
}

#Preview {
    ContentView()
}
