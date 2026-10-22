import AppKit

/// Swaps the Dock icon between AppIcon.icns (light) and AppIconDark.icns (dark)
/// whenever the system appearance changes. Call `AppIconAppearance.shared.start()`
/// once at launch (e.g. in your App init or applicationDidFinishLaunching).
final class AppIconAppearance {
    static let shared = AppIconAppearance()
    private var observation: NSKeyValueObservation?
    private lazy var light = Bundle.main.image(forResource: "AppIcon")
    private lazy var dark = Bundle.main.image(forResource: "AppIconDark")

    func start() {
        apply()
        observation = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
            DispatchQueue.main.async { self?.apply() }
        }
    }

    private func apply() {
        let isDark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        NSApp.applicationIconImage = isDark ? dark : light
    }
}
