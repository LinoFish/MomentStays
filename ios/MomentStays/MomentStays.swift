import SwiftUI
import Flutter

@main
struct MomentStaysApp: App {
    @StateObject private var flutter = FlutterHost()

    var body: some Scene {
        WindowGroup { FlutterScreen(engine: flutter.engine).ignoresSafeArea() }
    }
}

@MainActor
final class FlutterHost: ObservableObject {
    let engine = FlutterEngine(name: "momentstays")

    init() {
        engine.run()
        FlutterNativeBridge.attach(to: engine.binaryMessenger)
    }
}

struct FlutterScreen: UIViewControllerRepresentable {
    let engine: FlutterEngine
    func makeUIViewController(context: Context) -> FlutterViewController {
        FlutterViewController(engine: engine, nibName: nil, bundle: nil)
    }
    func updateUIViewController(_ controller: FlutterViewController, context: Context) {}
}
