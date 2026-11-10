import SwiftUI
import UIKit

/// SeedLock for iPhone (a preview). Seeds live in SeedLock's Rust code (crates/seedlock-ffi, the same as Android's),
/// in memory that is wiped when it is let go of: the Swift side holds a seed only while a screen needs it
/// (`OpenSeed`), and leaving the app, or the iPhone going online, closes that screen at once (`AppModel`).
@main
struct SeedLockApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
        }
    }
}

/// Refuses keyboards from other apps inside SeedLock: iOS then shows its own keyboard in every field. A keyboard from
/// another app could keep what is typed (a password, a seed's words) or, with full access, send it away.
final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        shouldAllowExtensionPointIdentifier extensionPointIdentifier: UIApplication.ExtensionPointIdentifier
    ) -> Bool {
        extensionPointIdentifier != .keyboard
    }
}
