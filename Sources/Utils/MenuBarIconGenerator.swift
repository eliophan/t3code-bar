import AppKit
import SwiftUI

@MainActor
public enum MenuBarIconGenerator {
    private static var iconCache: [String: NSImage] = [:]

    private static var baseLogoImage: NSImage? = {
        // 1. The packaged app: build_app.sh copies T3Logo.png into Contents/Resources.
        if let img = Bundle.main.image(forResource: "T3Logo") {
            return img
        }

        if let resURL = Bundle.main.resourceURL?.appendingPathComponent("T3Logo.png"),
           let img = NSImage(contentsOf: resURL) {
            return img
        }

        // 2. `swift run`: the SwiftPM resource bundle. Never touch Bundle.module
        // inside an .app. Older toolchains generate an accessor that only looks
        // at the .app root and the build machine's absolute path, and it
        // fatalErrors, so a CI-built release crashed on launch everywhere else.
        #if SWIFT_PACKAGE
        if Bundle.main.bundleURL.pathExtension != "app",
           let url = Bundle.module.url(forResource: "T3Logo", withExtension: "png"),
           let img = NSImage(contentsOf: url) {
            return img
        }
        #endif

        // 3. Fallback: check relative path from executable or working directory
        let localPath = "Sources/Resources/T3Logo.png"
        if FileManager.default.fileExists(atPath: localPath),
           let img = NSImage(contentsOfFile: localPath) {
            return img
        }

        return nil
    }()

    public static func createIcon(status: ThreadStatus, activeCount: Int = 0) -> NSImage {
        let cacheKey = "\(status.label)_\(status.color.description)_\(activeCount)"
        if let cached = iconCache[cacheKey] {
            return cached
        }

        let size = NSSize(width: 18, height: 18)
        let image = NSImage(size: size, flipped: false) { rect in
            if let logo = baseLogoImage {
                // Draw the authentic T3 logo mark
                logo.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
            } else {
                // Fallback squircle background if image cannot be loaded
                let squircleRect = NSRect(x: 0.5, y: 0.5, width: 17, height: 17)
                let path = NSBezierPath(roundedRect: squircleRect, xRadius: 4.0, yRadius: 4.0)
                NSColor(calibratedRed: 0.12, green: 0.10, blue: 0.14, alpha: 1.0).setFill()
                path.fill()
            }

            // Draw in-flight status indicator dot in bottom-right corner
            if status != .settled {
                let dotRadius: CGFloat = 2.5
                let dotCenter = NSPoint(x: 14.5, y: 3.5)
                let dotRect = NSRect(
                    x: dotCenter.x - dotRadius,
                    y: dotCenter.y - dotRadius,
                    width: dotRadius * 2,
                    height: dotRadius * 2
                )

                // Black outline for contrast against any menu bar background
                let outlinePath = NSBezierPath(ovalIn: dotRect.insetBy(dx: -0.75, dy: -0.75))
                NSColor.black.setFill()
                outlinePath.fill()

                // Status dot color
                let dotColor: NSColor
                switch status {
                case .connecting, .working, .goal:
                    dotColor = NSColor(calibratedRed: 0.22, green: 0.74, blue: 0.97, alpha: 1.0) // Sky
                case .approval, .woke, .limited:
                    dotColor = NSColor(calibratedRed: 0.99, green: 0.83, blue: 0.30, alpha: 1.0) // Amber
                case .input:
                    dotColor = NSColor(calibratedRed: 0.65, green: 0.71, blue: 0.99, alpha: 1.0) // Indigo
                case .failed:
                    dotColor = NSColor(calibratedRed: 0.99, green: 0.65, blue: 0.65, alpha: 1.0) // Red
                case .done:
                    dotColor = NSColor(calibratedRed: 0.43, green: 0.91, blue: 0.72, alpha: 1.0) // Emerald
                default:
                    dotColor = NSColor(calibratedRed: 0.86, green: 0.15, blue: 0.47, alpha: 1.0) // Pink
                }

                let dotPath = NSBezierPath(ovalIn: dotRect)
                dotColor.setFill()
                dotPath.fill()
            }

            return true
        }

        image.isTemplate = false
        iconCache[cacheKey] = image
        return image
    }
}
