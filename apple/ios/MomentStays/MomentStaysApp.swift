import SwiftUI

@main
struct MomentStaysApp: App {
    var body: some Scene {
        WindowGroup {
            MSRootView(engine: Self.engine)
                .task { PhotoSession.removeExpired() }
        }
    }

    private static var engine: any ConversionEngine {
        #if MOTIONLIVE_NATIVE_ENGINE
        NativeConversionAdapter()
        #else
        UnavailableEngine()
        #endif
    }
}
