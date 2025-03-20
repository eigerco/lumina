//
//  ContentView.swift
//  LuminaDemo
//
//  Created by zwolin on 14/02/2025.
//

import SwiftUI

@MainActor
class LuminaViewModel: ObservableObject {
    @Published var error: Error?
    @Published var isRunning: Bool = false
    @Published var network: Network = .mocha
    @Published var events: [NodeEvent] = []
    @Published var connectedPeers: UInt64 = 0
    @Published var trustedPeers: UInt64 = 0
    @Published var syncProgress: Double = 0.0

    private var node: LuminaNode!
    private var eventsTask: Task<Void, Error>?
    private var statsTimer: Timer?

    private let approxHeadersToSync: UInt64 = 30 * 24 * 60 * 5  // 30 days * 24 hours * 60 mins * 5 blocks

    nonisolated init() {
        Task { @MainActor in
            await initializeNode()
        }
    }

    deinit {
        eventsTask?.cancel()
        statsTimer?.invalidate()
    }

    private func initializeNode() async {
        do {
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
            node = try LuminaNode(config: config)
        } catch {
            self.error = error
        }
    }

    func startNode() async {
        do {
            let _ = try await node.start()

            isRunning = await node.isRunning()
            eventsTask = pollEvents()
            statsTimer = pollStats()
        } catch {
            self.error = error
        }
    }

    func stopNode() async {
        do {
            eventsTask?.cancel()
            statsTimer?.invalidate()
            statsTimer = nil

            isRunning = false
            events.removeAll(keepingCapacity: true)
            connectedPeers = 0
            trustedPeers = 0
            syncProgress = 0.0

            try await node.stop()
        } catch {
            self.error = error
        }
    }

    private func updateStats() async {
        do {
            let peerInfo = try await node.peerTrackerInfo()
            connectedPeers = peerInfo.numConnectedPeers
            trustedPeers = peerInfo.numConnectedTrustedPeers

            let syncInfo = try await node.syncerInfo()
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

    private func pollEvents() -> Task<Void, Error> {
        return Task { [weak self] in
            while true {
                guard !Task.isCancelled else { return }
                if let event = try await self?.node?.nextEvent() {
                    self?.handleEvent(event)
                }
            }
        }
    }

    private func handleEvent(_ event: NodeEvent) {
        switch event {
        // skip too noisy events
        case .shareSamplingResult(_, _, _, _, _):
            return
        default:
            print("Event received: \(event)")
        }
        self.events.append(event)
    }

    func changeNetwork(_ network: Network) async {
        self.network = network
        await initializeNode()
    }

    func refreshRunningState() async {
        isRunning = await node?.isRunning() ?? false
    }
}

struct ContentView: View {
    @StateObject private var viewModel = LuminaViewModel()
    @State private var showingNetworkSelection = false

    var body: some View {
        VStack(spacing: 20) {
            Text("Hello Lumina!")
                .font(.title)

            if let error = viewModel.error {
                Text("Error: \(error.localizedDescription)")
                    .foregroundColor(.red)
                    .padding()
                    .background(Color(.systemGray6))
                    .cornerRadius(10)
            }

            if !viewModel.isRunning {
                Button("Start Node") {
                    showingNetworkSelection = true
                }
                .buttonStyle(.borderedProminent)
                .sheet(isPresented: $showingNetworkSelection) {
                    NetworkSelectionView(
                        viewModel: viewModel,
                        isPresented: $showingNetworkSelection)
                }
            } else {
                VStack(spacing: 15) {
                    StatusCard(
                        isRunning: viewModel.isRunning,
                        connectedPeers: viewModel.connectedPeers,
                        trustedPeers: viewModel.trustedPeers,
                        syncProgress: viewModel.syncProgress
                    )

                    EventsView(events: viewModel.events)

                    HStack(spacing: 20) {
                        Button("Stop") {
                            Task {
                                await viewModel.stopNode()
                            }
                        }
                        .buttonStyle(.bordered)

                        Button("Restart") {
                            Task {
                                await viewModel.stopNode()
                                await viewModel.startNode()
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
    let isRunning: Bool
    let connectedPeers: UInt64
    let trustedPeers: UInt64
    let syncProgress: Double

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Text("Node Status")
                    .font(.headline)
                Spacer()
                Text(isRunning ? "Running" : "Stopped")
                    .foregroundColor(isRunning ? .green : .red)
                    .fontWeight(.medium)
            }

            Divider()

            if isRunning {
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
            }
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(10)
    }
}

struct NetworkSelectionView: View {
    @ObservedObject var viewModel: LuminaViewModel
    @Binding var isPresented: Bool

    var body: some View {
        NavigationView {
            List {
                ForEach(
                    [
                        Network.mainnet, .arabica, .mocha,
                        .custom(NetworkId(id: "private")),
                    ], id: \.self
                ) { network in
                    Button {
                        Task {
                            await viewModel.changeNetwork(network)
                            await viewModel.startNode()
                            isPresented = false
                        }
                    } label: {
                        HStack {
                            Text(network.description)
                            Spacer()
                            if viewModel.network == network {
                                Image(systemName: "checkmark")
                            }
                        }
                    }
                }
            }
            .navigationTitle("Select Network")
            .navigationBarItems(
                trailing: Button("Cancel") {
                    isPresented = false
                })
        }
    }
}

extension Network: CustomStringConvertible {
    public var description: String {
        switch self {
        case .mainnet: return "Mainnet"
        case .arabica: return "Arabica"
        case .mocha: return "Mocha"
        case .custom(let id): return "Custom: \(id)"
        }
    }
}

struct EventsView: View {
    let events: [NodeEvent]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Node Events")
                .font(.headline)
                .padding(.bottom, 4)

            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading) {
                        ForEach(Array(events.enumerated()), id: \.offset) {
                            _, event in
                            Text(eventDescription(event))
                                .font(.system(.caption, design: .monospaced))
                                .foregroundColor(.secondary)
                                .textSelection(.enabled)
                        }
                    }
                    .onChange(of: events.count) { oldCount, newCount in
                        if !events.isEmpty {
                            withAnimation {
                                proxy.scrollTo(
                                    events.count - 1, anchor: .bottom)
                            }
                        }
                    }
                }
            }
            .frame(maxHeight: 200)
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(10)
    }

    private func eventDescription(_ event: NodeEvent) -> String {
        switch event {
        case .connectingToBootnodes:
            return "🔄 Connecting to bootnodes"
        case .peerConnected(let id, let trusted):
            return "✅ Peer connected: \(id.peerId) (trusted: \(trusted))"
        case .peerDisconnected(let id, let trusted):
            return "❌ Peer disconnected: \(id.peerId) (trusted: \(trusted))"
        case .samplingStarted(let height, let width, _):
            return "📊 Starting sampling at height \(height) (width: \(width))"
        case .samplingFinished(let height, let accepted, let ms):
            return
                "✔️ Sampling finished at \(height) (accepted: \(accepted)) [\(ms)ms]"
        case .fetchingHeadersStarted(let from, let to):
            return "📥 Fetching headers \(from)-\(to)"
        case .fetchingHeadersFinished(let from, let to, let ms):
            return "✅ Headers synced \(from)-\(to) [\(ms)ms]"
        case .fetchingHeadersFailed(let from, let to, let error, _):
            return "❌ Sync failed \(from)-\(to): \(error)"

        default:
            return "Event: \(String(describing: event))"
        }
    }
}

#Preview {
    ContentView()
}
