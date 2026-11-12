import Foundation

/// The multisig wallets kept on this iPhone: `.multisig` files, the same as the computer keeps in its "multisig
/// wallets" folder and the Android app keeps, each with a wallet's descriptor as Sparrow gives it. They hold no secret,
/// only the wallets' public keys (anyone who reads one can see all the wallet's addresses), and are kept as the backups
/// are: in SeedLock's own storage, encrypted while the iPhone is locked, and left out of the iPhone's backups.
final class MultisigWallets {
    static let ext = ".multisig"

    private let dir: URL

    /// The multisig wallets in SeedLock's own storage.
    convenience init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        self.init(dir: support.appending(path: "multisig", directoryHint: .isDirectory))
    }

    /// The multisig wallets in the folder `dir` (ios/swift-check uses one of its own).
    init(dir: URL) {
        self.dir = dir
    }

    /// The names, in alphabetical order.
    func names() -> [String] {
        let files = (try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        return files.map { $0.lastPathComponent }
            .filter { $0.hasSuffix(MultisigWallets.ext) && $0.count > MultisigWallets.ext.count }
            .map { String($0.dropLast(MultisigWallets.ext.count)) }
            .sorted { $0.caseInsensitiveCompare($1) == .orderedAscending }
    }

    /// The contents of the wallet's file.
    func read(_ name: String) throws -> String {
        let data = try readFile(file(name), limit: 1_000_000, what: "a multisig wallet")
        guard let text = String(data: data, encoding: .utf8) else {
            throw UserError("This multisig wallet's file is not text.")
        }
        return text
    }

    /// The wallet kept under `name`; the error says why its file cannot be read.
    func wallet(_ name: String) throws -> MultisigWallet {
        try MultisigWallet.fromText(text: read(name))
    }

    /// Every multisig wallet on this iPhone that can be read: what checking and signing a transaction need.
    func all() -> [RegisteredMultisig] {
        names().compactMap { name in
            (try? wallet(name)).map { RegisteredMultisig(name: name, wallet: $0) }
        }
    }

    /// The wallet on this iPhone that is this very wallet (the same kind, threshold and keys), if there is one.
    func same(_ wallet: MultisigWallet) -> String? {
        all().first { $0.wallet.sameAs(other: wallet) }?.name
    }

    /// The wallet that already uses this name, ignoring upper and lower case (as for the backups).
    func taken(_ name: String) -> String? {
        names().first { $0.caseInsensitiveCompare(name) == .orderedSame }
    }

    /// Saves a new multisig wallet. Never replaces one: the name must be free.
    func save(_ name: String, contents: String) throws {
        if let taken = taken(name) {
            throw UserError("There is already a multisig wallet named \"\(taken)\" on this iPhone.")
        }
        try FileManager.default.createDirectory(
            at: dir, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.complete]
        )
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        var folder = dir
        try folder.setResourceValues(values)
        try Data(contents.utf8).write(to: file(name), options: [.atomic, .completeFileProtection])
    }

    func delete(_ name: String) {
        try? FileManager.default.removeItem(at: file(name))
    }

    private func file(_ name: String) -> URL {
        dir.appending(path: name + MultisigWallets.ext, directoryHint: .notDirectory)
    }
}
