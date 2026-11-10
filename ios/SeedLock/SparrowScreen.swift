import SwiftUI

/// A backup's wallet laid out for Sparrow (the command-line program's menu 5): one QR code to scan, which also names
/// the wallet's keystore after the backup, or three fields to type; and the first addresses to compare. Or the seed's
/// silent payment wallet. Both can be saved as files: the QR code as a picture, and all of it as text.
///
/// The screen holds the wallet's master key while it is open (`OpenWallet`), and shows only what is public. The
/// screen is at full brightness meanwhile, for Sparrow's camera (`Brightness`).
struct SparrowScreen: View {
    let model: AppModel
    let open: OpenWallet
    /// The backup's name.
    let name: String
    @State private var type: AddressType = .nativeSegwit
    @State private var silent = false
    @State private var export: ExportFile?
    @State private var saved: String?

    var body: some View {
        List {
            // read here, and never kept by the screen: closing the screen lets go of the wallet (OpenWallet.close)
            if let wallet = open.wallet {
                Section {
                    Picker("Wallet", selection: $silent) {
                        Text(verbatim: "Usual wallet").tag(false)
                        Text(verbatim: "Silent payments").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    if !silent {
                        Picker("Script Type", selection: $type) {
                            ForEach(addressTypes, id: \.self) { kind in
                                Text(verbatim: sparrowName(kind)).tag(kind)
                            }
                        }
                    }
                } footer: {
                    if silent {
                        Text(verbatim: "One address that you give to everyone, and nobody else can tell which payments "
                            + "on the blockchain went to it. In Sparrow 2.5 or later.")
                    } else {
                        Text(verbatim: "Native Segwit is the usual address type: choose another one only if your "
                            + "wallet in Sparrow uses another Script Type.")
                    }
                }
                if silent {
                    silentWallet(Result { try wallet.silentPayments(name: name) })
                } else {
                    usualWallet(Result { try wallet.sparrow(addressType: type, name: name) })
                }
            }
        }
        .navigationTitle("Wallet for Sparrow")
        .navigationBarTitleDisplayMode(.inline)
        .exportsFile($export, saved: $saved)
        .onChange(of: silent) {
            // what was saved is about the wallet shown before
            saved = nil
        }
        .onChange(of: type) {
            saved = nil
        }
        .onAppear {
            Brightness.qrShowing(true)
        }
        .onDisappear {
            Brightness.qrShowing(false)
        }
    }

    // MARK: The usual wallet

    @ViewBuilder
    private func usualWallet(_ made: Result<SparrowWallet, Error>) -> some View {
        switch made {
        case .success(let s):
            Section {
                Text(verbatim: "Wallet \(s.fingerprint): \(s.addressType), \(s.network).")
                if s.passphraseUsed {
                    Text(verbatim: "With its BIP39 passphrase: a different wallet from the one without it.")
                        .font(.footnote)
                }
            }
            Section {
                Text(verbatim: "In Sparrow: File > Import Wallet, then Output Descriptor and Scan..., and hold this "
                    + "iPhone's screen toward the computer's camera. Sparrow names the wallet's keystore "
                    + "\"\(s.label)\". When it asks for the wallet's name, type \"\(name)\" there too.")
                    .font(.footnote)
                QrCodeView(text: s.qr)
            } header: {
                Text(verbatim: "In one step: scan this QR code")
            }
            Section {
                Text(verbatim: s.descriptor)
                    .font(.system(.footnote, design: .monospaced))
            } header: {
                Text(verbatim: "The same, as text (an output descriptor)")
            }
            Section {
                Text(verbatim: "In Sparrow: File > New Wallet, Script Type \"\(s.addressType)\", Keystore \"Watch Only "
                    + "Wallet\":")
                    .font(.footnote)
                field("Master fingerprint", s.fingerprint)
                field("Derivation", s.derivation)
                field("xpub", s.xpub)
            } header: {
                Text(verbatim: "Or type the three fields")
            }
            Section {
                Text(verbatim: "Sparrow must show exactly these.")
                    .font(.footnote)
                ForEach(s.firstAddresses, id: \.path) { line in
                    field(line.path, line.address)
                }
            } header: {
                Text(verbatim: "First receive addresses")
            }
            saveSection(
                picture: {
                    try qrPictureFile(
                        s.qr, name: sparrowFileName(name, silent: false, picture: true),
                        caption: ["\(name): wallet for Sparrow", "\(s.fingerprint), \(s.addressType)"]
                    )
                },
                text: sparrowFile(s, name: name),
                silent: false
            )
        case .failure(let error):
            problem(error)
        }
    }

    // MARK: The silent payment wallet

    @ViewBuilder
    private func silentWallet(_ made: Result<SilentPaymentWallet, Error>) -> some View {
        switch made {
        case .success(let s):
            Section {
                Text(verbatim: "Silent payment wallet \(s.fingerprint), \(s.network).")
                if s.passphraseUsed {
                    Text(verbatim: "With its BIP39 passphrase: a different wallet from the one without it.")
                        .font(.footnote)
                }
            }
            Section {
                Text(verbatim: "Give it to anyone who pays you, as often as you like: each payment gets an address of "
                    + "its own on the blockchain, which nobody else can link to this one or to the other payments.")
                    .font(.footnote)
                Text(verbatim: s.address)
                    .font(.system(.body, design: .monospaced))
            } header: {
                Text(verbatim: "Your silent payment address")
            }
            Section {
                Text(verbatim: "In Sparrow 2.5 or later: File > Import Wallet, then Output Descriptor and Scan..., and "
                    + "hold this iPhone's screen toward the computer's camera.")
                    .font(.footnote)
                QrCodeView(text: s.descriptor)
            } header: {
                Text(verbatim: "In one step: scan this QR code")
            }
            Section {
                Text(verbatim: s.descriptor)
                    .font(.system(.footnote, design: .monospaced))
            } header: {
                Text(verbatim: "The same, as text (an output descriptor)")
            }
            Section {
                Text(verbatim: "In Sparrow: File > New Wallet, Policy Type \"\(sparrowSilentPolicy)\", Keystore "
                    + "\"Watch Only Wallet\":")
                    .font(.footnote)
                field("Master fingerprint", s.fingerprint)
                field("Derivation", s.derivation)
                field(s.spscanName, s.spscan)
            } header: {
                Text(verbatim: "Or type the fields")
            } footer: {
                Text(verbatim: "When Sparrow asks for the wallet's name, type \"\(name)\". Sparrow calls the keystore "
                    + "\"Keystore 1\": it takes no name from a silent payment wallet's descriptor. To give it the "
                    + "backup's name too: the wallet's Settings tab, the keystore's Label field, \"\(s.label)\" "
                    + "(Sparrow allows 16 characters), then Apply.")
            }
            Section {
                Text(verbatim: "With this descriptor Sparrow FINDS the payments to your address. It cannot spend them: "
                    + "that takes your seed, which stays in its encrypted backup. But whoever has it sees every "
                    + "payment you receive. To find them, Sparrow must be connected to a server that supports silent "
                    + "payments (not to Bitcoin Core), and it asks from which date to look: the day you first gave out "
                    + "the address. To spend them, Sparrow sends the transaction to SeedLock to be signed, as for any "
                    + "other wallet.")
                    .font(.footnote)
            } header: {
                Text(verbatim: "Keep it private")
            }
            saveSection(
                picture: {
                    try qrPictureFile(
                        s.descriptor, name: sparrowFileName(name, silent: true, picture: true),
                        caption: ["\(name): silent payments for Sparrow", "\(s.fingerprint): keep it private"]
                    )
                },
                text: silentPaymentFile(s, name: name),
                silent: true
            )
        case .failure(let error):
            problem(error)
        }
    }

    // MARK: Parts

    /// Saving the wallet as files: its QR code as a picture, and all of it as text.
    @ViewBuilder
    private func saveSection(picture: @escaping () throws -> ExportFile, text: String, silent: Bool) -> some View {
        Section {
            Button("Save the QR code as a picture (PNG)") {
                do {
                    export = try picture()
                } catch {
                    saved = "Not saved: \(userText(error))"
                }
            }
            Button("Save all of it as a text file (.txt)") {
                export = ExportFile(
                    data: Data(text.utf8), type: .plainText, name: sparrowFileName(name, silent: silent, picture: false)
                )
            }
            if let saved {
                Text(verbatim: saved)
                    .font(.footnote)
            }
        } header: {
            Text(verbatim: "Save it as files")
        } footer: {
            Text(verbatim: (silent
                ? "Neither file can spend anything, but whoever reads them can see every payment you receive. "
                : "Neither file holds a secret, but whoever reads them can see all your addresses. ")
                + ExportFile.placeHint)
        }
    }

    /// A value to read, or to type into Sparrow, under its name.
    private func field(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(verbatim: label)
                .font(.footnote)
                .foregroundStyle(Color.secondary)
            Text(verbatim: value)
                .font(.system(.body, design: .monospaced))
        }
    }

    private func problem(_ error: Error) -> some View {
        Section {
            Text(verbatim: userText(error))
                .foregroundStyle(Color.red)
        }
    }
}
