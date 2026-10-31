import SwiftUI

@main
struct MomentStaysApp: App {
    var body: some Scene {
        WindowGroup {
            HomeView(engine: ConversionClient(engine: NativeConversionAdapter()))
                .tint(.orange)
        }
    }
}
