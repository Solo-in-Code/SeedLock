import Foundation

/// "1 input", "2 inputs".
func inputsText(_ count: Int32) -> String {
    count == 1 ? "1 input" : "\(count) inputs"
}

/// The name of the file that holds a signed transaction, from its id, as the Android app names it.
func signedFileName(txid: String) -> String {
    "signed-\(txid.prefix(8)).psbt"
}

/// Why a backup may not be the one to sign a transaction: the wallets that the transaction says sign it (`signers`,
/// their fingerprints), the backup's own (`ours`), and the backups on this iPhone that are those wallets (`here`).
func otherBackupText(signers: [String], ours: String, here: [String]) -> String {
    let wallets = signers.count == 1 ? "wallet" : "wallets"
    var text = "This transaction is for the \(wallets) \(signers.joined(separator: ", ")). This backup is the wallet "
        + "\(ours), so it cannot sign it, unless you use it with a BIP39 passphrase (then it is another wallet)."
    if !here.isEmpty {
        text += "\n\nThe backup for it on this iPhone: \(here.map { "\"\($0)\"" }.joined(separator: " or "))."
    }
    return text
}
