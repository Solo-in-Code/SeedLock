import SwiftUI
import UIKit

/// Reads a transaction, a backup or a multisig wallet with the camera (`CameraReader`), over the screen that wants it.
/// `finished` receives the scanner once everything has been read; `cancel` closes it before that. The screen stays
/// on meanwhile.
struct ScanScreen: View {
    let target: ScanTarget
    let finished: (QrScanner) -> Void
    let cancel: () -> Void
    @State private var camera: CameraReader

    init(target: ScanTarget, finished: @escaping (QrScanner) -> Void, cancel: @escaping () -> Void) {
        self.target = target
        self.finished = finished
        self.cancel = cancel
        _camera = State(initialValue: CameraReader(target: target))
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 14) {
                Text(verbatim: instructions)
                    .font(.footnote)
                    .frame(maxWidth: .infinity, alignment: .leading)
                ZStack {
                    Color.black
                    if let session = camera.session {
                        CameraPreview(session: session)
                    } else {
                        ProgressView()
                    }
                }
                // the camera's whole picture, upright: 720 x 1280
                .aspectRatio(9.0 / 16.0, contentMode: .fit)
                .frame(maxHeight: 440)
                if camera.total > 0 {
                    ProgressView(value: Double(camera.have), total: Double(camera.total))
                }
                Text(verbatim: camera.status)
                    .font(.footnote)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Spacer()
            }
            .padding()
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        cancel()
                    }
                }
            }
        }
        .onAppear {
            camera.finished = finished
            camera.start()
            UIApplication.shared.isIdleTimerDisabled = true
        }
        .onDisappear {
            camera.stop()
            UIApplication.shared.isIdleTimerDisabled = false
        }
    }

    private var title: String {
        switch target {
        case .transaction:
            return "Scan the transaction"
        case .backup:
            return "Scan the backup"
        case .multisigWallet:
            return "Scan the multisig wallet"
        }
    }

    private var instructions: String {
        switch target {
        case .transaction:
            return "In Sparrow, show the transaction as a QR code (Show QR), and hold this iPhone's camera toward that "
                + "screen. The code is animated and changes all the time: that is normal."
        case .backup:
            return "Hold the backup's QR code (printed, or on another screen) in front of the camera, flat and steady."
        case .multisigWallet:
            return "In Sparrow, open the multisig wallet's Settings tab and click the QR code button under Script "
                + "Policy (\"Show as QR code\"), then hold this iPhone's camera toward that screen. The code may be "
                + "animated: keep the camera on it until every part has arrived."
        }
    }
}
