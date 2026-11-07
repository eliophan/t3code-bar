import Foundation

public enum UpdateRetryPolicy {
    /// Nil means stop retrying and wait for the regular 6 hour check.
    public static func delay(afterConsecutiveFailures count: Int) -> TimeInterval? {
        switch count {
        case 1: 5 * 60
        case 2: 15 * 60
        case 3: 60 * 60
        default: nil
        }
    }
}
