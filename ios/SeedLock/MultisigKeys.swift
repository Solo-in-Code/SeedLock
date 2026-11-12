import Foundation

/// The kinds of multisig wallet, in the order SeedLock lists them: the usual one first.
var multisigKinds: [MultisigKind] {
    [.nativeSegwit, .nestedSegwit, .legacy]
}

/// A seed's key for a multisig wallet, for real bitcoin and for testing (the same key, written as an xpub or a tpub).
struct CosignerKeys {
    let mainnet: CosignerKey
    let testing: CosignerKey
}

/// What a backup's seed is for multisig (the command-line program's menu 10, 1): a cosigner of a multisig wallet on
/// this iPhone, or not yet.
enum Cosigning {
    /// A cosigner of the multisig wallet `wallet` on this iPhone, at `position` (1 for the first): its key there, with
    /// that wallet's kind and network.
    case known(wallet: String, position: Int32, key: CosignerKey)
    /// Not a cosigner of a wallet on this iPhone yet: its key for each kind of multisig wallet.
    case choose([MultisigKind: CosignerKeys])
}

/// The keys of `wallet`'s seed for a multisig wallet. `name`: the backup's, which Sparrow takes as the keystore's
/// name. Public keys only: the wallet can be closed as soon as they are known.
func cosigning(_ wallet: Wallet, backup name: String, registered: [RegisteredMultisig]) throws -> Cosigning {
    let fingerprint = wallet.fingerprint()
    for multisig in registered {
        if let position = multisig.wallet.positionOf(fingerprint: fingerprint) {
            let key = try wallet.cosignerKeyIn(wallet: multisig.wallet, name: name)
            return .known(wallet: multisig.name, position: position, key: key)
        }
    }
    var keys: [MultisigKind: CosignerKeys] = [:]
    for kind in multisigKinds {
        keys[kind] = CosignerKeys(
            mainnet: try wallet.cosignerKey(kind: kind, mainnet: true, name: name),
            testing: try wallet.cosignerKey(kind: kind, mainnet: false, name: name)
        )
    }
    return .choose(keys)
}

/// The backups on this iPhone that are cosigners of `wallet`, as (position, backup name), in its order (1 for the
/// first); a backup's fingerprint is known once it was opened here.
func cosignersHere(_ wallet: MultisigWallet, backups: Backups) -> [(position: Int, backup: String)] {
    wallet.cosigners().enumerated().flatMap { index, cosigner in
        backups.names(withFingerprintIn: [cosigner.fingerprint]).map { (position: index + 1, backup: $0) }
    }
}
