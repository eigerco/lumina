// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "LuminaNode",
    products: [
        .library(
            name: "LuminaNode",
            targets: ["LuminaNode"]),
    ],
    targets: [
        .target(
            name: "LuminaNodeHeaders",
            publicHeadersPath: "."),
        .target(
            name: "LuminaNode",
            dependencies: ["LuminaNodeHeaders"],
            linkerSettings: [
                .linkedLibrary("lumina_node_uniffi"),
                .unsafeFlags(["-L", "./lib"])
            ]),
        .testTarget(
            name: "LuminaNodeTests",
            dependencies: ["LuminaNode"]),
    ]
)
