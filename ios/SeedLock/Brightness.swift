import UIKit

/// The screen at full brightness and kept on while a QR code is shown, for another camera to read it; as it was
/// before, once the code is gone or SeedLock is left.
enum Brightness {
    private static var before: CGFloat?

    static func qrShowing(_ on: Bool) {
        let screen = UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.screen }.first
        if on {
            if before == nil {
                before = screen?.brightness
            }
            screen?.brightness = 1
            UIApplication.shared.isIdleTimerDisabled = true
        } else {
            if let before {
                screen?.brightness = before
            }
            before = nil
            UIApplication.shared.isIdleTimerDisabled = false
        }
    }
}
