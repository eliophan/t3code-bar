import AppKit

/// Loads bundled resources from the packaged app, falling back to the SwiftPM
/// resource bundle only when running outside an `.app` (`swift run`).
///
/// Never touch `Bundle.module` inside an `.app`: on some toolchains its accessor
/// fatalErrors there, which is how v1.0.0 crashed on launch. `build_app.sh` copies
/// everything in `Sources/Resources` flat into `Contents/Resources`.
enum AppResources {
    static func url(forResource name: String, withExtension ext: String) -> URL? {
        if let url = Bundle.main.url(forResource: name, withExtension: ext) {
            return url
        }
        #if SWIFT_PACKAGE
        if Bundle.main.bundleURL.pathExtension != "app" {
            return Bundle.module.url(forResource: name, withExtension: ext)
        }
        #endif
        return nil
    }

    static func image(named name: String, withExtension ext: String) -> NSImage? {
        url(forResource: name, withExtension: ext).flatMap { NSImage(contentsOf: $0) }
    }
}
