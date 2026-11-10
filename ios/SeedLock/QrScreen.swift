import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// A QR code to show on a screen of its own: what it says, and how the screen and its saved picture describe it.
struct QrShown: Hashable {
    let title: String
    let text: String
    let hint: String
    /// The saved picture's name, without ".png".
    let pictureName: String
    /// The lines under the code in the saved picture.
    let caption: [String]

    /// A backup's QR code, with its name (`backupQrText`), for SeedLock on another phone or computer to scan.
    static func backup(_ name: String, text: String) throws -> QrShown {
        QrShown(
            title: "Backup \"\(name)\"",
            text: try backupQrText(text: text, name: name),
            hint: "The backup's QR code, with its name: scan it with SeedLock on another phone or computer.",
            pictureName: "\(name) QR code",
            caption: ["SeedLock backup \"\(name)\"", "Encrypted: useless without its password"]
        )
    }

    /// Its picture as a PNG file to save, to print or keep.
    func pictureFile() throws -> ExportFile {
        try qrPictureFile(text, name: pictureName, caption: caption)
    }
}

/// The picture of the QR code of `text` as a PNG file to save, to print or keep, with the lines of `caption` under the
/// code. `name`: the file's, without ".png".
func qrPictureFile(_ text: String, name: String, caption: [String]) throws -> ExportFile {
    guard let png = qrPicture(try qrCode(text: text), scale: 12, caption: caption).pngData() else {
        throw UserError("The picture could not be made.")
    }
    return ExportFile(data: png, type: .png, name: name)
}

/// The QR code of `text` in a screen's list, as wide as the screen: one pixel per module, made larger without
/// smoothing, so that the modules stay sharp.
struct QrCodeView: View {
    let text: String

    var body: some View {
        switch Result(catching: { try qrCode(text: text) }) {
        case .success(let code):
            Image(uiImage: qrPicture(code, scale: 1))
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .frame(maxWidth: .infinity)
        case .failure(let error):
            Text(verbatim: "The QR code cannot be made: \(userText(error))")
                .foregroundStyle(Color.red)
        }
    }
}

/// Shows a QR code for another camera to read: sharp, as wide as the screen, with the screen at full brightness and
/// kept on meanwhile (`Brightness`). It can be saved as a picture, with its caption under it.
struct QrScreen: View {
    let shown: QrShown
    @State private var picture: UIImage?
    @State private var problem: String?
    @State private var export: ExportFile?
    @State private var saved: String?

    var body: some View {
        List {
            Section {
                Text(verbatim: shown.hint)
                if let picture {
                    // one pixel per module, made larger without smoothing: the modules stay sharp
                    Image(uiImage: picture)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity)
                }
                if let problem {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
            Section {
                Button("Save it as a picture (PNG)") {
                    do {
                        export = try shown.pictureFile()
                    } catch {
                        saved = "Not saved: \(userText(error))"
                    }
                }
                if let saved {
                    Text(verbatim: saved)
                        .font(.footnote)
                }
            } footer: {
                Text(verbatim: ExportFile.placeHint)
            }
        }
        .navigationTitle(shown.title)
        .navigationBarTitleDisplayMode(.inline)
        .exportsFile($export, saved: $saved)
        .onAppear {
            Brightness.qrShowing(true)
            if picture == nil {
                do {
                    picture = qrPicture(try qrCode(text: shown.text), scale: 1)
                } catch {
                    problem = "The QR code cannot be made: \(userText(error))"
                }
            }
        }
        .onDisappear {
            Brightness.qrShowing(false)
        }
    }
}

/// A QR code as a picture: each dark module a black square of `scale` pixels on white, with the border of 4 modules
/// that cameras need, and the lines of `caption` under it, each cut to the width.
func qrPicture(_ code: QrImage, scale: Int, caption: [String] = []) -> UIImage {
    let width = Int(code.width)
    let modules = [UInt8](code.modules)
    let unit = CGFloat(scale)
    let side = CGFloat(width + 8) * unit
    let lineHeight: CGFloat = 40
    let height = side + (caption.isEmpty ? 0 : CGFloat(caption.count) * lineHeight + 16)
    let format = UIGraphicsImageRendererFormat()
    format.scale = 1
    format.opaque = true
    return UIGraphicsImageRenderer(size: CGSize(width: side, height: height), format: format).image { context in
        UIColor.white.setFill()
        context.fill(CGRect(x: 0, y: 0, width: side, height: height))
        UIColor.black.setFill()
        if modules.count >= width * width {
            for y in 0..<width {
                for x in 0..<width where modules[y * width + x] == 1 {
                    context.fill(CGRect(x: CGFloat(x + 4) * unit, y: CGFloat(y + 4) * unit, width: unit, height: unit))
                }
            }
        }
        // the text goes below the code and its white border, which cameras need to find the code
        let style = NSMutableParagraphStyle()
        style.alignment = .center
        style.lineBreakMode = .byTruncatingTail
        let attributes: [NSAttributedString.Key: Any] = [
            .font: UIFont.systemFont(ofSize: 28), .foregroundColor: UIColor.black, .paragraphStyle: style,
        ]
        var top = side + 4
        for line in caption {
            NSString(string: line).draw(
                in: CGRect(x: 4 * unit, y: top, width: side - 8 * unit, height: lineHeight), withAttributes: attributes
            )
            top += lineHeight
        }
    }
}

/// A file to save where the person chooses, with iOS's file picker: on this iPhone, on a USB stick, or in a cloud
/// folder.
struct ExportFile: FileDocument {
    static var readableContentTypes: [UTType] {
        [.data]
    }

    static var writableContentTypes: [UTType] {
        [.data, .png, .plainText]
    }

    /// What to know about the place iOS asks for.
    static let placeHint = "iOS then asks where to save it: on this iPhone, on a USB stick, or in a cloud folder "
        + "(Browse, at the bottom, shows them all)."

    let data: Data
    let type: UTType
    /// The file's name, as the picker first shows it.
    let name: String

    init(data: Data, type: UTType, name: String) {
        self.data = data
        self.type = type
        self.name = name
    }

    init(configuration: ReadConfiguration) throws {
        data = configuration.file.regularFileContents ?? Data()
        type = .data
        name = ""
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: data)
    }
}

extension View {
    /// Saves `export` where the person chooses, once it is set, and says in `saved` where it went.
    func exportsFile(_ export: Binding<ExportFile?>, saved: Binding<String?>) -> some View {
        fileExporter(
            isPresented: Binding(get: { export.wrappedValue != nil }, set: { if !$0 { export.wrappedValue = nil } }),
            document: export.wrappedValue,
            contentType: export.wrappedValue?.type ?? .data,
            defaultFilename: export.wrappedValue?.name
        ) { result in
            switch result {
            case .success(let url):
                saved.wrappedValue = "Saved: \"\(url.lastPathComponent)\", in the folder "
                    + "\"\(url.deletingLastPathComponent().lastPathComponent)\"."
            case .failure(let error):
                if let cocoa = error as? CocoaError, cocoa.code == .userCancelled {
                    return
                }
                saved.wrappedValue = "Not saved: \(userText(error))"
            }
        }
    }
}
