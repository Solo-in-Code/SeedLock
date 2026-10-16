//! A wallet opened from a seed: laid out for Sparrow, and reviewing and signing transactions. The texts
//! are the command-line program's, so that both say the same things.

use crate::error::{Result, SeedLockError};
use crate::multisig::{registered, RegisteredMultisig};
use crate::transaction::Transaction;
use seedlock_core::bitcoin::Network;
use seedlock_core::multisig::keystore_label;
use seedlock_core::psbt::{self, OutputKind};
use seedlock_core::wallet::{descriptor_checksum, path_to_display, ScriptType, Wallet as CoreWallet};
use std::sync::Arc;

/// The network a wallet is for.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Net {
    /// Real bitcoin.
    Mainnet,
    Testnet,
    Signet,
    Regtest,
}

impl Net {
    pub(crate) fn network(self) -> Network {
        match self {
            Net::Mainnet => Network::Bitcoin,
            Net::Testnet => Network::Testnet,
            Net::Signet => Network::Signet,
            Net::Regtest => Network::Regtest,
        }
    }

    fn of(network: Network) -> Net {
        match network {
            Network::Bitcoin => Net::Mainnet,
            Network::Signet => Net::Signet,
            Network::Regtest => Net::Regtest,
            _ => Net::Testnet,
        }
    }
}

fn network_name(network: Network) -> &'static str {
    match network {
        Network::Bitcoin => "mainnet",
        Network::Testnet => "testnet",
        Network::Signet => "signet",
        Network::Regtest => "regtest",
        _ => "other",
    }
}

/// The kind of addresses a wallet uses (Sparrow's "Script Type").
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressType {
    /// bc1q... addresses, the usual kind today.
    NativeSegwit,
    /// bc1p... addresses.
    Taproot,
    /// 3... addresses.
    NestedSegwit,
    /// 1... addresses.
    Legacy,
}

impl AddressType {
    const ALL: [AddressType; 4] = [AddressType::NativeSegwit, AddressType::Taproot, AddressType::NestedSegwit, AddressType::Legacy];

    fn script(self) -> ScriptType {
        match self {
            AddressType::NativeSegwit => ScriptType::Wpkh,
            AddressType::Taproot => ScriptType::Tr,
            AddressType::NestedSegwit => ScriptType::ShWpkh,
            AddressType::Legacy => ScriptType::Pkh,
        }
    }

    /// Its name in Sparrow.
    fn sparrow_name(self) -> &'static str {
        match self {
            AddressType::NativeSegwit => "Native Segwit (P2WPKH)",
            AddressType::NestedSegwit => "Nested Segwit (P2SH-P2WPKH)",
            AddressType::Taproot => "Taproot (P2TR)",
            AddressType::Legacy => "Legacy (P2PKH)",
        }
    }
}

/// One address with its key path.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AddressLine {
    pub path: String,
    pub address: String,
}

/// The same wallet with another address type.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OtherAddressType {
    /// Sparrow's name for it.
    pub name: String,
    pub derivation: String,
    pub xpub: String,
}

/// A wallet laid out for Sparrow, as the command-line program's menu 5 shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SparrowWallet {
    pub fingerprint: String,
    /// Sparrow's name for the address type (its "Script Type").
    pub address_type: String,
    /// "mainnet (real bitcoin)", "a test network (testnet or signet)" or "regtest".
    pub network: String,
    /// Whether a BIP39 passphrase was used (a different wallet from the one without it).
    pub passphrase_used: bool,
    /// The derivation, written the way Sparrow wants it (m/84'/0'/0').
    pub derivation: String,
    pub xpub: String,
    /// The whole wallet in one line (BIP380), to type or copy into Sparrow (File > Import Wallet > Output
    /// Descriptor).
    pub descriptor: String,
    /// The text of the QR code for Sparrow's File > Import Wallet > Output Descriptor > Scan...: the same
    /// wallet as a UR crypto-output that also carries [`SparrowWallet::label`].
    pub qr: String,
    /// The label Sparrow gives the wallet's keystore when it scans [`SparrowWallet::qr`]: the backup's
    /// name, cut to the 16 characters Sparrow allows.
    pub label: String,
    /// The first five receive addresses: Sparrow must show exactly these.
    pub first_addresses: Vec<AddressLine>,
    /// The same wallet with the other address types.
    pub other_types: Vec<OtherAddressType>,
}

/// One input of a transaction, as shown before signing.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct InputLine {
    pub index: i32,
    /// In BTC, with 8 decimals.
    pub amount: String,
    /// Whether it is this wallet's (verified from the seed).
    pub mine: bool,
    /// "yours (m/84'/0'/0'/0/3)", "yours, from the multisig wallet "vault" (m/48'/0'/0'/2'/0/3)" or "NOT yours".
    pub owner: String,
}

/// One output of a transaction, as shown before signing.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OutputLine {
    pub index: i32,
    /// In BTC, with 8 decimals.
    pub amount: String,
    pub address: String,
    /// True when it is paid to someone else: the address to check carefully.
    pub external: bool,
    /// What it is: sent to someone else, or change back to this wallet (verified from the seed).
    pub note: String,
}

/// Everything to look at before agreeing to sign.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct TxReview {
    /// mainnet, testnet, signet or regtest.
    pub network: String,
    pub fingerprint: String,
    pub inputs: Vec<InputLine>,
    pub outputs: Vec<OutputLine>,
    /// Amounts in BTC, with 8 decimals.
    pub total_in: String,
    pub total_out: String,
    pub fee: String,
    /// What leaves the wallet: the outputs not verified as its own (the fee comes on top).
    pub sending: String,
    /// How many inputs this wallet signs.
    pub inputs_to_sign: i32,
    /// For a multisig wallet's transaction: which cosigner this seed is, and whether its signature is the
    /// last one needed ("Multisig "vault", 2 of 3: this seed is cosigner 1 and adds its signature (1 of the
    /// 2 needed, with it). 1 more cosigner must sign it too before Sparrow can finalize it.").
    pub multisig: Option<String>,
    pub warnings: Vec<String>,
}

/// A signed transaction.
#[derive(uniffi::Record, Clone, Debug)]
pub struct Signed {
    /// How many inputs were signed.
    pub inputs: i32,
    /// The signed transaction, to give back to Sparrow (as an animated QR code, or a file).
    pub transaction: Arc<Transaction>,
    /// For a multisig wallet's transaction: how many of the signatures it needs it has now, and what is
    /// left to do ("Multisig "vault": it now has 1 of the 2 signatures it needs. 1 more cosigner must sign
    /// it too before Sparrow can finalize it.").
    pub multisig: Option<String>,
}

/// The wallet of a seed (with a passphrase or without) for one network. The master key is wiped when
/// the app lets go of the object.
#[derive(uniffi::Object)]
pub struct Wallet {
    pub(crate) inner: CoreWallet,
    passphrase_used: bool,
}

impl Wallet {
    pub(crate) fn new(inner: CoreWallet, passphrase_used: bool) -> Wallet {
        Wallet { inner, passphrase_used }
    }
}

fn btc(amount: seedlock_core::bitcoin::Amount) -> String {
    psbt::format_btc(amount)
}

#[uniffi::export]
impl Wallet {
    /// The fingerprint of this wallet (with its passphrase, if one was given).
    pub fn fingerprint(&self) -> String {
        self.inner.fingerprint().to_string()
    }

    pub fn network(&self) -> Net {
        Net::of(self.inner.network())
    }

    /// The wallet laid out for Sparrow: its QR code (carrying `name`, the backup's name, as the keystore's
    /// label), its three fields, the one-line descriptor, the first addresses to compare, and the other
    /// address types.
    pub fn sparrow(&self, address_type: AddressType, name: String) -> Result<SparrowWallet> {
        let wallet = &self.inner;
        let script = address_type.script();
        let label = keystore_label(&name, wallet.fingerprint());
        let qr = wallet.crypto_output_ur(script, 0, &label)?;
        let body = script.wrap_descriptor(&format!("{}/<0;1>/*", wallet.xpub_with_origin(script, 0)?));
        let descriptor = format!("{body}#{}", descriptor_checksum(&body)?);
        let base = path_to_display(&wallet.account_path(script, 0)?);
        let network = match wallet.network() {
            Network::Bitcoin => "mainnet (real bitcoin)",
            Network::Regtest => "regtest",
            _ => "a test network (testnet or signet)",
        };
        let first_addresses = (0..5)
            .map(|i| Ok(AddressLine { path: format!("{base}/0/{i}"), address: wallet.address(script, 0, false, i)?.to_string() }))
            .collect::<Result<Vec<_>>>()?;
        let other_types = AddressType::ALL
            .into_iter()
            .filter(|other| *other != address_type)
            .map(|other| {
                Ok(OtherAddressType {
                    name: other.sparrow_name().to_string(),
                    derivation: path_to_display(&wallet.account_path(other.script(), 0)?),
                    xpub: wallet.account_xpub(other.script(), 0)?.to_string(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SparrowWallet {
            fingerprint: wallet.fingerprint().to_string(),
            address_type: address_type.sparrow_name().to_string(),
            network: network.to_string(),
            passphrase_used: self.passphrase_used,
            derivation: base,
            xpub: wallet.account_xpub(script, 0)?.to_string(),
            descriptor,
            qr,
            label,
            first_addresses,
            other_types,
        })
    }

    /// Check a transaction against this wallet without signing it: every input and output, what is
    /// verified as this wallet's own, the fee and the warnings. `multisig`: the multisig wallets kept on
    /// this phone, whose inputs and change this seed can then check and sign as one of their cosigners.
    /// An error says why it cannot be signed.
    pub fn review(&self, transaction: Arc<Transaction>, multisig: Vec<RegisteredMultisig>) -> Result<TxReview> {
        let r = psbt::review_with(&transaction.psbt, &self.inner, &registered(&multisig)).map_err(for_the_phone)?;
        let inputs = r
            .inputs
            .iter()
            .map(|i| InputLine {
                index: i.index as i32,
                amount: btc(i.value),
                mine: i.path.is_some(),
                owner: match (&i.path, &i.multisig) {
                    (Some(p), Some(wallet)) => format!("yours, from the multisig wallet \"{wallet}\" ({p})"),
                    (Some(p), None) => format!("yours ({p})"),
                    (None, _) => "NOT yours".to_string(),
                },
            })
            .collect();
        let outputs = r
            .outputs
            .iter()
            .map(|o| OutputLine {
                index: o.index as i32,
                amount: btc(o.value),
                address: o.address.clone(),
                external: o.kind == OutputKind::External,
                note: match &o.kind {
                    OutputKind::External => "SENT TO SOMEONE ELSE. Check this address carefully!".to_string(),
                    OutputKind::Change { path } => format!("change back to you (verified from your seed: {path})"),
                    OutputKind::OwnReceive { path } => format!("goes to your own address (verified from your seed: {path})"),
                    OutputKind::MultisigChange { path, wallet } => {
                        format!("change back to the multisig wallet \"{wallet}\" (verified from all its cosigners' keys: {path})")
                    }
                    OutputKind::MultisigReceive { path, wallet } => {
                        format!("goes to the multisig wallet \"{wallet}\" (verified from all its cosigners' keys: {path})")
                    }
                },
            })
            .collect();
        Ok(TxReview {
            network: network_name(r.network).to_string(),
            fingerprint: r.fingerprint.to_string(),
            inputs,
            outputs,
            total_in: btc(r.total_in),
            total_out: btc(r.total_out),
            fee: btc(r.fee),
            sending: btc(r.sending),
            inputs_to_sign: r.own_inputs as i32,
            multisig: r.multisig.as_ref().map(multisig_note),
            warnings: r.warnings.clone(),
        })
    }

    /// Sign this wallet's inputs (`multisig`: as for [`Wallet::review`]). The transaction given stays as
    /// it was; the signed one is returned.
    pub fn sign(&self, transaction: Arc<Transaction>, multisig: Vec<RegisteredMultisig>) -> Result<Signed> {
        let registered = registered(&multisig);
        let before = psbt::review_with(&transaction.psbt, &self.inner, &registered).map_err(for_the_phone)?;
        let mut psbt = transaction.psbt.clone();
        let count = psbt::sign_with(&mut psbt, &self.inner, &registered).map_err(for_the_phone)?;
        Ok(Signed {
            inputs: count as i32,
            transaction: Arc::new(Transaction::new(psbt)),
            multisig: before.multisig.as_ref().map(multisig_after),
        })
    }
}

/// A multisig transaction once this seed has signed it: the signatures it has now, and what is left to do.
fn multisig_after(m: &psbt::MultisigSummary) -> String {
    let have = (m.signed_by_others + 1).min(m.threshold);
    if have >= m.threshold {
        format!("Multisig \"{}\": it now has the {} signatures it needs. Sparrow can finalize and broadcast it.", m.name, m.threshold)
    } else {
        let more = m.threshold - have;
        format!(
            "Multisig \"{}\": it now has {have} of the {} signatures it needs. {more} more cosigner{} must sign it too \
             before Sparrow can finalize it.",
            m.name,
            m.threshold,
            if more == 1 { "" } else { "s" }
        )
    }
}

/// What the command-line program says about a multisig transaction under its review.
fn multisig_note(m: &psbt::MultisigSummary) -> String {
    let after = m.signed_by_others + 1;
    let adds = format!(
        "Multisig \"{}\", {} of {}: this seed is cosigner {} and adds its signature ({} of the {} needed, with it).",
        m.name,
        m.threshold,
        m.cosigners,
        m.position,
        after.min(m.threshold),
        m.threshold
    );
    if after >= m.threshold {
        format!("{adds} That is enough: after this, Sparrow can finalize and broadcast the transaction.")
    } else {
        let more = m.threshold - after;
        format!("{adds} {more} more cosigner{} must sign it too before Sparrow can finalize it.", if more == 1 { "" } else { "s" })
    }
}

/// The command-line program's way to register a multisig wallet, as seedlock-core words it ...
const REGISTER_THERE: &str =
    "Register the wallet first (menu 10), with the output descriptor that Sparrow exports (File > Export Wallet > Output Descriptor)";

/// ... and this app's.
const REGISTER_HERE: &str = "Add the wallet to this phone first (on SeedLock's first screen: Add a multisig wallet), with \
                             its QR code or the file that Sparrow exports";

/// seedlock-core's errors, with what they say about the command-line program's menus said for the phone.
fn for_the_phone(e: seedlock_core::Error) -> SeedLockError {
    match SeedLockError::from(e) {
        SeedLockError::Failed(text) => SeedLockError::Failed(text.replace(REGISTER_THERE, REGISTER_HERE)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qr::qr_animation;
    use crate::scanner::{tests::camera_picture, QrScanner, ScanTarget};
    use crate::seed::Seed;
    use crate::transaction::NetworkHint;
    use seedlock_core::bitcoin::bip32::{ChildNumber, Fingerprint};
    use seedlock_core::bitcoin::hashes::Hash;
    use seedlock_core::bitcoin::NetworkKind;
    use seedlock_core::cbor::{self, Value};
    use seedlock_core::bitcoin::{absolute::LockTime, bip32::DerivationPath, transaction::Version, Amount, OutPoint, Psbt, ScriptBuf, Sequence, TxIn, TxOut, Txid, WPubkeyHash, Witness};

    const WORDS: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    /// A payment as Sparrow makes it: one coin of this wallet (its first receive address, 100 000 sat)
    /// pays 60 000 sat to someone else and 39 000 sat back to the wallet's first change address.
    fn payment(w: &CoreWallet) -> Arc<Transaction> {
        let own = |change: bool| w.address(ScriptType::Wpkh, 0, change, 0).unwrap().script_pubkey();
        let tx = seedlock_core::bitcoin::Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint { txid: Txid::from_byte_array([7; 32]), vout: 1 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut { value: Amount::from_sat(60_000), script_pubkey: ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([9; 20])) },
                TxOut { value: Amount::from_sat(39_000), script_pubkey: own(true) },
            ],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(TxOut { value: Amount::from_sat(100_000), script_pubkey: own(false) });
        let origin = |p: &str| (w.public_key_at(&p.parse::<DerivationPath>().unwrap()).unwrap(), (w.fingerprint(), p.parse::<DerivationPath>().unwrap()));
        let (key, source) = origin("m/84'/0'/0'/0/0");
        psbt.inputs[0].bip32_derivation.insert(key, source);
        let (key, source) = origin("m/84'/0'/0'/1/0");
        psbt.outputs[1].bip32_derivation.insert(key, source);
        Arc::new(Transaction::new(psbt))
    }

    #[test]
    fn reviews_and_signs_a_payment_and_gives_it_back_as_an_animated_qr_code() {
        let seed = Seed::from_words(WORDS.into()).unwrap();
        let wallet = seed.wallet(Vec::new(), Net::Mainnet).unwrap();
        let tx = payment(&wallet.inner);
        assert_eq!(tx.network_hint(), NetworkHint::Mainnet);
        assert_eq!((tx.input_count(), tx.output_count()), (1, 2));
        let r = wallet.review(tx.clone(), Vec::new()).unwrap();
        assert_eq!((r.network.as_str(), r.fingerprint.as_str()), ("mainnet", "73c5da0a"));
        assert!(r.inputs[0].mine && r.inputs[0].owner.starts_with("yours (m/84'/0'/0'/0/0"), "{:?}", r.inputs[0]);
        assert!(r.outputs[0].external && r.outputs[0].note.contains("SENT TO SOMEONE ELSE"));
        assert!(!r.outputs[1].external && r.outputs[1].note.contains("change back to you"), "{:?}", r.outputs[1]);
        assert_eq!((r.total_in.as_str(), r.total_out.as_str()), ("0.00100000", "0.00099000"));
        assert_eq!((r.fee.as_str(), r.sending.as_str(), r.inputs_to_sign), ("0.00001000", "0.00060000", 1));

        assert_eq!(tx.input_fingerprints(), ["73c5da0a"], "the wallet that signs it");
        let signed = wallet.sign(tx.clone(), Vec::new()).unwrap();
        assert_eq!((signed.inputs, signed.multisig.as_deref()), (1, None));
        assert!(tx.psbt.inputs[0].partial_sigs.is_empty(), "the transaction given stays as it was");
        assert_eq!(signed.transaction.psbt.inputs[0].partial_sigs.len(), 1);
        assert_eq!(signed.transaction.txid(), tx.txid(), "the same payment");
        // another seed has nothing to sign in it
        let stranger = Seed::from_words("legal winner thank year wave sausage worth useful legal winner thank yellow".into()).unwrap();
        let stranger = stranger.wallet(Vec::new(), Net::Mainnet).unwrap();
        let other = stranger.review(tx.clone(), Vec::new()).map(|r| r.inputs_to_sign).unwrap_or(0);
        assert_eq!(other, 0);

        // the signed transaction through its animated QR code, a camera and the scanner: the same bytes arrive
        let codes = qr_animation(signed.transaction.qr_frames().unwrap()).unwrap();
        let scanner = QrScanner::new(ScanTarget::Transaction);
        let arrived = codes.iter().cycle().take(codes.len() * 2).enumerate().any(|(i, code)| {
            let (w, h, luma) = camera_picture(code, 40 + i as u64);
            scanner.add_picture(w, h, luma).unwrap().done
        });
        assert!(arrived);
        assert_eq!(scanner.transaction().unwrap().to_bytes(), signed.transaction.to_bytes());
        // and as a file
        let from_file = Transaction::from_bytes(signed.transaction.to_bytes()).unwrap();
        assert_eq!(from_file.to_base64(), signed.transaction.to_base64());
        assert!(Transaction::from_bytes(b"not a transaction".to_vec()).is_err());
    }

    /// The CBOR inside a `UR:CRYPTO-OUTPUT/...` text, with its script tags taken off (outermost first).
    fn unwrap_output(text: &str) -> (Vec<u64>, Value) {
        assert!(text.starts_with("UR:CRYPTO-OUTPUT/"), "{text}");
        let (_, bytes) = ur::ur::decode(text).unwrap();
        let mut value = cbor::decode(&bytes).unwrap();
        let mut tags = Vec::new();
        while let Value::Tag(tag, inner) = value {
            if tag == 303 {
                return (tags, *inner);
            }
            tags.push(tag);
            value = *inner;
        }
        panic!("no crypto-hdkey in {text}");
    }

    #[test]
    fn the_qr_code_for_sparrow_is_a_ur_crypto_output_named_after_the_backup() {
        use seedlock_core::bitcoin::bip32::{ChainCode, Xpub};
        let seed = Seed::from_words(WORDS.into()).unwrap();
        let wallet = seed.wallet(Vec::new(), Net::Mainnet).unwrap();
        let s = wallet.sparrow(AddressType::NativeSegwit, "My wallet".into()).unwrap();
        let (tags, hdkey) = unwrap_output(&s.qr);
        assert_eq!(tags, [404], "wpkh");
        assert_eq!(hdkey.get(9), Some(&Value::Text("My wallet".to_string())), "the name Sparrow gives the keystore");
        let Some(Value::Tag(304, origin)) = hdkey.get(6) else { panic!("the key's origin") };
        let path: Vec<Value> = [84, 0, 0].iter().flat_map(|&i| [Value::Uint(i), Value::Bool(true)]).collect();
        assert_eq!(origin.get(1), Some(&Value::Array(path)));
        assert_eq!(origin.get(2).and_then(Value::as_uint), Some(0x73c5_da0a), "the master fingerprint");
        let Some(Value::Tag(305, use_info)) = hdkey.get(5) else { panic!("the network") };
        assert_eq!(use_info.get(2).and_then(Value::as_uint), Some(0), "mainnet");
        // the fields rebuild the account xpub of BIP84's test vector
        let parent = hdkey.get(8).and_then(Value::as_uint).unwrap() as u32;
        let chain: [u8; 32] = hdkey.get(4).and_then(Value::as_bytes).unwrap().try_into().unwrap();
        let xpub = Xpub {
            network: NetworkKind::Main,
            depth: 3,
            parent_fingerprint: Fingerprint::from(parent.to_be_bytes()),
            child_number: ChildNumber::from_hardened_idx(0).unwrap(),
            public_key: seedlock_core::bitcoin::secp256k1::PublicKey::from_slice(hdkey.get(3).and_then(Value::as_bytes).unwrap()).unwrap(),
            chain_code: ChainCode::from(chain),
        };
        assert_eq!(xpub.to_string(), s.xpub);

        // every address type has its own tags, and a test network says so
        let t = seed.wallet(Vec::new(), Net::Testnet).unwrap();
        for (kind, purpose, tags) in [
            (AddressType::NativeSegwit, 84, vec![404]),
            (AddressType::NestedSegwit, 49, vec![400, 404]),
            (AddressType::Legacy, 44, vec![403]),
            (AddressType::Taproot, 86, vec![409]),
        ] {
            let (found, hdkey) = unwrap_output(&t.sparrow(kind, "Test".into()).unwrap().qr);
            assert_eq!(found, tags, "{kind:?}");
            let Some(Value::Tag(305, use_info)) = hdkey.get(5) else { panic!("the network") };
            assert_eq!(use_info.get(2).and_then(Value::as_uint), Some(1), "a test network");
            let Some(Value::Tag(304, origin)) = hdkey.get(6) else { panic!("the key's origin") };
            assert_eq!(origin.get(1).and_then(Value::as_array).unwrap()[0].as_uint(), Some(purpose));
        }
    }

    #[test]
    fn the_wallet_for_sparrow_matches_the_known_test_vector() {
        let seed = Seed::from_words(WORDS.into()).unwrap();
        let wallet = seed.wallet(Vec::new(), Net::Mainnet).unwrap();
        assert_eq!(wallet.network(), Net::Mainnet);
        let s = wallet.sparrow(AddressType::NativeSegwit, "Savings of the family".into()).unwrap();
        assert_eq!(s.fingerprint, "73c5da0a");
        // the QR code for Sparrow carries the backup's name, cut to the 16 characters of a keystore label
        assert_eq!(s.label, "Savings of the f");
        assert!(s.qr.starts_with("UR:CRYPTO-OUTPUT/"), "{}", s.qr);
        assert_eq!(wallet.sparrow(AddressType::NativeSegwit, "  ".into()).unwrap().label, "SeedLock 73c5", "no name: the fingerprint");
        assert_eq!(s.derivation, "m/84'/0'/0'");
        // BIP84's test vector for this seed
        assert_eq!(s.xpub, "xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYVUhLv1VMrjPC7PW6V");
        assert_eq!(s.first_addresses[0].address, "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
        assert_eq!(s.first_addresses[0].path, "m/84'/0'/0'/0/0");
        // the same descriptor as the command-line program's menu 5 gives (key origin written with h, as Sparrow accepts it)
        assert!(s.descriptor.starts_with("wpkh([73c5da0a/84h/0h/0h]xpub6CatWdiZ") && s.descriptor.ends_with("/<0;1>/*)#qf45pmyh"), "{}", s.descriptor);
        assert_eq!(s.other_types.len(), 3);
        assert!(!s.passphrase_used && s.network.contains("real bitcoin"));
        // with a passphrase: another wallet
        let other = seed.wallet(b"TREZOR".to_vec(), Net::Testnet).unwrap();
        assert_ne!(other.fingerprint(), "73c5da0a");
        let t = other.sparrow(AddressType::Taproot, "Test".into()).unwrap();
        assert!(t.passphrase_used && t.xpub.starts_with("tpub") && t.first_addresses[0].address.starts_with("tb1p"));
        assert_eq!(t.derivation, "m/86'/1'/0'");
    }
}
