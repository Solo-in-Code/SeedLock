import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Signing a transaction with the backup `name` (the command-line program's menu 6). The transaction comes from Sparrow
/// (its QR code, read with the camera, or its .psbt file); its network is found, the backup is opened, and everything
/// is shown for review (`ReviewScreen`): only then is it signed and given back to Sparrow (`SignedScreen`).
struct SignScreen: View {
    let model: AppModel
    let name: String
    @State private var scanning = false
    @State private var picking = false
    @State private var scanned: Transaction?
    @State private var other: OtherBackup?
    @State private var busy = false
    @State private var problem: String?

    /// A transaction that the backup may not be the one to sign, and why.
    struct OtherBackup {
        let transaction: Transaction
        let text: String
    }

    var body: some View {
        List {
            Section {
                Text(verbatim: "The payment is prepared in Sparrow on your online computer. This iPhone checks it, "
                    + "shows you every address and amount, and signs it only if you say yes.")
            }
            Section {
                Button("Scan the QR code that Sparrow shows") {
                    problem = nil
                    other = nil
                    scanning = true
                }
                Button("Open the .psbt file that Sparrow saved") {
                    problem = nil
                    other = nil
                    picking = true
                }
            } header: {
                Text(verbatim: "How does the transaction get to this iPhone?")
            }
            .disabled(busy)
            if let other {
                Section {
                    Text(verbatim: other.text)
                    Button("Open this backup anyway") {
                        self.other = nil
                        network(other.transaction)
                    }
                } header: {
                    Text(verbatim: "Another backup?")
                }
            }
            if busy {
                Section {
                    HStack(spacing: 12) {
                        ProgressView()
                        Text(verbatim: "Checking the transaction against your wallet ...")
                    }
                }
            }
            if let problem {
                Section {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
        }
        .navigationTitle("Sign a transaction")
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $scanning, onDismiss: scanEnded) {
            ScanScreen(target: .transaction) { scanner in
                scanned = scanner.transaction()
                scanning = false
            } cancel: {
                scanning = false
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.data]) { result in
            opened(result)
        }
    }

    /// The camera's screen has gone: the password prompt may show now.
    private func scanEnded() {
        if let transaction = scanned {
            scanned = nil
            received(transaction)
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
        let transaction: Transaction
        do {
            transaction = try Transaction.fromBytes(data: readFile(url, limit: 16 * 1024 * 1024, what: "a transaction"))
        } catch {
            problem = "Not a transaction: \(userText(error))"
            return
        }
        // the password prompt shows once the file picker has gone
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            received(transaction)
        }
    }

    /// Before the password is asked for: whether this backup is one of the wallets the transaction is for. Its
    /// fingerprint is known once it was opened on this iPhone; when it is not among the transaction's, the person
    /// learns which backup on this iPhone is, and may still open this one (with a BIP39 passphrase it is another
    /// wallet, with another fingerprint).
    private func received(_ transaction: Transaction) {
        problem = nil
        let signers = transaction.inputFingerprints()
        guard let ours = model.backups.fingerprint(name), !signers.isEmpty, !signers.contains(ours) else {
            network(transaction)
            return
        }
        let here = model.backups.names(withFingerprintIn: signers)
        other = OtherBackup(transaction: transaction, text: otherBackupText(signers: signers, ours: ours, here: here))
    }

    /// The network decides which wallet signs: the transaction's key paths say it, or the password prompt asks.
    private func network(_ transaction: Transaction) {
        switch transaction.networkHintWith(multisig: []) {
        case .mainnet:
            unlock(transaction, network: .mainnet, note: nil)
        case .testNetwork:
            unlock(transaction, network: .testnet, note: nil)
        case .unknown:
            unlock(transaction, network: nil, note: "This transaction does not say which network it is for.")
        case .conflicting:
            unlock(transaction, network: nil, note: "Some key paths in this transaction say mainnet and others a test "
                + "network: something is wrong with it.")
        }
    }

    /// Asks for the backup's password (and a BIP39 passphrase, and the network when the transaction does not say it),
    /// and opens the backup's wallet.
    private func unlock(_ transaction: Transaction, network: Net?, note: String?) {
        let text: String
        let info: BackupInfo
        do {
            text = try model.backups.read(name)
            info = try backupInfo(text: text)
        } catch {
            problem = "This backup cannot be read: \(userText(error))"
            return
        }
        model.askWallet(name: name, text: text, memoryMib: info.memoryMib, network: network, note: note) { open in
            check(transaction, with: open)
        }
    }

    /// The transaction checked against the wallet, away from the screen's thread; then its review.
    private func check(_ transaction: Transaction, with open: OpenWallet) {
        guard let wallet = open.wallet else { return }
        busy = true
        let started = model.workStarted()
        inBackground {
            try wallet.review(transaction: transaction, multisig: [])
        } done: { result in
            busy = false
            guard model.workEnded(started: started) else {
                open.close()
                return
            }
            switch result {
            case .success(let review):
                let route = Route.review(Signing(open, transaction: transaction, review: review))
                model.showInstead(route)
                if model.path.last != route {
                    open.close()
                }
            case .failure(let error):
                open.close()
                problem = "This transaction cannot be signed: \(userText(error))"
            }
        }
    }
}

/// A transaction to sign and the wallet that signs it, held while its review is on the screen: `close()` lets go of
/// the wallet.
final class Signing: Held, Hashable {
    private let open: OpenWallet
    let transaction: Transaction
    let review: TxReview

    init(_ open: OpenWallet, transaction: Transaction, review: TxReview) {
        self.open = open
        self.transaction = transaction
        self.review = review
    }

    var wallet: Wallet? {
        open.wallet
    }

    func close() {
        open.close()
    }

    static func == (a: Signing, b: Signing) -> Bool {
        a === b
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(ObjectIdentifier(self))
    }
}

/// Everything to look at before signing; the transaction is signed only from here, on request.
struct ReviewScreen: View {
    let model: AppModel
    let signing: Signing
    @State private var busy = false
    @State private var problem: String?

    var body: some View {
        let review = signing.review
        List {
            Section {
                LabeledContent("Network", value: review.network)
                LabeledContent("Wallet fingerprint", value: review.fingerprint)
                if let multisig = review.multisig {
                    Text(verbatim: multisig)
                        .foregroundStyle(Color.orange)
                }
            }
            Section {
                ForEach(review.inputs, id: \.index) { input in
                    Text(verbatim: "#\(input.index)   \(input.amount) BTC   \(input.owner)")
                }
            } header: {
                Text(verbatim: "Inputs (\(review.inputs.count))")
            }
            Section {
                ForEach(review.outputs, id: \.index) { output in
                    outputRow(output)
                }
            } header: {
                Text(verbatim: "Outputs (\(review.outputs.count))")
            }
            Section {
                LabeledContent("Total in", value: "\(review.totalIn) BTC")
                LabeledContent("Total out", value: "\(review.totalOut) BTC")
                LabeledContent("Fee", value: "\(review.fee) BTC")
                LabeledContent("Leaving your wallet", value: "\(review.sending) BTC")
            } header: {
                Text(verbatim: "Totals")
            }
            if !review.warnings.isEmpty {
                Section {
                    ForEach(review.warnings, id: \.self) { warning in
                        Text(verbatim: warning)
                            .foregroundStyle(Color.red)
                    }
                } header: {
                    Text(verbatim: "Warnings")
                }
            }
            Section {
                if review.inputsToSign == 0 {
                    Text(verbatim: "This wallet has nothing to sign in this transaction. Is it the right backup, "
                        + "passphrase and network?")
                        .foregroundStyle(Color.red)
                } else {
                    Button("SIGN this transaction (\(inputsText(review.inputsToSign)))") {
                        sign()
                    }
                    .disabled(busy)
                }
                Button("Cancel: do not sign") {
                    model.back()
                }
                .disabled(busy)
                if busy {
                    HStack(spacing: 12) {
                        ProgressView()
                        Text(verbatim: "Signing ...")
                    }
                }
                if let problem {
                    Text(verbatim: problem)
                        .foregroundStyle(Color.red)
                }
            }
        }
        .navigationTitle("Check before signing")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func outputRow(_ output: OutputLine) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(verbatim: "#\(output.index)   \(output.amount) BTC   to:")
            Text(verbatim: output.address)
                .font(.system(.body, design: .monospaced))
            if let onChain = output.silentPayment {
                Text(verbatim: "A silent payment: on the blockchain it is \(onChain), which only the receiver can tell "
                    + "is theirs.")
                    .font(.footnote)
            }
            Text(verbatim: output.note)
                .font(output.external ? .body : .footnote)
                .foregroundStyle(output.external ? Color.red : Color.secondary)
        }
    }

    private func sign() {
        guard let wallet = signing.wallet, !busy else { return }
        busy = true
        problem = nil
        let transaction = signing.transaction
        let started = model.workStarted()
        inBackground {
            try wallet.sign(transaction: transaction, multisig: [])
        } done: { result in
            busy = false
            guard model.workEnded(started: started) else { return }
            switch result {
            case .success(let signed):
                // the review closes, and with it the wallet
                model.showInstead(.signed(SignedShown(signed)))
            case .failure(let error):
                problem = "Not signed: \(userText(error))"
            }
        }
    }
}

/// A signed transaction, to give back to Sparrow: it holds no secret.
final class SignedShown: Hashable {
    let signed: Signed

    init(_ signed: Signed) {
        self.signed = signed
    }

    static func == (a: SignedShown, b: SignedShown) -> Bool {
        a === b
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(ObjectIdentifier(self))
    }
}

/// The signed transaction as an animated QR code for Sparrow, at full brightness, with the choice of a file instead.
struct SignedScreen: View {
    let model: AppModel
    let shown: SignedShown
    @State private var pictures: Result<[UIImage], Error>?
    @State private var export: ExportFile?
    @State private var saved: String?

    var body: some View {
        let signed = shown.signed
        List {
            if let multisig = signed.multisig {
                Section {
                    Text(verbatim: multisig)
                        .foregroundStyle(Color.orange)
                }
            }
            Section {
                Text(verbatim: "Signed \(inputsText(signed.inputs)). In Sparrow, on the transaction's screen, click "
                    + "Scan QR and hold this iPhone's screen toward the computer's camera. The code changes all the "
                    + "time: that is normal.")
                    .font(.footnote)
                switch pictures {
                case .success(let codes):
                    AnimatedQrView(pictures: codes)
                    if codes.count > 1 {
                        Text(verbatim: "\(codes.count) QR codes, shown one after the other.")
                            .font(.footnote)
                    }
                case .failure(let error):
                    Text(verbatim: "The QR code cannot be made: \(userText(error))")
                        .foregroundStyle(Color.red)
                case nil:
                    ProgressView()
                }
            }
            Section {
                Button("Save it as a file instead (.psbt)") {
                    let transaction = signed.transaction
                    export = ExportFile(
                        data: transaction.toBytes(), type: .data, name: signedFileName(txid: transaction.txid())
                    )
                }
                if let saved {
                    Text(verbatim: saved)
                        .font(.footnote)
                }
            } footer: {
                Text(verbatim: "In Sparrow: File > Open Transaction, then Broadcast. " + ExportFile.placeHint)
            }
            Section {
                Button("Done") {
                    model.back()
                }
            }
        }
        .navigationTitle("Signed")
        .navigationBarTitleDisplayMode(.inline)
        .exportsFile($export, saved: $saved)
        .onAppear {
            if pictures == nil {
                pictures = Result {
                    try qrAnimation(frames: signed.transaction.qrFrames()).map { qrPicture($0, scale: 1) }
                }
            }
            Brightness.qrShowing(true)
        }
        .onDisappear {
            Brightness.qrShowing(false)
        }
    }
}

/// An animated QR code: `pictures` one after the other, one every 300 ms, in a loop (as the Android app and the
/// command-line program show a signed transaction).
struct AnimatedQrView: View {
    let pictures: [UIImage]

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.3)) { context in
            let index = Int(context.date.timeIntervalSinceReferenceDate / 0.3) % max(pictures.count, 1)
            if pictures.indices.contains(index) {
                Image(uiImage: pictures[index])
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity)
            }
        }
    }
}
