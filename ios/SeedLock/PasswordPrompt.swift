import SwiftUI

/// Asks for a backup's password and opens the backup, away from the screen's thread; a wrong password asks again. A
/// password manager on this iPhone may fill the password in (AutoFill), or it may be copied from there and pasted.
/// To open a backup's wallet, it also asks whether the wallet uses a BIP39 passphrase (and which), and for which
/// network when that is not known yet.
struct PasswordPrompt: View {
    let model: AppModel
    let request: PasswordRequest
    @State private var password = ""
    @State private var usesPassphrase = false
    @State private var passphrase = ""
    @State private var network: Net = .mainnet
    @State private var problem: String?
    /// What SeedLock did after a paste into the password, or into the passphrase: said under that field.
    @State private var note: String?
    @State private var passphraseNote: String?
    @State private var busy = false
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                if let warning = request.note {
                    Section {
                        Text(verbatim: warning)
                            .foregroundStyle(Color.orange)
                    }
                }
                if request.opening == .wallet(nil) {
                    Section {
                        Picker("Network", selection: $network) {
                            Text(verbatim: "Real bitcoin (mainnet)").tag(Net.mainnet)
                            Text(verbatim: "Testing (testnet or signet)").tag(Net.testnet)
                        }
                        .pickerStyle(.inline)
                        .labelsHidden()
                        .disabled(busy)
                    } header: {
                        Text(verbatim: "Is this wallet for real bitcoin, or for testing?")
                    }
                }
                Section {
                    SecureField("Password", text: $password)
                        .textContentType(.password)
                        .autocorrectionDisabled(true)
                        .textInputAutocapitalization(.never)
                        .submitLabel(.go)
                        .focused($focused)
                        .disabled(busy)
                        .onSubmit {
                            open()
                        }
                    // what happens shows right under the password, where the eyes are: further down, the keyboard
                    // could hide it
                    if busy {
                        HStack(spacing: 12) {
                            ProgressView()
                            Text(verbatim: "Opening the backup (Argon2id, \(request.memoryMib) MiB): this takes a few "
                                + "seconds ...")
                        }
                    }
                    if let problem {
                        Text(verbatim: problem)
                            .foregroundStyle(Color.red)
                    }
                    if let note {
                        Text(verbatim: note)
                            .font(.footnote)
                    }
                } header: {
                    Text(verbatim: "Password of the backup \"\(request.name)\"")
                } footer: {
                    Text(verbatim: "With a password manager on this iPhone, let it fill the password in (AutoFill, "
                        + "above the keyboard) if you can: then the password never goes through the clipboard. Or copy "
                        + "it there and come back within \(AppModel.keepOpenMinutes) minutes to paste it here: "
                        + "SeedLock empties the clipboard afterwards.")
                }
                if request.opening != .seed {
                    Section {
                        Toggle("This wallet uses a BIP39 passphrase", isOn: $usesPassphrase)
                            .disabled(busy)
                        if usesPassphrase {
                            SecureField("BIP39 passphrase (upper and lower case count)", text: $passphrase)
                                .autocorrectionDisabled(true)
                                .textInputAutocapitalization(.never)
                                .submitLabel(.go)
                                .disabled(busy)
                                .onSubmit {
                                    open()
                                }
                            if let passphraseNote {
                                Text(verbatim: passphraseNote)
                                    .font(.footnote)
                            }
                        }
                    } footer: {
                        Text(verbatim: "Only if you added one to this seed. Each passphrase opens a different wallet, "
                            + "without any error, so a typo opens an empty wallet: its fingerprint shows which one "
                            + "opened.")
                    }
                }
            }
            .navigationTitle("Open the backup")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        model.closePrompt()
                    }
                    .disabled(busy)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Open") {
                        open()
                    }
                    .disabled(busy || password.isEmpty)
                }
            }
        }
        .interactiveDismissDisabled(busy)
        .onAppear {
            // the keyboard comes up once the prompt is in place; for a wallet, not at once: the questions above and
            // below the password show first
            if request.opening == .seed {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    focused = true
                }
            }
        }
        .onChange(of: password) { old, new in
            if new.count > old.count + 1, let said = model.pastedSecret() {
                note = said
            }
        }
        .onChange(of: passphrase) { old, new in
            if new.count > old.count + 1, let said = model.pastedSecret() {
                passphraseNote = said
            }
        }
    }

    private func open() {
        guard !busy, !password.isEmpty else { return }
        let wanted: Net?
        if case .wallet(let known) = request.opening {
            if usesPassphrase && passphrase.isEmpty {
                problem = "Type the wallet's BIP39 passphrase, or turn off \"This wallet uses a BIP39 passphrase\"."
                return
            }
            wanted = known ?? network
        } else {
            wanted = nil
        }
        // Swift's text cannot be wiped: the fields are emptied at once, and the bytes that go to Rust are wiped after
        let typed = password
        let phrase = usesPassphrase ? passphrase : ""
        let text = request.text
        password = ""
        passphrase = ""
        problem = nil
        busy = true
        let started = model.workStarted()
        inBackground { () throws -> (Opened, String?) in
            var secret = Data(typed.utf8)
            var extra = Data(phrase.utf8)
            defer {
                secret.resetBytes(in: 0..<secret.count)
                extra.resetBytes(in: 0..<extra.count)
            }
            let seed = try Seed.unlock(backupText: text, password: secret)
            let fingerprint = try? seed.fingerprint()
            guard let wanted else {
                return (.seed(seed), fingerprint)
            }
            // the seed is let go of when this ends: only its wallet stays open
            return (.wallet(try seed.wallet(passphrase: extra, network: wanted)), fingerprint)
        } done: { result in
            busy = false
            problem = model.unlocked(request, started: started, result: result)
            if problem != nil {
                focused = true
            }
        }
    }
}
