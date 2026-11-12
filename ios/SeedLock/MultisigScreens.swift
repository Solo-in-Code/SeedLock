import SwiftUI
import UniformTypeIdentifiers

// Multisig wallets, as the command-line program's menu 10 has them: a seed's key for a multisig wallet in Sparrow
// (CosignerScreen), the multisig wallets kept on this iPhone (AddMultisigScreen, CheckMultisigScreen, MultisigScreen),
// and signing their transactions (SignScreens.swift, with the wallets as `RegisteredMultisig`).

/// Where Sparrow shows a multisig wallet's QR code, and saves its file.
private let fromSparrow = "Sparrow gives the wallet as a QR code (the wallet's Settings tab: under Script Policy, the "
    + "QR code button) or as a file (File > Export Wallet > Output Descriptor). It holds no secrets, only the wallet's "
    + "public keys."

/// Adds a multisig wallet that was made in Sparrow (the command-line program's menu 10, 2): from its QR code, read with
/// the camera, from the file Sparrow exported, or from its descriptor pasted in.
struct AddMultisigScreen: View {
    let model: AppModel
    @State private var text = ""
    @State private var scanning = false
    @State private var picking = false
    @State private var scanned: Result<MultisigWallet, Error>?
    @State private var problem: String?

    var body: some View {
        List {
            Section {
                Text(verbatim: "Once a multisig wallet (for example 2 of 3) is made in Sparrow, add it here: then the "
                    + "backups on this iPhone that are its cosigners can check and sign its transactions. "
                    + fromSparrow)
                    .font(.footnote)
            }
            Section {
                Button("Scan Sparrow's QR code") {
                    problem = nil
                    scanning = true
                }
                Button("Open the file that Sparrow exported") {
                    problem = nil
                    picking = true
                }
            } header: {
                Text(verbatim: "The wallet")
            }
            Section {
                TextField("wsh(sortedmulti(...", text: $text, axis: .vertical)
                    .lineLimit(3...8)
                    .font(.system(.footnote, design: .monospaced))
                    .autocorrectionDisabled(true)
                    .textInputAutocapitalization(.never)
                Button("Check this wallet") {
                    take(Result { try MultisigWallet.fromText(text: text) })
                }
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            } header: {
                Text(verbatim: "Or paste its descriptor")
            } footer: {
                Text(verbatim: "It starts with wsh(, sh(wsh( or sh( and ends with # and 8 characters; or paste the "
                    + "whole contents of the file that Sparrow exported.")
            }
            if let problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
        }
        .navigationTitle("Add a multisig wallet")
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $scanning, onDismiss: scanEnded) {
            ScanScreen(target: .multisigWallet) { scanner in
                scanned = Result { try scanner.multisigWallet() }
                scanning = false
            } cancel: {
                scanning = false
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.data]) { result in
            opened(result)
        }
    }

    private func scanEnded() {
        if let read = scanned {
            scanned = nil
            take(read)
        }
    }

    private func opened(_ result: Result<URL, Error>) {
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
        let read = Result { () throws -> MultisigWallet in
            let data = try readFile(url, limit: 1_000_000, what: "a wallet's descriptor")
            guard let contents = String(data: data, encoding: .utf8) else {
                throw UserError("This file is not text.")
            }
            return try MultisigWallet.fromText(text: contents)
        }
        // the next screen shows once the file picker has gone
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            take(read)
        }
    }

    /// A multisig wallet that was read: unless it is already on this iPhone, it is shown for checking before it is
    /// added.
    private func take(_ read: Result<MultisigWallet, Error>) {
        switch read {
        case .success(let wallet):
            problem = nil
            if let same = model.multisig.same(wallet) {
                model.setPath([.multisig(same)])
                model.say("Already on this iPhone", "This multisig wallet is already on this iPhone, as \"\(same)\". "
                    + "Nothing was changed.")
            } else {
                model.show(.checkMultisig(MultisigShown(wallet)))
            }
        case .failure(let error):
            problem = "Not added: \(userText(error))"
        }
    }
}

/// A multisig wallet that was read (it holds no secret).
final class MultisigShown: Hashable {
    let wallet: MultisigWallet

    init(_ wallet: MultisigWallet) {
        self.wallet = wallet
    }

    static func == (a: MultisigShown, b: MultisigShown) -> Bool {
        a === b
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(ObjectIdentifier(self))
    }
}

/// A multisig wallet read from Sparrow, to check before it is added (as the command-line program asks): its kind, its
/// cosigners, and its first receive address, which Sparrow must show too; then its name on this iPhone.
struct CheckMultisigScreen: View {
    let model: AppModel
    let shown: MultisigShown
    @State private var name: String
    @State private var problem: String?

    init(model: AppModel, shown: MultisigShown) {
        self.model = model
        self.shown = shown
        _name = State(initialValue: shown.wallet.defaultName())
    }

    var body: some View {
        let wallet = shown.wallet
        List {
            Section {
                Text(verbatim: "Multisig wallet: \(wallet.label()) signatures needed, \(wallet.kindName()), "
                    + "\(wallet.networkName()).")
            }
            CosignersSection(model: model, wallet: wallet, withKeys: true)
            switch Result(catching: { try wallet.receiveAddress(index: 0) }) {
            case .success(let first):
                Section {
                    Text(verbatim: first)
                        .font(.system(.body, design: .monospaced))
                } header: {
                    Text(verbatim: "Its first receive address")
                } footer: {
                    Text(verbatim: "Sparrow must show exactly this one: in the wallet's Addresses tab, the first "
                        + "receive address.")
                }
                Section {
                    TextField("Name", text: $name)
                        .autocorrectionDisabled(true)
                        .textInputAutocapitalization(.never)
                } header: {
                    Text(verbatim: "Its name on this iPhone")
                } footer: {
                    Text(verbatim: "It also names its file when you save a copy.")
                }
                Section {
                    Button("It matches, and these are the right cosigners: add it") {
                        add()
                    }
                    Button("Cancel: do not add it") {
                        model.back()
                    }
                    if let problem {
                        Text(verbatim: problem)
                            .foregroundStyle(Color.red)
                    }
                } header: {
                    Text(verbatim: "Does it match?")
                }
            case .failure(let error):
                Section {
                    Text(verbatim: userText(error))
                        .foregroundStyle(Color.red)
                }
            }
        }
        .navigationTitle("Add a multisig wallet")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func add() {
        problem = nil
        let clean: String
        do {
            clean = try checkWalletName(typed: name)
        } catch {
            problem = userText(error)
            return
        }
        if let taken = model.multisig.taken(clean) {
            problem = "There is already a multisig wallet named \"\(taken)\" on this iPhone. Please choose another "
                + "name."
            return
        }
        do {
            try model.multisig.save(clean, contents: shown.wallet.fileContents(name: clean))
        } catch {
            problem = "Not saved: \(userText(error))"
            return
        }
        model.multisigAdded(clean)
    }
}

/// The cosigners of a multisig wallet, in its descriptor's order (with their keys when `withKeys`), each with the
/// backups on this iPhone that have its fingerprint.
struct CosignersSection: View {
    let model: AppModel
    let wallet: MultisigWallet
    let withKeys: Bool

    var body: some View {
        let cosigners = wallet.cosigners()
        let here = cosigners.map { model.backups.names(withFingerprintIn: [$0.fingerprint]) }
        Section {
            ForEach(cosigners.indices, id: \.self) { index in
                VStack(alignment: .leading, spacing: 4) {
                    Text(verbatim: "\(index + 1))  \(cosigners[index].fingerprint)   \(cosigners[index].derivation)")
                    if !here[index].isEmpty {
                        let names = here[index].map { "\"\($0)\"" }.joined(separator: " or ")
                        Text(verbatim: "Backup on this iPhone: \(names)")
                            .foregroundStyle(Color.green)
                    }
                    if withKeys {
                        Text(verbatim: cosigners[index].xpub)
                            .font(.system(.footnote, design: .monospaced))
                    }
                }
            }
        } header: {
            Text(verbatim: "Cosigners (\(wallet.cosignerCount()))")
        } footer: {
            if here.contains(where: { $0.isEmpty }) {
                Text(verbatim: "A backup on this iPhone shows here once it was opened (a backup used with a BIP39 "
                    + "passphrase is another wallet, with another fingerprint, which SeedLock does not keep).")
            }
        }
    }
}

/// A multisig wallet on this iPhone: what it is, signing its transactions with the backups here that are its
/// cosigners, a copy of its file, and deleting it.
struct MultisigScreen: View {
    let model: AppModel
    let name: String
    @State private var export: ExportFile?
    @State private var saved: String?
    @State private var confirmDelete = false

    var body: some View {
        List {
            switch Result(catching: { try model.multisig.wallet(name) }) {
            case .success(let wallet):
                details(wallet)
            case .failure(let error):
                Section {
                    Text(verbatim: "This multisig wallet's file cannot be read: \(userText(error))")
                        .foregroundStyle(Color.red)
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
        .navigationTitle(name)
        .navigationBarTitleDisplayMode(.inline)
        .exportsFile($export, saved: $saved)
        .confirmationDialog("Delete this multisig wallet?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                model.deleteMultisig(name)
            }
        } message: {
            Text(verbatim: "\"\(name)\" is deleted from this iPhone only: its coins stay where they are, and Sparrow "
                + "keeps the wallet. To sign its transactions here again, add it again.")
        }
    }

    @ViewBuilder
    private func details(_ wallet: MultisigWallet) -> some View {
        Section {
            Text(verbatim: "Multisig wallet \"\(name)\": \(wallet.label()), \(wallet.kindName()), "
                + "\(wallet.networkName()).")
        }
        CosignersSection(model: model, wallet: wallet, withKeys: false)
        Section {
            switch Result(catching: { try wallet.receiveAddress(index: 0) }) {
            case .success(let first):
                Text(verbatim: first)
                    .font(.system(.body, design: .monospaced))
            case .failure(let error):
                Text(verbatim: userText(error))
                    .foregroundStyle(Color.red)
            }
        } header: {
            Text(verbatim: "First receive address")
        } footer: {
            Text(verbatim: "Sparrow shows the same one, in the wallet's Addresses tab.")
        }
        let here = cosignersHere(wallet, backups: model.backups)
        Section {
            ForEach(here.indices, id: \.self) { index in
                Button("Sign a transaction with \"\(here[index].backup)\" (cosigner \(here[index].position))") {
                    model.show(.sign(here[index].backup))
                }
            }
            if here.isEmpty {
                Text(verbatim: "Open a backup on this iPhone that is one of its cosigners and choose \"Sign a "
                    + "transaction\". (Once a backup was opened here, its fingerprint shows beside its name, and its "
                    + "button here.)")
                    .font(.footnote)
            }
        } header: {
            Text(verbatim: "Signing its transactions")
        } footer: {
            Text(verbatim: "\(wallet.threshold()) of the \(wallet.cosignerCount()) cosigners sign each payment, each "
                + "with its own seed. SeedLock checks every input of this wallet, and that the change really returns "
                + "to it.")
        }
        Section {
            Button("Save a copy as a file (\(MultisigWallets.ext))") {
                do {
                    export = ExportFile(
                        data: Data(try model.multisig.read(name).utf8), type: .data, name: name + MultisigWallets.ext
                    )
                } catch {
                    saved = "Not saved: \(userText(error))"
                }
            }
            if let saved {
                Text(verbatim: saved)
                    .font(.footnote)
            }
        } header: {
            Text(verbatim: "Copy it")
        } footer: {
            Text(verbatim: "No secrets in it, but anyone who reads it can see all the wallet's addresses. Sparrow "
                + "opens the file (File > Import Wallet > Output Descriptor), and SeedLock on a computer reads it in "
                + "the \"multisig wallets\" folder inside its \"seedlocker backups\" folder. " + ExportFile.placeHint)
        }
    }
}

/// A backup's keys for a multisig wallet (public keys only: the wallet they came from was closed at once).
final class CosignerShown: Hashable {
    /// The backup's name.
    let name: String
    let cosigning: Cosigning

    init(name: String, cosigning: Cosigning) {
        self.name = name
        self.cosigning = cosigning
    }

    static func == (a: CosignerShown, b: CosignerShown) -> Bool {
        a === b
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(ObjectIdentifier(self))
    }
}

/// Opens the wallet of the backup `name` and lays its key out for a multisig wallet in Sparrow (the command-line
/// program's menu 10, 1). When the seed is a cosigner of a multisig wallet on this iPhone, that wallet's kind and
/// network are taken; otherwise the screen asks for them. Only public keys are kept: the wallet is closed as soon as
/// they are known.
func showCosignerKey(_ model: AppModel, backup name: String, text: String, memoryMib: Int32) {
    // the network does not matter here: the screen writes the key for either
    model.askWallet(name: name, text: text, memoryMib: memoryMib, network: .mainnet) { open in
        defer {
            open.close()
        }
        guard let wallet = open.wallet else { return }
        do {
            let found = try cosigning(wallet, backup: name, registered: model.multisig.all())
            model.show(.cosigner(CosignerShown(name: name, cosigning: found)))
        } catch {
            model.say("Not opened", userText(error))
        }
    }
}

/// A seed's key for a multisig wallet, laid out for Sparrow: one QR code to scan (which names the keystore after the
/// backup), or three fields to type. Only public keys are on this screen. It is at full brightness, for Sparrow's
/// camera.
struct CosignerScreen: View {
    let shown: CosignerShown
    @State private var mainnet = true
    @State private var kind: MultisigKind = .nativeSegwit
    @State private var export: ExportFile?
    @State private var saved: String?

    var body: some View {
        List {
            switch shown.cosigning {
            case .known(let wallet, let position, let key):
                Section {
                    Text(verbatim: "Seed \(key.fingerprint) as a cosigner of a multisig wallet: \(key.kindName), "
                        + "\(key.network).")
                    Text(verbatim: "This seed is cosigner \(position) of the multisig wallet \"\(wallet)\" on this "
                        + "iPhone: its kind and network are taken from there.")
                        .font(.footnote)
                }
                keySections(key)
            case .choose(let keys):
                Section {
                    Picker("Network", selection: $mainnet) {
                        Text(verbatim: "Real bitcoin (mainnet)").tag(true)
                        Text(verbatim: "Testing (testnet or signet)").tag(false)
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                    Picker("Script Type", selection: $kind) {
                        ForEach(multisigKinds, id: \.self) { each in
                            Text(verbatim: multisigKindName(kind: each)).tag(each)
                        }
                    }
                } header: {
                    Text(verbatim: "Which multisig wallet?")
                } footer: {
                    Text(verbatim: "Sparrow calls the wallet's kind its \"Script Type\": all its cosigners use the "
                        + "same one, and Native Segwit is the usual one.")
                }
                if let pair = keys[kind] {
                    let key = mainnet ? pair.mainnet : pair.testing
                    Section {
                        Text(verbatim: "Seed \(key.fingerprint) as a cosigner of a multisig wallet: \(key.kindName), "
                            + "\(key.network).")
                    }
                    keySections(key)
                }
            }
        }
        .navigationTitle("Key for a multisig wallet")
        .navigationBarTitleDisplayMode(.inline)
        .exportsFile($export, saved: $saved)
        .onChange(of: mainnet) {
            saved = nil
        }
        .onChange(of: kind) {
            saved = nil
        }
        .onAppear {
            Brightness.qrShowing(true)
        }
        .onDisappear {
            Brightness.qrShowing(false)
        }
    }

    @ViewBuilder
    private func keySections(_ key: CosignerKey) -> some View {
        Section {
            Text(verbatim: "In Sparrow: File > New Wallet, Policy Type \"Multi Signature\" (for example 2 of 3), "
                + "Script Type \"\(key.kindName)\". For this seed's keystore choose \"Watch Only Wallet\", then click "
                + "the camera button next to the xpub field (\"Scan a QR code\") and hold this iPhone's screen toward "
                + "the computer's camera. Sparrow fills in the fields and names the keystore \"\(key.label)\""
                + (key.labelShortened ? " (Sparrow allows 16 characters)." : "."))
                .font(.footnote)
            QrCodeView(text: key.qr)
        } header: {
            Text(verbatim: "In one step: scan this QR code")
        }
        Section {
            Text(verbatim: "In this seed's keystore (Watch Only Wallet):")
                .font(.footnote)
            ValueRow(label: "Master fingerprint", value: key.fingerprint)
            ValueRow(label: "Derivation", value: key.derivation)
            ValueRow(label: "xpub", value: key.xpub)
            ValueRow(label: "In one piece, for wallets that ask for it that way", value: key.keyWithOrigin)
        } header: {
            Text(verbatim: "Or type the three fields")
        }
        Section {
            Text(verbatim: "When every cosigner's keystore is in and the wallet is saved in Sparrow, add the wallet to "
                + "this iPhone (on the first screen: \"Add a multisig wallet\"). Then SeedLock can check and sign its "
                + "transactions. " + fromSparrow)
                .font(.footnote)
        } header: {
            Text(verbatim: "Then")
        }
        Section {
            Button("Save its QR code as a picture (PNG)") {
                do {
                    export = try qrPictureFile(
                        key.qr, name: "\(shown.name)-cosigner-qr",
                        caption: ["SeedLock cosigner key \"\(shown.name)\" (\(key.fingerprint))",
                                  "\(key.kindName), \(key.network)"]
                    )
                } catch {
                    saved = "Not saved: \(userText(error))"
                }
            }
            Button("Save it as a text file (.txt)") {
                export = ExportFile(data: Data(key.text.utf8), type: .plainText, name: "\(shown.name)-cosigner")
            }
            if let saved {
                Text(verbatim: saved)
                    .font(.footnote)
            }
        } header: {
            Text(verbatim: "Save it as files")
        } footer: {
            Text(verbatim: "To take it to the computer. No secrets in them: only this seed's public key for the "
                + "wallet. " + ExportFile.placeHint)
        }
    }
}

/// A value to read, or to type into Sparrow, under its name.
struct ValueRow: View {
    let label: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(verbatim: label)
                .font(.footnote)
                .foregroundStyle(Color.secondary)
            Text(verbatim: value)
                .font(.system(.body, design: .monospaced))
        }
    }
}
