import Foundation

/// A forward-only JSON reader over raw bytes.
///
/// The scan cache is 20 MB or more and its key order decides which duplicate record wins,
/// so `JSONSerialization` (which builds a dictionary and loses the order) is not usable here.
struct JSONScanner {
    struct Malformed: Error {}

    private let bytes: UnsafeBufferPointer<UInt8>
    private(set) var position = 0

    init(_ bytes: UnsafeBufferPointer<UInt8>, at position: Int = 0) {
        self.bytes = bytes
        self.position = position
    }

    private enum Byte {
        static let quote = UInt8(ascii: "\""), backslash = UInt8(ascii: "\\"), colon = UInt8(ascii: ":")
        static let comma = UInt8(ascii: ","), openBrace = UInt8(ascii: "{"), closeBrace = UInt8(ascii: "}")
        static let openBracket = UInt8(ascii: "["), closeBracket = UInt8(ascii: "]")
        static let minus = UInt8(ascii: "-"), zero = UInt8(ascii: "0"), nine = UInt8(ascii: "9")
        static let n = UInt8(ascii: "n")
    }

    mutating func peek() -> UInt8? {
        skipWhitespace()
        return position < bytes.count ? bytes[position] : nil
    }

    mutating func skipWhitespace() {
        while position < bytes.count {
            let byte = bytes[position]
            guard byte == 0x20 || byte == 0x0A || byte == 0x0D || byte == 0x09 else { return }
            position += 1
        }
    }

    mutating func expect(_ byte: UInt8) throws {
        guard peek() == byte else { throw Malformed() }
        position += 1
    }

    mutating func openObject() throws { try expect(Byte.openBrace) }
    mutating func openArray() throws { try expect(Byte.openBracket) }

    /// After a member or item: true past a comma, false past the closing byte of the container.
    mutating func nextMember() throws -> Bool { try nextElement(closing: Byte.closeBrace) }
    mutating func nextItem() throws -> Bool { try nextElement(closing: Byte.closeBracket) }

    private mutating func nextElement(closing: UInt8) throws -> Bool {
        guard let byte = peek() else { throw Malformed() }
        position += 1
        if byte == Byte.comma { return true }
        guard byte == closing else { throw Malformed() }
        return false
    }

    /// True when the next byte closes the container, which is then consumed.
    mutating func closesObject() -> Bool { closes(Byte.closeBrace) }
    mutating func closesArray() -> Bool { closes(Byte.closeBracket) }

    private mutating func closes(_ byte: UInt8) -> Bool {
        guard peek() == byte else { return false }
        position += 1
        return true
    }

    mutating func objectKey() throws -> String {
        let key = try string()
        try expect(Byte.colon)
        return key
    }

    mutating func isNull() -> Bool { peek() == Byte.n }

    mutating func consumeNull() throws {
        guard position + 4 <= bytes.count, bytes[position] == Byte.n else { throw Malformed() }
        position += 4
    }

    mutating func isString() -> Bool { peek() == Byte.quote }

    mutating func isObject() -> Bool { peek() == Byte.openBrace }

    mutating func isNumber() -> Bool {
        guard let byte = peek() else { return false }
        return byte == Byte.minus || (Byte.zero...Byte.nine).contains(byte)
    }

    mutating func rewind(to position: Int) { self.position = position }

    mutating func string() throws -> String {
        try expect(Byte.quote)
        let start = position
        while position < bytes.count {
            let byte = bytes[position]
            if byte == Byte.quote {
                let text = String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<position]), as: UTF8.self)
                position += 1
                return text
            }
            if byte == Byte.backslash { break }
            position += 1
        }
        guard position < bytes.count else { throw Malformed() }
        return try escapedString(from: start)
    }

    private mutating func escapedString(from start: Int) throws -> String {
        var out = Array(bytes[start..<position])
        while position < bytes.count {
            let byte = bytes[position]
            position += 1
            if byte == Byte.quote { return String(decoding: out, as: UTF8.self) }
            guard byte == Byte.backslash else { out.append(byte); continue }
            guard position < bytes.count else { throw Malformed() }
            let escape = bytes[position]
            position += 1
            switch escape {
            case UInt8(ascii: "n"): out.append(0x0A)
            case UInt8(ascii: "t"): out.append(0x09)
            case UInt8(ascii: "r"): out.append(0x0D)
            case UInt8(ascii: "b"): out.append(0x08)
            case UInt8(ascii: "f"): out.append(0x0C)
            case UInt8(ascii: "u"): out.append(contentsOf: try unicodeEscape())
            default: out.append(escape)
            }
        }
        throw Malformed()
    }

    private mutating func unicodeEscape() throws -> [UInt8] {
        var value = try hex4()
        if (0xD800..<0xDC00).contains(value), position + 1 < bytes.count,
           bytes[position] == Byte.backslash, bytes[position + 1] == UInt8(ascii: "u") {
            let saved = position
            position += 2
            let low = try hex4()
            if (0xDC00..<0xE000).contains(low) {
                value = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00)
            } else {
                position = saved
            }
        }
        let scalar = Unicode.Scalar(value) ?? "\u{FFFD}"
        return Array(String(Character(scalar)).utf8)
    }

    private mutating func hex4() throws -> UInt32 {
        guard position + 4 <= bytes.count else { throw Malformed() }
        var value: UInt32 = 0
        for _ in 0..<4 {
            let byte = bytes[position]
            position += 1
            let digit: UInt32
            switch byte {
            case UInt8(ascii: "0")...UInt8(ascii: "9"): digit = UInt32(byte - UInt8(ascii: "0"))
            case UInt8(ascii: "a")...UInt8(ascii: "f"): digit = UInt32(byte - UInt8(ascii: "a")) + 10
            case UInt8(ascii: "A")...UInt8(ascii: "F"): digit = UInt32(byte - UInt8(ascii: "A")) + 10
            default: throw Malformed()
            }
            value = value << 4 | digit
        }
        return value
    }

    mutating func number() throws -> Double {
        skipWhitespace()
        let start = position
        var negative = false
        if position < bytes.count, bytes[position] == Byte.minus {
            negative = true
            position += 1
        }
        var integer: Int64 = 0
        var digits = 0
        while position < bytes.count, (Byte.zero...Byte.nine).contains(bytes[position]) {
            if digits < 18 { integer = integer * 10 + Int64(bytes[position] - Byte.zero) }
            digits += 1
            position += 1
        }
        guard digits > 0 else { throw Malformed() }
        var isPlainInteger = digits <= 18
        while position < bytes.count {
            let byte = bytes[position]
            guard (Byte.zero...Byte.nine).contains(byte) || byte == UInt8(ascii: ".") || byte == UInt8(ascii: "e")
                    || byte == UInt8(ascii: "E") || byte == UInt8(ascii: "+") || byte == Byte.minus else { break }
            isPlainInteger = false
            position += 1
        }
        if isPlainInteger { return negative ? -Double(integer) : Double(integer) }
        let text = String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<position]), as: UTF8.self)
        guard let value = Double(text), value.isFinite else { throw Malformed() }
        return value
    }

    /// Moves past one value of any type. It does not check that the bytes inside are valid JSON.
    mutating func skipValue() throws {
        guard let first = peek() else { throw Malformed() }
        switch first {
        case Byte.quote:
            try skipString()
        case Byte.openBrace, Byte.openBracket:
            var depth = 0
            repeat {
                guard position < bytes.count else { throw Malformed() }
                let byte = bytes[position]
                if byte == Byte.quote { try skipString(); continue }
                if byte == Byte.openBrace || byte == Byte.openBracket { depth += 1 }
                if byte == Byte.closeBrace || byte == Byte.closeBracket { depth -= 1 }
                position += 1
            } while depth > 0
        default:
            while position < bytes.count {
                let byte = bytes[position]
                if byte == Byte.comma || byte == Byte.closeBrace || byte == Byte.closeBracket || byte <= 0x20 { break }
                position += 1
            }
        }
    }

    private mutating func skipString() throws {
        position += 1
        while position < bytes.count {
            let byte = bytes[position]
            position += 1
            if byte == Byte.backslash { position += 1 } else if byte == Byte.quote { return }
        }
        throw Malformed()
    }
}
