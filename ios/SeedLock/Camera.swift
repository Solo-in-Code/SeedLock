import AVFoundation
import SwiftUI
import UIKit

/// Reads QR codes with the camera on the back of the iPhone: a transaction to sign, a backup, or a multisig wallet.
/// The grey part of each camera picture goes to SeedLock's own QR reader (`QrScanner`: the Android app's and the
/// command-line program's), on a thread of its own; pictures that arrive while one is being read are dropped. Nothing
/// from the camera is kept. Once everything has been read, the camera goes off and `finished` receives the scanner.
///
/// This file is the only one that uses AVFoundation, and only to take pictures with the camera (the offline audit
/// checks that).
@Observable
final class CameraReader {
    let target: ScanTarget
    /// What to tell the person now.
    private(set) var status = "Opening the camera..."
    /// Parts of an animated code received so far, and how many it has (0 and 0: none yet).
    private(set) var have: Int32 = 0
    private(set) var total: Int32 = 0
    /// The camera's session, once it runs: the screen shows what the camera sees.
    private(set) var session: AVCaptureSession?
    @ObservationIgnored var finished: ((QrScanner) -> Void)?
    private let reader: PictureReader
    private let control = DispatchQueue(label: "org.seedlock.camera")
    @ObservationIgnored private var started = false
    @ObservationIgnored private var stopped = false

    init(target: ScanTarget) {
        self.target = target
        reader = PictureReader(scanner: QrScanner(target: target))
        reader.report = { [weak self] step in
            DispatchQueue.main.async {
                self?.took(step)
            }
        }
    }

    /// Asks for the camera the first time, then switches it on.
    func start() {
        guard !started, !stopped else { return }
        started = true
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            switchOn()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { granted in
                DispatchQueue.main.async {
                    if granted {
                        self.switchOn()
                    } else {
                        self.refused()
                    }
                }
            }
        default:
            refused()
        }
    }

    /// The camera goes off: the screen closed, or everything was read.
    func stop() {
        stopped = true
        reader.stop()
        if let session {
            self.session = nil
            control.async {
                session.stopRunning()
            }
        }
    }

    private func refused() {
        status = "SeedLock may not use the camera: allow it in the iPhone's Settings, on SeedLock's page there "
            + "(Camera). Or open the file instead."
    }

    /// Sets the camera up and starts it, away from the screen's thread (that takes a moment).
    private func switchOn() {
        guard !stopped else { return }
        let reader = self.reader
        control.async {
            let made = CameraReader.session(for: reader)
            if case .success(let session) = made {
                session.startRunning()
            }
            DispatchQueue.main.async {
                self.switchedOn(made)
            }
        }
    }

    private func switchedOn(_ made: Result<AVCaptureSession, Error>) {
        switch made {
        case .success(let session):
            guard !stopped else {
                // the screen closed meanwhile
                control.async {
                    session.stopRunning()
                }
                return
            }
            self.session = session
            status = "The camera is on. Looking for the QR code..."
        case .failure(let error):
            status = userText(error)
        }
    }

    /// What came of one picture (on the screen's thread).
    private func took(_ step: ScanStep?) {
        guard let step, !stopped else { return }
        if let message = step.message {
            status = message
        }
        if step.total > 0 {
            have = step.have
            total = step.total
        }
        if step.done {
            stop()
            finished?(reader.scanner)
        }
    }

    private static let busyCamera = "The camera cannot be used now. Close other apps that use it, and try again."

    /// The camera on the back, set up for QR codes: pictures of 1280 x 720 pixels (large enough for a dense code on a
    /// screen, small enough to read several a second), focusing on what is near.
    private static func session(for reader: PictureReader) -> Result<AVCaptureSession, Error> {
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back)
            ?? AVCaptureDevice.default(for: .video) else {
            return .failure(UserError("No camera was found on this iPhone. Open the file instead."))
        }
        let session = AVCaptureSession()
        session.beginConfiguration()
        defer {
            session.commitConfiguration()
        }
        if session.canSetSessionPreset(.hd1280x720) {
            session.sessionPreset = .hd1280x720
        }
        do {
            let input = try AVCaptureDeviceInput(device: device)
            guard session.canAddInput(input) else {
                return .failure(UserError(busyCamera))
            }
            session.addInput(input)
        } catch {
            return .failure(UserError("The camera could not be opened: \(userText(error))"))
        }
        let output = AVCaptureVideoDataOutput()
        // pictures whose first plane is their grey part
        output.videoSettings = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        ]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(reader, queue: reader.queue)
        guard session.canAddOutput(output) else {
            return .failure(UserError(busyCamera))
        }
        session.addOutput(output)
        do {
            try device.lockForConfiguration()
            if device.isFocusModeSupported(.continuousAutoFocus) {
                device.focusMode = .continuousAutoFocus
            }
            if device.isAutoFocusRangeRestrictionSupported {
                device.autoFocusRangeRestriction = .near
            }
            device.unlockForConfiguration()
        } catch {
            // the camera then focuses as it does by itself
        }
        return .success(session)
    }
}

/// Takes the camera's pictures, on a thread of its own, to the QR reader: one at a time, as fast as it reads them.
final class PictureReader: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let scanner: QrScanner
    let queue = DispatchQueue(label: "org.seedlock.camera.pictures")
    /// Called on `queue` with what came of each picture read (nil: it could not be read).
    var report: ((ScanStep?) -> Void)?
    private let lock = NSLock()
    private var ended = false

    init(scanner: QrScanner) {
        self.scanner = scanner
    }

    /// No more pictures are read.
    func stop() {
        lock.lock()
        ended = true
        lock.unlock()
    }

    private var stopped: Bool {
        lock.lock()
        defer {
            lock.unlock()
        }
        return ended
    }

    func captureOutput(
        _ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection
    ) {
        guard !stopped, let picture = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        CVPixelBufferLockBaseAddress(picture, .readOnly)
        var luma = Data()
        let width = CVPixelBufferGetWidthOfPlane(picture, 0)
        let height = CVPixelBufferGetHeightOfPlane(picture, 0)
        if CVPixelBufferGetPlaneCount(picture) > 0, let base = CVPixelBufferGetBaseAddressOfPlane(picture, 0) {
            // the picture's first plane is its grey part: one byte per pixel
            let bytesPerRow = CVPixelBufferGetBytesPerRowOfPlane(picture, 0)
            luma = packedLuma(base, width: width, height: height, bytesPerRow: bytesPerRow)
        }
        CVPixelBufferUnlockBaseAddress(picture, .readOnly)
        guard !luma.isEmpty else { return }
        let step = try? scanner.addPicture(width: Int32(width), height: Int32(height), luma: luma)
        if step?.done == true {
            stop()
        }
        report?(step)
    }
}

/// What the camera sees, whole: the reader reads the whole picture.
struct CameraPreview: UIViewRepresentable {
    let session: AVCaptureSession

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.backgroundColor = .black
        view.preview.session = session
        view.preview.videoGravity = .resizeAspect
        // the iPhone is upright, and the camera's pictures lie on their side
        if let connection = view.preview.connection, connection.isVideoRotationAngleSupported(90) {
            connection.videoRotationAngle = 90
        }
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {}
}

/// A view whose layer shows the camera's pictures.
final class PreviewView: UIView {
    override class var layerClass: AnyClass {
        AVCaptureVideoPreviewLayer.self
    }

    var preview: AVCaptureVideoPreviewLayer {
        // the layer is of the class above
        layer as! AVCaptureVideoPreviewLayer
    }
}
