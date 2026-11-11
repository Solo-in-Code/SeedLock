import SwiftUI
import UniformTypeIdentifiers

/// Adds a backup made elsewhere: from its file (.seedlock), its QR code (read with the camera), or its text pasted in,
/// under a name the person chooses.
struct AddBackupScreen: View {
    let model: AppModel
    @State private var text = ""
    @State private var name = ""
    @State private var problem: String?
    @State private var picking = false
    @State private var scanning = false
    /// The backup came from a QR code that carries no name (a code made from the backup's text alone).
    @State private var unnamed = false

    var body: some View {
        Form {
            Section {
                Button("Open its file (.seedlock)") {
                    picking = true
                }
                Button("Scan its QR code") {
                    problem = nil
                    scanning = true
                }
                PasteButton(payloadType: String.self) { strings in
                    // the clipboard's text, read only because the button was tapped
                    let pasted = strings.first ?? ""
                    DispatchQueue.main.async {
                        take(pasted, suggestedName: nil)
                    }
                }
            } header: {
                Text(verbatim: "The backup")
            } footer: {
                Text(verbatim: "Or type or paste its text below: the line that starts with SEEDLOCK1: (or the whole "
                    + "contents of its file).")
            }
            Section {
                TextEditor(text: $text)
                    .font(.system(.footnote, design: .monospaced))
                    .frame(minHeight: 120)
                    .autocorrectionDisabled(true)
                    .textInputAutocapitalization(.never)
            }
            Section {
                TextField("Name", text: $name)
                    .autocorrectionDisabled(true)
                    .textInputAutocapitalization(.never)
            } header: {
                Text(verbatim: "Its name on this iPhone")
            } footer: {
                if unnamed && name.isEmpty {
                    Text(verbatim: "This QR code carries no name: type one.")
                }
            }
            if let problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
            Section {
                Button("Add this backup") {
                    add()
                }
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .navigationTitle("Add a backup")
        .navigationBarTitleDisplayMode(.inline)
        .fileImporter(isPresented: $picking, allowedContentTypes: [.data]) { result in
            opened(result)
        }
        .sheet(isPresented: $scanning) {
            ScanScreen(target: .backup) { scanner in
                // the QR codes that SeedLock shows and saves carry the backup's name, unless it was made from the
                // backup's text alone
                if let read = scanner.backupText() {
                    take(read, suggestedName: nil)
                    unnamed = backupNameIn(text: read) == nil
                }
                scanning = false
            } cancel: {
                scanning = false
            }
        }
        .onChange(of: text) { _, new in
            // a backup's QR code made by SeedLock carries its name
            if name.isEmpty, let suggested = backupNameIn(text: new) {
                name = suggested
            }
        }
    }

    private func take(_ backup: String, suggestedName: String?) {
        problem = nil
        unnamed = false
        text = backup
        if let suggested = backupNameIn(text: backup) ?? suggestedName {
            name = suggested
        }
    }

    private func opened(_ result: Result<URL, Error>) {
        problem = nil
        let url: URL
        switch result {
        case .success(let picked):
            url = picked
        case .failure(let error):
            problem = "Not opened: \(userText(error))"
            return
        }
        // a file from the Files app, which SeedLock may read only now
        let access = url.startAccessingSecurityScopedResource()
        defer {
            if access {
                url.stopAccessingSecurityScopedResource()
            }
        }
        do {
            let data = try readFile(url, limit: 64 * 1024, what: "a SeedLock backup")
            guard let read = String(data: data, encoding: .utf8) else {
                problem = "That file is not a SeedLock backup."
                return
            }
            var fileName = url.lastPathComponent
            if fileName.lowercased().hasSuffix(Backups.ext) {
                fileName = String(fileName.dropLast(Backups.ext.count))
            }
            take(read, suggestedName: fileName)
        } catch {
            problem = "Not read: \(userText(error))"
        }
    }

    private func add() {
        problem = nil
        let backup = text
        let info: BackupInfo
        do {
            info = try backupInfo(text: backup)
        } catch {
            problem = "This is not a SeedLock backup: \(userText(error))"
            return
        }
        if let same = model.backups.same(backup) {
            model.showInstead(.backup(same))
            model.say("Already on this iPhone", "This backup is already on this iPhone, as \"\(same)\".")
            return
        }
        let clean: String
        do {
            clean = try checkBackupName(typed: name)
        } catch {
            problem = userText(error)
            return
        }
        if let taken = model.backups.taken(clean) {
            problem = "There is already a backup named \"\(taken)\" on this iPhone. Please choose another name."
            return
        }
        do {
            let contents = try backupFileContents(text: backup)
            try model.backups.save(clean, contents: contents)
        } catch {
            problem = "Not saved: \(userText(error))"
            return
        }
        model.refreshBackups()
        model.showInstead(.backup(clean))
        model.say(
            "Backup added",
            "The backup \"\(clean)\" is on this iPhone now. It opens with its password (that takes "
                + "\(info.memoryMib) MiB of memory and a few seconds). Its wallet fingerprint shows beside its name "
                + "once it was opened here: the backup is encrypted, so only its password tells which wallet it is."
        )
    }
}
