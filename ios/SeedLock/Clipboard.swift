import UIKit
import UniformTypeIdentifiers

/// The clipboard, only as far as SeedLock needs it: whether it holds text (iOS says so without SeedLock reading the
/// text, and without asking the person), whether it changed since SeedLock put something there (the same), emptying
/// it, and putting a new password there for the password manager on this iPhone.
enum Clipboard {
    static var holdsText: Bool {
        UIPasteboard.general.hasStrings
    }

    static func empty() {
        UIPasteboard.general.items = []
    }

    /// Puts a secret on the clipboard for this iPhone only (iOS does not hand it to a Mac or an iPad nearby), until
    /// iOS empties it after `minutes`. Gives the clipboard's count of changes after it.
    static func copySecret(_ text: String, minutes: Int) -> Int {
        UIPasteboard.general.setItems(
            [[UTType.utf8PlainText.identifier: text]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(TimeInterval(minutes * 60))]
        )
        return UIPasteboard.general.changeCount
    }

    /// Whether the clipboard still holds what was put there when its count of changes was `count`.
    static func stillHolds(_ count: Int) -> Bool {
        UIPasteboard.general.changeCount == count
    }
}
