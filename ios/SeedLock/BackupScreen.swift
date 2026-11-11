import SwiftUI

/// One backup on this iPhone, and what can be done with it.
struct BackupScreen: View {
    let model: AppModel
    let name: String
    @State private var confirmDelete = false
    @State private var export: ExportFile?
    @State private var saved: String?

    var body: some View {
        let loaded = load()
        let fingerprint = model.entry(name).fingerprint
        List {
            if let problem = loaded.problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
            if let text = loaded.text, let info = loaded.info {
                Section {
                    Text(verbatim: "Encrypted backup \"\(name)\". Opening it takes its password, \(info.memoryMib) MiB "
                        + "of memory and a few seconds.")
                    if let fingerprint {
                        LabeledContent("Wallet fingerprint", value: fingerprint)
                    }
                } footer: {
                    if fingerprint == nil {
                        Text(verbatim: "Its wallet fingerprint shows here once the backup was opened with its password "
                            + "(for example with \"Check the backup\").")
                    } else {
                        Text(verbatim: "Without a BIP39 passphrase. With one, the seed is another wallet, with another "
                            + "fingerprint (SeedLock keeps nothing about it).")
                    }
                }
                Section {
                    Button("Sign a transaction") {
                        model.show(.sign(name))
                    }
                    Button("Wallet for Sparrow (xpub, descriptor, addresses)") {
                        model.askWallet(name: name, text: text, memoryMib: info.memoryMib) { open in
                            model.show(.sparrow(open, name))
                        }
                    }
                } header: {
                    Text(verbatim: "Use it")
                } footer: {
                    Text(verbatim: "With Sparrow on a computer: it prepares payments with the wallet for Sparrow (a "
                        + "watch-only wallet, which cannot sign them), and SeedLock signs them here.")
                }
                Section {
                    Button("Check the backup (its password, and the words on paper)") {
                        model.askPassword(name: name, text: text, memoryMib: info.memoryMib) { open in
                            model.show(.check(open))
                        }
                    }
                    Button("Show the words") {
                        model.askPassword(
                            name: name, text: text, memoryMib: info.memoryMib,
                            note: "Once the backup opens, its words show: make sure that nobody can see this screen "
                                + "and that no camera points at it. iOS lets no app block a screenshot: take none."
                        ) { open in
                            model.show(.words(open))
                        }
                    }
                } header: {
                    Text(verbatim: "Check it")
                }
                Section {
                    Button("Show it as a QR code") {
                        do {
                            model.show(.qr(try QrShown.backup(name, text: text)))
                        } catch {
                            saved = "Damaged backup: \(userText(error))"
                        }
                    }
                    Button("Save its QR code as a picture (PNG), to print or keep") {
                        do {
                            export = try QrShown.backup(name, text: text).pictureFile()
                        } catch {
                            saved = "Not saved: \(userText(error))"
                        }
                    }
                    Button("Save a copy as a file (.seedlock)") {
                        export = ExportFile(data: Data(text.utf8), type: .data, name: name + Backups.ext)
                    }
                    if let saved {
                        Text(verbatim: saved)
                            .font(.footnote)
                    }
                } header: {
                    Text(verbatim: "Copy it")
                } footer: {
                    Text(verbatim: "It is encrypted: without its password it is useless, so a copy is safe on a USB "
                        + "stick, on paper or in a cloud folder. " + ExportFile.placeHint)
                }
            }
            Section {
                Button("Delete it from this iPhone", role: .destructive) {
                    confirmDelete = true
                }
            } header: {
                Text(verbatim: "Remove it")
            }
        }
        .navigationTitle(model.entry(name).label)
        .navigationBarTitleDisplayMode(.inline)
        .exportsFile($export, saved: $saved)
        .confirmationDialog("Delete this backup?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                model.delete(name)
            }
        } message: {
            Text(verbatim: "\"\(name)\" is deleted from this iPhone only. Do this only if you have other copies of it "
                + "(or the words on paper).")
        }
    }

    /// The backup's text and what it says without its password, or what is wrong with it.
    private struct Loaded {
        var text: String?
        var info: BackupInfo?
        var problem: String?
    }

    private func load() -> Loaded {
        let text: String
        do {
            text = try model.backups.read(name)
        } catch {
            return Loaded(problem: "This backup's file cannot be read: \(userText(error))")
        }
        do {
            let info = try backupInfo(text: text)
            return Loaded(text: text, info: info)
        } catch {
            return Loaded(problem: "This backup is damaged: \(userText(error))")
        }
    }
}

/// The backup opened with its password: its fingerprint, and a comparison with the words on paper.
struct CheckScreen: View {
    let model: AppModel
    let open: OpenSeed
    @State private var words = ""
    @State private var verdict: String?
    @State private var matched = false
    @State private var note: String?

    var body: some View {
        List {
            // read here, and never kept by the screen: closing the screen lets go of the seed (OpenSeed.close)
            if let seed = open.seed {
                Section {
                    Label(
                        "Backup OK: it opens with this password, and its seed is valid.", systemImage: "checkmark.seal"
                    )
                        .foregroundStyle(Color.green)
                    LabeledContent("Wallet fingerprint", value: (try? seed.fingerprint()) ?? "unknown")
                } footer: {
                    Text(verbatim: "Without a BIP39 passphrase.")
                }
                Section {
                    TextField("Words", text: $words, axis: .vertical)
                        .lineLimit(3...8)
                        .autocorrectionDisabled(true)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.asciiCapable)
                    Button("Compare") {
                        compare()
                    }
                    if let verdict {
                        Text(verbatim: verdict)
                            .foregroundStyle(matched ? Color.green : Color.red)
                    }
                    if let note {
                        Text(verbatim: note)
                            .font(.footnote)
                    }
                } header: {
                    Text(verbatim: "Compare it with the words on paper")
                } footer: {
                    Text(verbatim: "Type the \(seed.wordCount()) words of your paper backup, separated by spaces. The "
                        + "first four letters of each word are enough.")
                }
            }
        }
        .navigationTitle("Check the backup")
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: words) { old, new in
            if new.count > old.count + 1, let said = model.pastedSecret() {
                note = said
            }
        }
    }

    private func compare() {
        guard let seed = open.seed else { return }
        let typed = words
        words = ""
        do {
            matched = try seed.matchesWords(words: typed)
            verdict = matched
                ? "MATCH: the words you typed are this backup's seed."
                : "MISMATCH: the words you typed are a different seed than this backup."
        } catch {
            matched = false
            verdict = "Not compared: \(userText(error))"
        }
    }
}
