import Foundation

/// Numbers the way T3 Code's Usage page writes them. Nothing here reads the Mac's locale,
/// because T3 Code formats in en-US whatever the system language is.
public enum TokenFormat {
    private static let units: [(scale: Double, symbol: String, word: String)] = [
        (1e12, "T", "trillion"), (1e9, "B", "billion"), (1e6, "M", "million"), (1e3, "K", "thousand"),
    ]

    /// `16B`, `1.50M`, `193M`, `4.2K`, `999`. A fraction of all zeros is dropped, a trailing zero is not.
    public static func compact(_ value: Double) -> String {
        let (text, unit) = scaled(value)
        return text + (unit?.symbol ?? "")
    }

    public static func compact(_ count: Int) -> String { compact(Double(count)) }

    /// For VoiceOver, where "193M" would be read letter by letter.
    public static func spoken(_ count: Int) -> String {
        let (text, unit) = scaled(Double(count))
        return text + (unit.map { " " + $0.word } ?? "")
    }

    /// `$1,234.57`, two decimals always.
    public static func currency(_ value: Double) -> String {
        let formatter = NumberFormatter()
        formatter.locale = Locale(identifier: "en_US")
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 2
        formatter.maximumFractionDigits = 2
        formatter.roundingMode = .halfUp
        return (value < 0 ? "-$" : "$") + (formatter.string(from: NSNumber(value: abs(value))) ?? "0.00")
    }

    /// For VoiceOver, where "$2,927.93" would be read symbol by symbol: `2,927 dollars and 93 cents`.
    public static func spokenCurrency(_ value: Double) -> String {
        let parts = currency(abs(value)).dropFirst().replacingOccurrences(of: ",", with: "").split(separator: ".")
        let dollars = Int(parts[0]) ?? 0
        let cents = Int(parts.count > 1 ? parts[1] : "0") ?? 0
        let sign = value < 0 ? "minus " : ""
        let dollarText = "\(grouped(String(dollars))) \(dollars == 1 ? "dollar" : "dollars")"
        guard cents > 0 else { return sign + dollarText }
        return sign + dollarText + " and \(cents) \(cents == 1 ? "cent" : "cents")"
    }

    /// A share between 0 and 1 as `12.3%`, with `<0.1%` for anything above zero and below that.
    public static func percent(_ share: Double) -> String {
        let percent = share * 100
        return percent > 0 && percent < 0.1 ? "<0.1%" : fixed(percent, 1) + "%"
    }

    public static func sessions(_ count: Int) -> String {
        "\(grouped(String(count))) \(count == 1 ? "session" : "sessions")"
    }

    private static func scaled(_ value: Double) -> (text: String, unit: (scale: Double, symbol: String, word: String)?) {
        let magnitude = abs(value)
        guard let unit = units.first(where: { magnitude >= $0.scale }) else {
            return (grouped(String(Int((value + 0.5).rounded(.down)))), nil)
        }
        let scaledValue = value / unit.scale
        let places = abs(scaledValue) >= 100 ? 0 : abs(scaledValue) >= 10 ? 1 : 2
        var text = fixed(scaledValue, places)
        if let dot = text.firstIndex(of: "."), text[text.index(after: dot)...].allSatisfy({ $0 == "0" }) {
            text = String(text[..<dot])
        }
        return (text, unit)
    }

    /// JavaScript's `toFixed`. It rounds the exact binary value half up, where `printf` rounds half to even.
    static func fixed(_ value: Double, _ places: Int) -> String {
        if value < 0 { return "-" + fixed(-value, places) }
        let factor = pow(10, Double(places))
        let scaledValue = value * factor
        let isExactTie = scaledValue - scaledValue.rounded(.down) == 0.5 && fma(value, factor, -scaledValue) == 0
        return String(format: "%.\(places)f", isExactTie ? value.nextUp : (value == 0 ? 0 : value))
    }

    private static func grouped(_ number: String) -> String {
        let sign = number.hasPrefix("-") ? "-" : ""
        let unsigned = number.dropFirst(sign.count)
        let whole = unsigned.prefix { $0 != "." }
        let rest = unsigned.dropFirst(whole.count)
        var out = ""
        for (offset, digit) in whole.reversed().enumerated() {
            if offset > 0, offset % 3 == 0 { out.append(",") }
            out.append(digit)
        }
        return sign + String(out.reversed()) + rest
    }
}
