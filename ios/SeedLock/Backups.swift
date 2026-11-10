import Foundation

/// The backups kept on this iPhone: encrypted `.seedlock` files, the same as the computer and the Android app write,
/// in SeedLock's own storage, which other apps cannot read. iOS keeps them encrypted while the iPhone is locked (file
/// protection "complete"), and they are left out of the iPhone's backups (to iCloud or to a computer), which also
/// carry an iPhone's files over to a new one: the backups stay where you put them. Next to each, once the backup was
/// opened on this iPhone, its wallet's fingerprint without a BIP39 passphrase (`<name>.fingerprint`), to show it
/// beside the backup's name. Nothing about a passphrase is ever kept: with one, a seed is another wallet, whose
/// existence stays the person's secret.
final class Backups {
    static let ext = ".seedlock"
    private static let fingerprintExt = ".fingerprint"

    private let dir: URL

    /// The backups in SeedLock's own storage.
    convenience init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        self.init(dir: support.appending(path: "backups", directoryHint: .isDirectory))
    }

    /// The backups in the folder `dir` (ios/swift-check uses one of its own).
    init(dir: URL) {
        self.dir = dir
    }

    /// The names, in alphabetical order.
    func names() -> [String] {
        let files = (try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        return files.map { $0.lastPathComponent }
            .filter { $0.hasSuffix(Backups.ext) && $0.count > Backups.ext.count }
            .map { String($0.dropLast(Backups.ext.count)) }
            .sorted { $0.caseInsensitiveCompare($1) == .orderedAscending }
    }

    /// The backup's text: the contents of its file.
    func read(_ name: String) throws -> String {
        let data = try readFile(file(name), limit: 64 * 1024, what: "a SeedLock backup")
        guard let text = String(data: data, encoding: .utf8) else {
            throw UserError("This backup's file is not text.")
        }
        return text
    }

    /// The backup that already uses this name, ignoring upper and lower case (computers, USB sticks and cloud folders
    /// treat "Wallet" and "wallet" as one file).
    func taken(_ name: String) -> String? {
        names().first { $0.caseInsensitiveCompare(name) == .orderedSame }
    }

    /// Saves a new backup. Never replaces one: the name must be free.
    func save(_ name: String, contents: String) throws {
        if let taken = taken(name) {
            throw UserError("There is already a backup named \"\(taken)\" on this iPhone.")
        }
        try makeFolder()
        try Data(contents.utf8).write(to: file(name), options: [.atomic, .completeFileProtection])
        // a new backup: no fingerprint known yet
        try? FileManager.default.removeItem(at: fingerprintFile(name))
    }

    func delete(_ name: String) {
        try? FileManager.default.removeItem(at: fingerprintFile(name))
        try? FileManager.default.removeItem(at: file(name))
    }

    /// The wallet fingerprint of the backup `name` (without a passphrase), once it is known on this iPhone.
    func fingerprint(_ name: String) -> String? {
        guard let data = try? readFile(fingerprintFile(name), limit: 64, what: "a wallet fingerprint"),
              let text = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
              Backups.isFingerprint(text)
        else { return nil }
        return text
    }

    /// Remembers the wallet fingerprint (without a passphrase) of the backup `name`.
    func rememberFingerprint(_ name: String, _ fingerprint: String) {
        guard Backups.isFingerprint(fingerprint), self.fingerprint(name) != fingerprint,
              FileManager.default.fileExists(atPath: file(name).path(percentEncoded: false))
        else { return }
        // only a convenience: without it, the backup simply shows no fingerprint
        try? Data(fingerprint.utf8).write(to: fingerprintFile(name), options: [.atomic, .completeFileProtection])
    }

    /// The backup on this iPhone that is this very backup (the same backup line), if there is one.
    func same(_ text: String) -> String? {
        guard let wanted = try? backupText(text: text) else { return nil }
        return names().first { name in
            guard let other = try? read(name), let line = try? backupText(text: other) else { return false }
            return line == wanted
        }
    }

    private static func isFingerprint(_ text: String) -> Bool {
        text.count == 8 && text.allSatisfy { "0123456789abcdef".contains($0) }
    }

    private func file(_ name: String) -> URL {
        dir.appending(path: name + Backups.ext, directoryHint: .notDirectory)
    }

    private func fingerprintFile(_ name: String) -> URL {
        dir.appending(path: name + Backups.fingerprintExt, directoryHint: .notDirectory)
    }

    /// The folder of the backups, made the first time: encrypted while the iPhone is locked, and left out of the
    /// iPhone's backups.
    private func makeFolder() throws {
        try FileManager.default.createDirectory(
            at: dir, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.complete]
        )
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        var folder = dir
        try folder.setResourceValues(values)
    }
}
