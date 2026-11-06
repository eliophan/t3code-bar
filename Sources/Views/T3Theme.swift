import SwiftUI

public enum T3Theme {
    // Canvas & Outer - Pure OLED Black matching T3 Code
    public static let canvas = Color(hex: "#000000")
    public static let sidebar = Color(hex: "#000000")

    // Borders
    public static let border = Color.white.opacity(0.08)

    // Rows & Controls - Exact T3 Code Dark Mode values
    public static let rowHover = Color(hex: "#1e1e1e")

    // Typography
    public static let textPrimary = Color(hex: "#f4f4f5")
    public static let textSecondary = Color(hex: "#a1a1aa")
    public static let textMuted = Color(hex: "#8b8b94") // 6:1 on black; the old #71717a was 4.4:1
    public static let textHover = Color(hex: "#d1d5db")
    public static let textFaint = Color(hex: "#52525b") // decorative only, never carries information

    // Token types on the Usage screen: T3 Code mixes its foreground into the background at
    // 60, 30, 72 and 100 percent. The canvas is black, so those are opacities of the primary text.
    public static let tokenInput = textPrimary.opacity(0.60)
    public static let tokenCacheRead = textPrimary.opacity(0.30)
    public static let tokenCacheWrite = textPrimary.opacity(0.72)
    public static let tokenOutput = textPrimary

    // Status colors: T3 Code's dark-mode foreground tokens
    public static let statusWorking = Color(hex: "#60a5fa") // blue-400, T3 Code info-foreground
    public static let statusApproval = Color(hex: "#fbbf24") // amber-400, T3 Code warning-foreground
    public static let statusInput = Color(hex: "#a5b4fc") // Indigo-300
    public static let statusDone = Color(hex: "#34d399") // emerald-400, T3 Code success-foreground
    public static let statusFailed = Color(hex: "#f87171") // Red-400
    public static let statusRequest = Color(hex: "#f0abfc") // Fuchsia-300, T3 Code's guest color
    public static let statusWaiting = Color(hex: "#a3a3a3") // neutral-400, T3 Code muted-foreground
    public static let statusSettled = textMuted
}

extension Color {
    init(hex: String) {
        let cleanHex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: cleanHex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch cleanHex.count {
        case 3: // RGB (12-bit)
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: // RGB (24-bit)
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: // ARGB (32-bit)
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 0, 0, 0)
        }
        self.init(
            .sRGB,
            red: Double(r) / 255,
            green: Double(g) / 255,
            blue: Double(b) / 255,
            opacity: Double(a) / 255
        )
    }
}
