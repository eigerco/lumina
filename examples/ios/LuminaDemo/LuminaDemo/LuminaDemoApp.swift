//
//  LuminaDemoApp.swift
//  LuminaDemo
//
//  Created by zwolin on 14/02/2025.
//

import SwiftUI

@main
struct LuminaDemoApp: App {
    var body: some Scene {
        WindowGroup {
            TabView{
                Tab("Node", systemImage: "l.circle") {
                    LuminaNodeView()
                }
                Tab("gRPC", systemImage: "phone.connection") {
                    GrpcView()
                }
            }
        }
    }
}
