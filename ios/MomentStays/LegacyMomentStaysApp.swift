import SwiftUI

// Retained native UI entry point for the existing Swift implementation.
struct LegacyMomentStaysApp: App {
    var body: some Scene {
        WindowGroup { HomeView(engine: ConversionClient(engine: NativeConversionAdapter())).tint(.orange) }
    }
}
