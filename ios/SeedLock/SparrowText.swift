import Foundation

/// The address types of a wallet for Sparrow, in the order SeedLock lists them: the usual one first.
var addressTypes: [AddressType] {
    [.nativeSegwit, .taproot, .nestedSegwit, .legacy]
}

/// Sparrow's name for an address type (its "Script Type").
func sparrowName(_ type: AddressType) -> String {
    switch type {
    case .nativeSegwit:
        return "Native Segwit (P2WPKH)"
    case .taproot:
        return "Taproot (P2TR)"
    case .nestedSegwit:
        return "Nested Segwit (P2SH-P2WPKH)"
    case .legacy:
        return "Legacy (P2PKH)"
    }
}

/// How Sparrow calls a silent payment wallet (its Policy Type).
let sparrowSilentPolicy = "Single Signature SP (Silent Payments)"

/// The names of the files that hold a wallet for Sparrow, as the command-line program names them (without their
/// extension: the file picker adds it).
func sparrowFileName(_ name: String, silent: Bool, picture: Bool) -> String {
    "\(name)\(silent ? "-silent-payments" : "")-for-sparrow\(picture ? "-qr" : "")"
}

/// `text`, with spaces after it up to `width` characters.
private func padded(_ text: String, _ width: Int) -> String {
    text.count >= width ? text : text + String(repeating: " ", count: width - text.count)
}

/// The wallet laid out for Sparrow as a text file: what the command-line program saves as `<name>-for-sparrow.txt`,
/// said for the iPhone. `name`: the backup's. It holds no secret, but whoever reads it can see all the wallet's
/// addresses.
func sparrowFile(_ s: SparrowWallet, name: String) -> String {
    let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
    let shortened = !trimmed.isEmpty && s.label != trimmed ? " (Sparrow allows 16 characters)" : ""
    var lines = ["Wallet \(s.fingerprint) for Sparrow: \(s.addressType), \(s.network)"]
    lines.append(s.addressType == sparrowName(.nativeSegwit)
        ? "(Native Segwit, the usual address type; the others are listed below)"
        : "(the address type chosen in SeedLock; the others are listed below)")
    if s.passphraseUsed {
        lines.append("With its BIP39 passphrase: a different wallet from the one without it.")
    }
    lines += [
        "",
        "In Sparrow: File > New Wallet, Script Type \"\(s.addressType)\", Keystore \"Watch Only Wallet\":",
        "  Master fingerprint : \(s.fingerprint)",
        "  Derivation         : \(s.derivation)",
        "  xpub               : \(s.xpub)",
        "",
        "Or in one step: scan the wallet's QR code (File > Import Wallet, Output Descriptor, Scan...), which also",
        "names its keystore \"\(s.label)\"\(shortened); when Sparrow asks for the wallet's name, type "
            + "\"\(name)\" there too.",
        "Or paste this output descriptor (File > New Wallet, then on the Settings tab, next to Descriptor, Edit...):",
        "  \(s.descriptor)",
        "",
        "The same wallet with the other address types (if you choose another Script Type in Sparrow):",
    ]
    for other in s.otherTypes {
        lines.append("  \(padded(other.name, 28)) \(padded(other.derivation, 14)) \(other.xpub)")
    }
    lines += ["", "First receive addresses (Sparrow must show exactly these):"]
    for line in s.firstAddresses {
        lines.append("  \(padded(line.path, 20)) \(line.address)")
    }
    lines += ["", "As a QR code (UR crypto-output):", "  \(s.qr)"]
    return lines.joined(separator: "\n") + "\n"
}

/// The silent payment wallet laid out for Sparrow as a text file: what the command-line program saves as
/// `<name>-silent-payments-for-sparrow.txt`, said for the iPhone. It cannot spend anything, but whoever reads it can
/// see every payment the wallet receives.
func silentPaymentFile(_ s: SilentPaymentWallet, name: String) -> String {
    var lines = ["Silent payment wallet \(s.fingerprint) for Sparrow, \(s.network)"]
    if s.passphraseUsed {
        lines.append("With its BIP39 passphrase: a different wallet from the one without it.")
    }
    lines += [
        "",
        "Your silent payment address. Give it to anyone who pays you, as often as you like: each payment gets",
        "an address of its own on the blockchain, which nobody else can link to this one or to the other payments.",
        "  \(s.address)",
        "",
        "In Sparrow 2.5 or later: File > Import Wallet, Output Descriptor, then Scan... (its QR code) or paste:",
        "  \(s.descriptor)",
        "Or File > New Wallet, Policy Type \"\(sparrowSilentPolicy)\", Keystore \"Watch Only Wallet\":",
        "  Master fingerprint : \(s.fingerprint)",
        "  Derivation         : \(s.derivation)",
        "  \(padded(s.spscanName, 18)) : \(s.spscan)",
        "When Sparrow asks for the wallet's name, type \"\(name)\". Sparrow calls the keystore \"Keystore 1\": it "
            + "takes no",
        "name from a silent payment wallet's descriptor. To give it the backup's name too: the wallet's Settings tab,",
        "the keystore's Label field, \"\(s.label)\" (Sparrow allows 16 characters), then Apply.",
        "",
        "With this descriptor (its \(s.spscanName) key) Sparrow FINDS the payments to your address. It cannot spend "
            + "them:",
        "that takes your seed, which stays in its encrypted backup. Keep it private all the same: whoever has it sees",
        "every payment you receive. To find them, Sparrow must be connected to a server that supports silent payments",
        "(not to Bitcoin Core), and it asks from which date to look: the day you first gave out the address.",
        "To spend them, Sparrow sends the transaction to SeedLock to be signed, as for any other wallet.",
    ]
    return lines.joined(separator: "\n") + "\n"
}
