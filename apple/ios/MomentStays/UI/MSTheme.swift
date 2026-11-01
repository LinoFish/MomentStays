import SwiftUI
import UIKit

// Colors are defined once here (design-swiftui-addendum §1). Views never use raw hex.
extension Color {
    private static func dynamic(light: UInt32, dark: UInt32) -> Color {
        func ui(_ hex: UInt32) -> UIColor {
            UIColor(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
        }
        return Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? ui(dark) : ui(light) })
    }
    /// Primary button fill, selection circles and progress fill only.
    static let msAccentFill = dynamic(light: 0xF0A43C, dark: 0xF4B860)
    /// Text and checkmarks on amber.
    static let msOnAccent = dynamic(light: 0x1F2933, dark: 0x1F2933)
    /// Global tint: back, toolbar text, links, secondary actions, row icons.
    static let msLink = dynamic(light: 0x2563EB, dark: 0x60A5FA)
    static let msBackground = Color(uiColor: .systemGroupedBackground)
    static let msSurface = Color(uiColor: .secondarySystemGroupedBackground)
}

/// The single amber primary action: 52 pt high, 26 pt corners, dark 17 pt semibold text.
struct MSPrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.semibold))
            .foregroundStyle(isEnabled ? Color.msOnAccent : Color(uiColor: .secondaryLabel))
            .frame(maxWidth: .infinity, minHeight: 52)
            .padding(.horizontal, 16)
            .background(isEnabled ? Color.msAccentFill : Color(uiColor: .tertiarySystemFill), in: RoundedRectangle(cornerRadius: 26, style: .continuous))
            .opacity(configuration.isPressed ? 0.7 : 1)
            .contentShape(RoundedRectangle(cornerRadius: 26, style: .continuous))
    }
}

/// Bottom action area placed with `.safeAreaInset(edge: .bottom)` so the last row can scroll clear of it.
struct MSBottomBar<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        // Capped so the pinned bar never covers the list at the largest text sizes.
        VStack(spacing: 8) { content }
            .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity)
            .background(.bar)
            .overlay(alignment: .top) { Divider() }
    }
}

/// Neutral placeholder thumbnail. Never a fake photo.
struct MSPlaceholderThumb: View {
    var size: CGFloat = 44
    var body: some View {
        RoundedRectangle(cornerRadius: 6, style: .continuous)
            .fill(Color(uiColor: .tertiarySystemFill))
            .frame(width: size, height: size)
            .overlay { Image(systemName: "photo").foregroundStyle(.secondary) }
            .accessibilityHidden(true)
    }
}
