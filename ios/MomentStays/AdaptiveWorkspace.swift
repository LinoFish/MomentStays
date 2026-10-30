import SwiftUI

// The owner of the two panes holds all task and selection state. Only their presentation
// changes as the scene resizes; navigation and long-running work stay outside this view.
struct AdaptiveWorkspace<Controls: View, Content: View>: View {
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    var contentFirst: Bool = true
    @ViewBuilder var controls: Controls
    @ViewBuilder var content: Content
    var body: some View {
        GeometryReader { geometry in
            if geometry.size.width >= 700 && !dynamicTypeSize.isAccessibilitySize {
                if #available(iOS 27.1, *) {
                    ArrangementView {
                        ScrollView { controls.frame(maxWidth: 480).padding(20) }.splitArrangementLayoutRatio(0.35)
                    } secondary: {
                        ScrollView { content.frame(maxWidth: .infinity).padding(20) }
                    }.arrangementViewStyle(.split)
                } else {
                    HStack(alignment: .top, spacing: 24) {
                        ScrollView { controls.frame(maxWidth: 480).padding(20) }.frame(width: min(geometry.size.width * 0.35, 380))
                        ScrollView { content.frame(maxWidth: .infinity).padding(20) }
                    }
                }
            } else {
                ScrollView { VStack(spacing: 24) { if contentFirst { content; controls } else { controls; content } }.padding(20) }
            }
        }
    }
}
