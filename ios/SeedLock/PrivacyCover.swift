import SwiftUI
import UIKit

/// A window of SeedLock's own, above everything it shows (its prompt and its messages too), that covers the screen:
/// while SeedLock is left, so that the picture iOS keeps of it for the app switcher shows nothing; while it is partly
/// covered with a secret on the screen; and while the screen is recorded or mirrored, since iOS lets no app refuse
/// that (nor a screenshot, which it only reports afterwards). `AppModel` says when.
final class PrivacyCover {
    private var window: UIWindow?
    private var shownRecorded = false
    private var observers: [NSObjectProtocol] = []

    /// Called when the screen starts or stops being recorded or mirrored.
    var recordingChanged: (() -> Void)?
    /// Called right after a screenshot was taken.
    var screenshotTaken: (() -> Void)?

    init() {
        let center = NotificationCenter.default
        let main = OperationQueue.main
        observers.append(center.addObserver(forName: UIScreen.capturedDidChangeNotification, object: nil, queue: main) {
            [weak self] _ in
            self?.recordingChanged?()
        })
        observers.append(center.addObserver(
            forName: UIApplication.userDidTakeScreenshotNotification, object: nil, queue: main
        ) { [weak self] _ in
            self?.screenshotTaken?()
        })
        // at once when SeedLock is left, before iOS takes its picture for the app switcher
        observers.append(center.addObserver(forName: UIScene.didEnterBackgroundNotification, object: nil, queue: main) {
            [weak self] _ in
            self?.show(recorded: self?.recorded ?? false)
        })
    }

    /// Whether the screen is recorded or mirrored now (Control Center's screen recording, AirPlay, a cable).
    var recorded: Bool {
        windowScene?.screen.isCaptured ?? false
    }

    func show(recorded: Bool) {
        if window != nil && shownRecorded == recorded {
            return
        }
        guard let scene = windowScene else { return }
        let cover = window ?? UIWindow(windowScene: scene)
        cover.windowLevel = UIWindow.Level(rawValue: UIWindow.Level.alert.rawValue + 1)
        cover.rootViewController = UIHostingController(rootView: CoverView(recorded: recorded))
        cover.isHidden = false
        window = cover
        shownRecorded = recorded
    }

    func hide() {
        window?.isHidden = true
        window = nil
    }

    private var windowScene: UIWindowScene? {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
    }
}

/// What the cover shows: SeedLock's colours and name, and why, while the screen is recorded.
struct CoverView: View {
    let recorded: Bool

    var body: some View {
        ZStack {
            Color(red: 0x34 / 255.0, green: 0x45 / 255.0, blue: 0x4c / 255.0)
                .ignoresSafeArea()
            VStack(spacing: 16) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 44))
                Text(verbatim: "SeedLock")
                    .font(.largeTitle.bold())
                if recorded {
                    Text(verbatim: "The screen is being recorded or mirrored, so SeedLock shows nothing. Stop the "
                        + "recording or the mirroring to go on.")
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 32)
                }
            }
            .foregroundStyle(Color.white)
        }
    }
}
