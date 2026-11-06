import SwiftUI
import AppKit

/// Tracks hover for a clickable control and gives it the pointing-hand cursor, as thread rows do.
struct HoverArea<Content: View>: View {
    @State private var isHovered = false
    private let content: (Bool) -> Content

    init(@ViewBuilder content: @escaping (_ isHovered: Bool) -> Content) {
        self.content = content
    }

    var body: some View {
        content(isHovered)
            .onHover { hovering in
                withAnimation(.easeInOut(duration: 0.12)) { isHovered = hovering }
                if hovering { NSCursor.pointingHand.push() } else { NSCursor.pop() }
            }
            .pointingHandCursor()
            .onDisappear {
                if isHovered {
                    NSCursor.pop()
                    isHovered = false
                }
            }
    }
}
