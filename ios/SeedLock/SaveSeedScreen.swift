import SwiftUI

/// Encrypts a seed into a backup with a password and saves it on this iPhone, then reads the saved file back and opens
/// it with the password: proof that what is stored opens (`saveNewBackup`). The password: a strong one that SeedLock
/// makes (8 random words), to keep in a password manager, or one's own, typed twice.
///
/// iOS lets an app offer a password to a password manager only through the clipboard (saving it by AutoFill needs a
/// web site of the app's own), so the new password is copied there: for this iPhone only, and taken off again once
/// SeedLock is back in front. Then the password manager fills it back in here, by AutoFill, without the clipboard.
struct SaveSeedScreen: View {
    let model: AppModel
    let open: OpenSeed
    @State private var name = ""
    @State private var generated: String?
    @State private var password = ""
    @State private var again = ""
    @State private var light = false
    @State private var problem: String?
    @State private var note: String?
    @State private var busy = false

    var body: some View {
        List {
            // read here, and never kept by the screen: closing the screen lets go of the seed (OpenSeed.close)
            if let seed = open.seed {
                Section {
                    LabeledContent("Wallet fingerprint", value: (try? seed.fingerprint()) ?? "unknown")
                } footer: {
                    Text(verbatim: "The seed is now encrypted with a password into a backup. Keep the password in your "
                        + "password manager (on your everyday phone or computer): without it, the backup can never be "
                        + "opened.")
                }
                Section {
                    TextField("Name of the backup", text: $name)
                        .autocorrectionDisabled(true)
                        .textInputAutocapitalization(.never)
                } header: {
                    Text(verbatim: "Name")
                }
                if let generated {
                    generatedPassword(generated)
                } else {
                    ownPassword
                }
                Section {
                    Toggle("For an old phone with little memory", isOn: $light)
                } footer: {
                    Text(verbatim: "Protects the backup with 64 MiB instead of 256 MiB, so that it opens everywhere, "
                        + "an old phone included.")
                }
                if busy {
                    Section {
                        HStack(spacing: 12) {
                            ProgressView()
                            Text(verbatim: "Encrypting with Argon2id (\(light ? 64 : 256) MiB), saving, and opening "
                                + "the saved backup again to check it ...")
                        }
                    }
                }
                if let problem {
                    Section {
                        Text(verbatim: problem)
                            .foregroundStyle(Color.red)
                    }
                }
                Section {
                    Button("Encrypt and save") {
                        save()
                    }
                    .disabled(busy)
                }
            }
        }
        .navigationTitle("Save the backup")
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: password + again) { old, new in
            if new.count > old.count + 1, let said = model.pastedSecret() {
                note = said
            }
        }
    }

    @ViewBuilder
    private func generatedPassword(_ shown: String) -> some View {
        Section {
            Text(verbatim: shown)
                .font(.system(.title3, design: .monospaced))
            Button("Copy it, for the password manager on this iPhone") {
                model.copySecret(shown)
                model.say(
                    "Copied",
                    "Now open your password manager, make a new entry with the backup's name, paste the password "
                        + "into it and save it. Then come back to SeedLock within \(AppModel.keepOpenMinutes) minutes "
                        + "(this screen stays open meanwhile) and let the password manager fill it in here. Back in "
                        + "SeedLock, its copy leaves the clipboard."
                )
            }
        } header: {
            Text(verbatim: "Your new password (8 random words)")
        } footer: {
            Text(verbatim: "Save it in your password manager now, exactly as shown. A password manager on this iPhone "
                + "must be able to save while the iPhone is offline: Apple's Passwords app can, some others cannot "
                + "(Bitwarden, for one). Then, or for a password manager on another phone or computer, type the 8 "
                + "words into it by hand.")
        }
        Section {
            SecureField("The password", text: $password)
                .textContentType(.password)
                .autocorrectionDisabled(true)
                .textInputAutocapitalization(.never)
                .disabled(busy)
            pasteNote
        } header: {
            Text(verbatim: "Then, from the password manager")
        } footer: {
            Text(verbatim: "Let it fill the password in here (AutoFill, above the keyboard), or copy it there and "
                + "paste it here: this proves that it was saved correctly.")
        }
    }

    @ViewBuilder
    private var ownPassword: some View {
        Section {
            Button("Generate a strong password for me") {
                generated = try? generatePassword()
                model.passwordOnScreen = generated != nil
                password = ""
                again = ""
            }
        }
        Section {
            SecureField("Password", text: $password)
                .textContentType(.password)
                .autocorrectionDisabled(true)
                .textInputAutocapitalization(.never)
                .disabled(busy)
            SecureField("The same password again", text: $again)
                .textContentType(.password)
                .autocorrectionDisabled(true)
                .textInputAutocapitalization(.never)
                .disabled(busy)
            pasteNote
        } header: {
            Text(verbatim: "Or type your own (at least \(minPasswordLength()) characters)")
        }
    }

    /// What SeedLock did after a paste into a password: said right under the fields, where the eyes are (further down,
    /// the keyboard could hide it).
    @ViewBuilder
    private var pasteNote: some View {
        if let note {
            Text(verbatim: note)
                .font(.footnote)
        }
    }

    private func save() {
        guard let seed = open.seed, !busy else { return }
        problem = nil
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
        if let generated {
            if password != generated {
                password = ""
                problem = "That does not match the password shown. Check the password manager's entry and try again."
                return
            }
        } else {
            if password.count < Int(minPasswordLength()) {
                problem = "Too short: use at least \(minPasswordLength()) characters."
                return
            }
            if password != again {
                password = ""
                again = ""
                problem = "The two passwords are different. Please type them again."
                return
            }
        }
        // Swift's text cannot be wiped: the fields are emptied at once, and the bytes that go to Rust are wiped after
        let typed = password
        password = ""
        again = ""
        busy = true
        let strength: BackupStrength = light ? .light : .standard
        let backups = model.backups
        let started = model.workStarted()
        inBackground { () throws -> String in
            var secret = Data(typed.utf8)
            defer {
                secret.resetBytes(in: 0..<secret.count)
            }
            return try saveNewBackup(seed, name: clean, password: secret, strength: strength, in: backups)
        } done: { result in
            busy = false
            switch result {
            case .success(let fingerprint):
                model.newBackupSaved(clean, fingerprint: fingerprint, started: started)
            case .failure(let error):
                model.workEnded(started: started)
                problem = "Not saved: \(userText(error))"
            }
        }
    }
}
