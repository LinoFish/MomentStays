import Foundation
#if os(iOS)
import UIKit
#endif

private func runChecks(input: URL, root: URL) {
    let engine = NativeConversionAdapter()
    func finish(_ text: String) {
        try? text.write(to: root.appendingPathComponent("status.txt"), atomically: true, encoding: .utf8)
        print(text)
        #if os(macOS)
        exit(text == "PASS" ? 0 : 1)
        #endif
    }
    engine.inspect(input: input, cancellation: InspectionCancellation()) { inspection in
        guard case .success(let value) = inspection, value.classification == .motion else {
            finish("FAIL inspection: \(inspection)"); return
        }
        engine.convert(request: ConversionRequest(inputs: [input], outputDirectory: root.appendingPathComponent("apple"), targetVendor: .apple), progress: { _ in }) { result in
            guard case .success(let converted) = result, let item = converted.items.first,
                  item.status == .converted, let pair = item.output else {
                finish("FAIL apple: \(result)"); return
            }
            do { try checkAppleOutput(original: input, output: pair, work: root) }
            catch { finish("FAIL Apple content: \(error)"); return }
            engine.convert(request: ConversionRequest(inputs: [pair], outputDirectory: root.appendingPathComponent("android"), targetVendor: .google), progress: { _ in }) { reverse in
                guard case .success(let value) = reverse, let item = value.items.first, item.status == .converted else {
                    finish("FAIL android: \(reverse)"); return
                }
                guard let output = item.output else { finish("FAIL Android output missing"); return }
                do { try checkAndroidOutput(pairDirectory: pair, output: output, work: root) }
                catch { finish("FAIL Android content: \(error)"); return }
                finish("PASS")
            }
        }
    }
}

#if os(iOS)
@main class SimulatorApp: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        let root = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try? "started".write(to: root.appendingPathComponent("started.txt"), atomically: true, encoding: .utf8)
        window = UIWindow(frame: UIScreen.main.bounds)
        window?.rootViewController = UIViewController()
        window?.makeKeyAndVisible()
        let input = root.appendingPathComponent("input/motion.jpg")
        do {
            try FileManager.default.createDirectory(at: input.deletingLastPathComponent(), withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: Bundle.main.bundleURL.appendingPathComponent("motion.jpg"), to: input)
            runChecks(input: input, root: root)
        } catch { try? "FAIL setup: \(error)".write(to: root.appendingPathComponent("status.txt"), atomically: true, encoding: .utf8) }
        return true
    }
}
#else
@main struct MacHarness {
    static func main() {
        runChecks(input: URL(fileURLWithPath: CommandLine.arguments[1]), root: URL(fileURLWithPath: CommandLine.arguments[2]))
        dispatchMain()
    }
}
#endif
