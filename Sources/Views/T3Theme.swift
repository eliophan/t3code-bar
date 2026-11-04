import SwiftUI

public enum T3Theme {
    // Canvas & Outer - Pure OLED Black matching T3 Code
    public static let canvas = Color(hex: "#000000")
    public static let sidebar = Color(hex: "#000000")
    public static let chrome = Color(hex: "#000000")

    // Surfaces
    public static let surface = Color(hex: "#111111")
    public static let surfaceRaised = Color(hex: "#161616")
    public static let surfaceOverlay = Color(hex: "#000000")

    // Borders
    public static let border = Color.white.opacity(0.08)
    public static let sidebarBorder = Color.white.opacity(0.08)
    public static let controlBorder = Color.white.opacity(0.12)

    // Rows & Controls - Exact T3 Code Dark Mode values
    public static let controlSurface = Color(hex: "#121214")
    public static let controlSurfaceHover = Color(hex: "#1e1e1e")
    public static let rowHover = Color(hex: "#1e1e1e")
    public static let rowActive = Color(hex: "#262626")
    public static let cardHover = Color(hex: "#1e1e1e")

    // Typography
    public static let textPrimary = Color(hex: "#f4f4f5")
    public static let textSecondary = Color(hex: "#a1a1aa")
    public static let textMuted = Color(hex: "#71717a")

    // Accents
    public static let accent = Color(hex: "#38bdf8")
    public static let accentSubtle = Color(hex: "#0284c7")
    public static let focus = Color(hex: "#38bdf8")

    // Status Tokens matching Sidebar.tsx exactly
    public static let statusWorking = Color(hex: "#38bdf8") // Sky-400
    public static let statusApproval = Color(hex: "#fcd34d") // Amber-300
    public static let statusInput = Color(hex: "#a5b4fc") // Indigo-300
    public static let statusDone = Color(hex: "#6ee7b7") // Emerald-300
    public static let statusFailed = Color(hex: "#f87171") // Red-400
    public static let statusRequest = Color(hex: "#f0abfc") // Fuchsia-300, T3 Code's guest color
    public static let statusSettled = Color(hex: "#71717a")
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
