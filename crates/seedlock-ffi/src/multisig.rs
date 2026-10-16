//! Multisig wallets on the phone, as the command-line program's menu 10 has them: this seed's key for a
//! multisig wallet in Sparrow ([`Wallet::cosigner_key`]), the multisig wallets kept on the phone (read
//! from Sparrow's QR code, the file it exports or a pasted descriptor, and kept as the command-line
//! program's `.multisig` files), and checking and signing their transactions ([`Wallet::review`] and
//! [`Wallet::sign`] with [`RegisteredMultisig`]). The texts are the command-line program's.

use crate::error::{failed, Result};
use crate::seed::check_file_name;
use crate::wallet::Wallet;
use seedlock_core::bitcoin::bip32::{DerivationPath, Fingerprint, Xpub};
use seedlock_core::bitcoin::{Network, NetworkKind};
use seedlock_core::multisig::{cosigner_account_ur, keystore_label, MultisigScript, MultisigWallet as CoreMultisig};
use seedlock_core::wallet::{path_to_display, path_to_string};
use std::fmt::Write as _;
use std::sync::Arc;

/// The longest text read as a wallet's descriptor (the command-line program reads files of up to 1 MB).
const MAX_TEXT_BYTES: usize = 1_000_000;

/// The kind of a multisig wallet: Sparrow's "Script Type" for it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultisigKind {
    /// Native SegWit (P2WSH), the usual kind: addresses bc1q... (longer than single-signature ones).
    NativeSegwit,
    /// Nested SegWit (P2SH-P2WSH): addresses 3...
    NestedSegwit,
    /// Legacy (P2SH): addresses 3..., key path m/45'.
    Legacy,
}

impl MultisigKind {
    fn script(self) -> MultisigScript {
        match self {
            MultisigKind::NativeSegwit => MultisigScript::Wsh,
            MultisigKind::NestedSegwit => MultisigScript::ShWsh,
            MultisigKind::Legacy => MultisigScript::Sh,
        }
    }

    fn of(script: MultisigScript) -> MultisigKind {
        match script {
            MultisigScript::Wsh => MultisigKind::NativeSegwit,
            MultisigScript::ShWsh => MultisigKind::NestedSegwit,
            MultisigScript::Sh => MultisigKind::Legacy,
        }
    }
}

/// Sparrow's name for a kind of multisig wallet, as in its "Script Type" list ("Native Segwit (P2WSH)").
#[uniffi::export]
pub fn multisig_kind_name(kind: MultisigKind) -> String {
    kind.script().sparrow_name().to_string()
}

fn net_words(mainnet: bool) -> &'static str {
    if mainnet {
        "mainnet (real bitcoin)"
    } else {
        "a test network (testnet or signet)"
    }
}

/// The name of a multisig wallet on this phone (it also names its file), cleaned up from what was typed
/// (spaces around it, quotes and a typed ".multisig" are removed), or an error that says what is wrong
/// with it. The same rules as for backups.
#[uniffi::export]
pub fn check_wallet_name(typed: String) -> Result<String> {
    check_file_name(&typed, "multisig", "wallet")
}

/// One cosigner of a multisig wallet: where its key comes from (the master fingerprint of its seed and
/// the key path) and the key.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct CosignerLine {
    pub fingerprint: String,
    /// The key path, written the way Sparrow shows it (m/48'/0'/0'/2').
    pub derivation: String,
    pub xpub: String,
}

/// A multisig wallet (for example 2 of 3): its kind, its cosigners' keys and how many must sign. It
/// holds no secrets, only public keys.
#[derive(uniffi::Object, Debug)]
pub struct MultisigWallet {
    pub(crate) inner: CoreMultisig,
}

impl MultisigWallet {
    pub(crate) fn new(inner: CoreMultisig) -> MultisigWallet {
        MultisigWallet { inner }
    }

    fn network(&self) -> Network {
        if self.inner.is_mainnet() {
            Network::Bitcoin
        } else {
            Network::Testnet
        }
    }
}

#[uniffi::export]
impl MultisigWallet {
    /// A multisig wallet from its output descriptor: as Sparrow exports it (File > Export Wallet >
    /// Output Descriptor; the whole file is fine), pasted, or a wallet's `.multisig` file as
    /// [`MultisigWallet::file_contents`] (and the command-line program) writes it. The error says what is
    /// wrong (a changed checksum, a Taproot wallet, a key without its origin...).
    #[uniffi::constructor]
    pub fn from_text(text: String) -> Result<Arc<MultisigWallet>> {
        if text.len() > MAX_TEXT_BYTES {
            return failed("that is too long to be a wallet's descriptor");
        }
        Ok(Arc::new(MultisigWallet::new(CoreMultisig::from_descriptor_text(&text)?)))
    }

    /// "2 of 3".
    pub fn label(&self) -> String {
        self.inner.label()
    }

    /// How many signatures a payment needs.
    pub fn threshold(&self) -> i32 {
        self.inner.threshold as i32
    }

    pub fn cosigner_count(&self) -> i32 {
        self.inner.cosigners.len() as i32
    }

    pub fn kind(&self) -> MultisigKind {
        MultisigKind::of(self.inner.script)
    }

    /// Sparrow's name for its kind ("Native Segwit (P2WSH)"), and for a wallet whose keys keep the order
    /// the descriptor gives them (`multi` instead of `sortedmulti`), "... with its keys in a fixed order".
    pub fn kind_name(&self) -> String {
        let order = if self.inner.sorted { "" } else { " with its keys in a fixed order" };
        format!("{}{order}", self.inner.script.sparrow_name())
    }

    /// True for real bitcoin (xpub keys), false for a test network (tpub keys).
    pub fn mainnet(&self) -> bool {
        self.inner.is_mainnet()
    }

    /// "mainnet (real bitcoin)" or "a test network (testnet or signet)".
    pub fn network_name(&self) -> String {
        net_words(self.inner.is_mainnet()).to_string()
    }

    /// The cosigners, in the descriptor's order.
    pub fn cosigners(&self) -> Vec<CosignerLine> {
        self.inner
            .cosigners
            .iter()
            .map(|c| CosignerLine { fingerprint: c.fingerprint.to_string(), derivation: path_to_display(&c.path), xpub: c.xpub.to_string() })
            .collect()
    }

    /// The receive address number `index` (0 is the first). Sparrow shows the same ones, in the wallet's
    /// Addresses tab.
    pub fn receive_address(&self, index: i32) -> Result<String> {
        let Ok(index) = u32::try_from(index) else { return failed("an address number cannot be negative") };
        Ok(self.inner.address(0, index, self.network())?.to_string())
    }

    // (no slash and star in the doc comment below: in the Kotlin code made from it, they would open a comment)
    /// The whole wallet as one descriptor, the way Sparrow writes it (each key with its receive and change
    /// paths, `<0;1>`, and the checksum after `#`).
    pub fn descriptor(&self) -> Result<String> {
        Ok(self.inner.descriptor()?)
    }

    /// Whether `other` is the same wallet: the same kind, threshold and keys.
    pub fn same_as(&self, other: Arc<MultisigWallet>) -> bool {
        self.inner.same_as(&other.inner)
    }

    /// Which cosigner (1 for the first, in the descriptor's order) the seed with this master fingerprint
    /// is; none when it is not one of them.
    pub fn position_of(&self, fingerprint: String) -> Option<i32> {
        let fingerprint: Fingerprint = fingerprint.trim().parse().ok()?;
        self.inner.position(fingerprint).map(|p| p as i32 + 1)
    }

    /// The name the command-line program suggests for it: "multisig-2-of-3".
    pub fn default_name(&self) -> String {
        format!("multisig-{}-of-{}", self.inner.threshold, self.inner.cosigners.len())
    }

    /// What goes into the wallet's file, `<name>.multisig`: the same as the command-line program writes
    /// into its "multisig wallets" folder (so a copy of the file works there too), and Sparrow reads it as
    /// an output descriptor file (File > Import Wallet > Output Descriptor).
    pub fn file_contents(&self, name: String) -> Result<String> {
        let name = check_wallet_name(name)?;
        let ms = &self.inner;
        let cosigners: Vec<String> = ms.cosigners.iter().map(|c| format!("{} {}", c.fingerprint, path_to_display(&c.path))).collect();
        let mut body = String::new();
        let _ = writeln!(body, "# SeedLock multisig wallet \"{name}\": {}, {}, {}", ms.label(), self.kind_name(), net_words(ms.is_mainnet()));
        let _ = writeln!(body, "# Cosigners: {}", cosigners.join(", "));
        let _ = writeln!(body, "# First receive address: {}", ms.address(0, 0, self.network())?);
        let _ = writeln!(body, "# No secrets in here, but anyone who reads it can see all the wallet's addresses.");
        let _ = writeln!(body, "{}", ms.descriptor()?);
        Ok(body)
    }
}

/// A multisig wallet kept on this phone, with the name it is kept under: what checking and signing a
/// transaction need to know about it ([`Wallet::review`], [`Wallet::sign`]).
#[derive(uniffi::Record, Clone, Debug)]
pub struct RegisteredMultisig {
    pub name: String,
    pub wallet: Arc<MultisigWallet>,
}

/// The registered wallets as seedlock-core takes them.
pub(crate) fn registered(wallets: &[RegisteredMultisig]) -> Vec<(String, CoreMultisig)> {
    wallets.iter().map(|w| (w.name.clone(), w.wallet.inner.clone())).collect()
}

/// This seed's key for a multisig wallet, laid out for Sparrow as the command-line program's menu 10
/// shows it: one QR code to scan, or three fields to type.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct CosignerKey {
    pub fingerprint: String,
    /// The key path, written the way Sparrow wants it (m/48'/0'/0'/2').
    pub derivation: String,
    pub xpub: String,
    /// All three in one piece, `[73c5da0a/48h/0h/0h/2h]xpub...`, for wallets that ask for it that way.
    pub key_with_origin: String,
    /// Sparrow's name for the wallet's kind (its "Script Type").
    pub kind_name: String,
    pub mainnet: bool,
    /// "mainnet (real bitcoin)" or "a test network (testnet or signet)".
    pub network: String,
    /// The label Sparrow gives this seed's keystore when it scans [`CosignerKey::qr`]: the backup's name,
    /// cut to the 16 characters Sparrow allows.
    pub label: String,
    /// True when the backup's name had to be cut to make [`CosignerKey::label`].
    pub label_shortened: bool,
    /// The text of the QR code that Sparrow scans with the camera button next to a keystore's xpub field:
    /// a UR crypto-account (`UR:CRYPTO-ACCOUNT/...`), as SeedSigner shows it.
    pub qr: String,
    /// All of the above as text, to save as a file (`<name>-cosigner.txt`, as the command-line program
    /// names it).
    pub text: String,
}

/// This seed's key at `path` for a multisig wallet of kind `script`, laid out for Sparrow. `name` is the
/// backup's name, which Sparrow takes as the keystore's label.
fn lay_out(master: Fingerprint, path: &DerivationPath, xpub: &Xpub, script: MultisigScript, name: &str) -> CosignerKey {
    let mainnet = xpub.network == NetworkKind::Main;
    let label = keystore_label(name, master);
    let label_shortened = !name.trim().is_empty() && label != name.trim();
    let qr = cosigner_account_ur(master, path, xpub, script, &label);
    let derivation = path_to_display(path);
    let key_with_origin = format!("[{master}/{}]{xpub}", path_to_string(path));
    let kind = script.sparrow_name();
    let network = net_words(mainnet);
    let shortened = if label_shortened { " (Sparrow allows 16 characters)" } else { "" };
    // the command-line program's text (menu 10, 1), and the QR code's text, as in its "-cosigner.txt" files
    let mut t = String::new();
    let _ = writeln!(t, "Seed {master} as a cosigner of a multisig wallet: {kind}, {network}");
    let _ = writeln!(t, "\nIn Sparrow: File > New Wallet, Policy Type \"Multi Signature\" (for example 2 of 3), Script Type");
    let _ = writeln!(t, "\"{kind}\". For this seed's keystore choose \"Watch Only Wallet\", then either click the camera");
    let _ = writeln!(t, "button next to the xpub field (\"Scan a QR code\") and scan this seed's QR code, which fills in the fields");
    let _ = writeln!(t, "and names the keystore \"{label}\"{shortened}, or type:");
    let _ = writeln!(t, "  Master fingerprint : {master}");
    let _ = writeln!(t, "  Derivation         : {derivation}");
    let _ = writeln!(t, "  xpub               : {xpub}");
    let _ = writeln!(t, "\nIn one piece, for wallets that ask for it that way:\n  {key_with_origin}");
    let _ = writeln!(t, "\nAs a QR code (UR crypto-account):\n  {qr}");
    CosignerKey {
        fingerprint: master.to_string(),
        derivation,
        xpub: xpub.to_string(),
        key_with_origin,
        kind_name: kind.to_string(),
        mainnet,
        network: network.to_string(),
        label,
        label_shortened,
        qr,
        text: t,
    }
}

#[uniffi::export]
impl Wallet {
    /// This seed's key for a new multisig wallet of `kind`, for real bitcoin (`mainnet`) or a test
    /// network, at the usual key path: BIP 48 for SegWit (m/48'/0'/0'/2' native, m/48'/0'/0'/1' nested;
    /// coin 1' on a test network) and BIP 45 for legacy (m/45'). The key is the same whichever network this
    /// wallet was opened for; only the way it is written (xpub or tpub) follows `mainnet`. `name` is the
    /// backup's name: Sparrow names the keystore after it.
    pub fn cosigner_key(&self, kind: MultisigKind, mainnet: bool, name: String) -> Result<CosignerKey> {
        let script = kind.script();
        let path = script.account_path(mainnet);
        let xpub = self.inner.xpub_at(&path)?;
        let xpub = Xpub { network: if mainnet { NetworkKind::Main } else { NetworkKind::Test }, ..xpub };
        Ok(lay_out(self.inner.fingerprint(), &path, &xpub, script, &name))
    }

    /// This seed's key in `wallet`, a multisig wallet that it is a cosigner of: at that cosigner's key
    /// path, with the wallet's kind and network, checked against the key the wallet holds for it. An error
    /// says when this seed (with its BIP39 passphrase, if it has one) is not one of its cosigners.
    pub fn cosigner_key_in(&self, wallet: Arc<MultisigWallet>, name: String) -> Result<CosignerKey> {
        let fingerprint = self.inner.fingerprint();
        let ms = &wallet.inner;
        let mut ours = ms.cosigners.iter().filter(|c| c.fingerprint == fingerprint).peekable();
        if ours.peek().is_none() {
            return failed(format!(
                "this seed (fingerprint {fingerprint}) is not one of the cosigners of this multisig wallet. With a BIP39 \
                 passphrase it is another wallet: was the passphrase given, and right?"
            ));
        }
        for cosigner in ours {
            let derived = self.inner.xpub_at(&cosigner.path)?;
            if derived.public_key == cosigner.xpub.public_key && derived.chain_code == cosigner.xpub.chain_code {
                // the key as this seed derives it (its parent fingerprint too, which a wallet read from a QR
                // code may lack), written for the wallet's network
                let xpub = Xpub { network: cosigner.xpub.network, ..derived };
                return Ok(lay_out(fingerprint, &cosigner.path, &xpub, ms.script, &name));
            }
        }
        failed(format!(
            "the multisig wallet names this seed's fingerprint ({fingerprint}), but the key it holds for it is not this \
             seed's key at that key path. This seed cannot sign for that wallet."
        ))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::seed::Seed;
    use crate::transaction::{NetworkHint, Transaction};
    use crate::wallet::Net;
    use seedlock_core::bitcoin::hashes::Hash;
    use seedlock_core::bitcoin::{absolute::LockTime, transaction::Version, Amount, OutPoint, Psbt, ScriptBuf, Sequence, TxIn, TxOut, WPubkeyHash, Witness};
    use seedlock_core::cbor::{self, Value};
    use seedlock_core::wallet::descriptor_checksum;

    pub(crate) const SEEDS: [&str; 3] = [
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        "legal winner thank year wave sausage worth useful legal winner thank yellow",
        "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
    ];

    pub(crate) fn wallet(i: usize, net: Net) -> Arc<Wallet> {
        Seed::from_words(SEEDS[i].into()).unwrap().wallet(Vec::new(), net).unwrap()
    }

    /// The three seeds' 2-of-3 wallet as Sparrow exports it, `wrap` being "wsh(", "sh(wsh(" or "sh(" around
    /// `multi` ("sortedmulti" or "multi"), at `account` ("48h/1h/0h/2h") on a test network.
    pub(crate) fn descriptor(wrap: &str, multi: &str, account: &str) -> String {
        let path: DerivationPath = format!("m/{}", account.replace('h', "'")).parse().unwrap();
        let keys: Vec<String> = (0..3)
            .map(|i| {
                let w = wallet(i, Net::Testnet);
                format!("[{}/{account}]{}/<0;1>/*", w.inner.fingerprint(), w.inner.xpub_at(&path).unwrap())
            })
            .collect();
        let closing = ")".repeat(wrap.matches('(').count());
        let body = format!("{wrap}{multi}(2,{}){closing}", keys.join(","));
        format!("{body}#{}", descriptor_checksum(&body).unwrap())
    }

    #[test]
    fn reads_a_wallet_from_sparrow_and_keeps_it_as_the_command_line_program_does() {
        let text = descriptor("wsh(", "sortedmulti", "48h/1h/0h/2h");
        let exported = format!("# Receive and change descriptor:\n{text}\n\n\n# Receive descriptor:\n{}\n", text.replace("<0;1>", "0"));
        let ms = MultisigWallet::from_text(exported).unwrap();
        assert_eq!((ms.label().as_str(), ms.threshold(), ms.cosigner_count()), ("2 of 3", 2, 3));
        assert_eq!((ms.kind(), ms.kind_name().as_str()), (MultisigKind::NativeSegwit, "Native Segwit (P2WSH)"));
        assert!(!ms.mainnet() && ms.network_name() == "a test network (testnet or signet)");
        assert_eq!(ms.descriptor().unwrap(), text);
        let cosigners = ms.cosigners();
        assert_eq!(cosigners[0].fingerprint, "73c5da0a");
        assert_eq!(cosigners[0].derivation, "m/48'/1'/0'/2'");
        assert!(cosigners.iter().all(|c| c.xpub.starts_with("tpub")));
        assert_eq!(ms.position_of("73C5DA0A".into()), Some(1));
        assert_eq!(ms.position_of("00000000".into()), None);
        assert_eq!(ms.position_of("not one".into()), None);
        let first = ms.receive_address(0).unwrap();
        assert!(first.starts_with("tb1q") && first.len() == 62, "{first}");
        assert!(ms.receive_address(-1).is_err());
        assert_eq!(ms.default_name(), "multisig-2-of-3");

        // its file: exactly the command-line program's, and it reads back as the same wallet
        let file = ms.file_contents(" \"vault.multisig\" ".into()).unwrap();
        let expected = format!(
            "# SeedLock multisig wallet \"vault\": 2 of 3, Native Segwit (P2WSH), a test network (testnet or signet)\n\
             # Cosigners: 73c5da0a m/48'/1'/0'/2', {} m/48'/1'/0'/2', {} m/48'/1'/0'/2'\n\
             # First receive address: {first}\n\
             # No secrets in here, but anyone who reads it can see all the wallet's addresses.\n\
             {text}\n",
            cosigners[1].fingerprint, cosigners[2].fingerprint
        );
        assert_eq!(file, expected);
        assert!(MultisigWallet::from_text(file).unwrap().same_as(ms.clone()));
        assert!(ms.file_contents("a/b".into()).is_err(), "a name that cannot be a file name");

        // nested, and legacy with its keys in a fixed order (mainnet comes from the keys: tpub here)
        let nested = MultisigWallet::from_text(descriptor("sh(wsh(", "sortedmulti", "48h/1h/0h/1h")).unwrap();
        assert_eq!(nested.kind(), MultisigKind::NestedSegwit);
        assert!(nested.receive_address(0).unwrap().starts_with('2'));
        let legacy = MultisigWallet::from_text(descriptor("sh(", "multi", "45h")).unwrap();
        assert_eq!(legacy.kind_name(), "Legacy (P2SH) with its keys in a fixed order");
        assert!(!legacy.same_as(MultisigWallet::from_text(descriptor("sh(", "sortedmulti", "45h")).unwrap()));
        assert_eq!(multisig_kind_name(MultisigKind::Legacy), "Legacy (P2SH)");
    }

    #[test]
    fn says_why_a_wallet_cannot_be_read() {
        let text = descriptor("wsh(", "sortedmulti", "48h/1h/0h/2h");
        let bad = format!("{}x", &text[..text.len() - 1]);
        assert!(MultisigWallet::from_text(bad).unwrap_err().to_string().contains("checksum"));
        assert!(MultisigWallet::from_text("hello".into()).unwrap_err().to_string().contains("no multisig output descriptor"));
        assert!(MultisigWallet::from_text("x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        assert_eq!(check_wallet_name("  'Family vault.multisig' ".into()).unwrap(), "Family vault");
        let e = check_wallet_name("a:b".into()).unwrap_err().to_string();
        assert!(e.contains("A wallet name cannot contain the character ':'"), "{e}");
        assert!(check_wallet_name("  ".into()).unwrap_err().to_string().contains("name for the wallet"));
    }

    /// The UR crypto-account inside a cosigner key's QR code.
    fn account_of(key: &CosignerKey) -> Value {
        let (_, payload) = ur::ur::decode(&key.qr.to_lowercase()).unwrap();
        cbor::decode(&payload).unwrap()
    }

    #[test]
    fn lays_out_this_seeds_key_for_a_multisig_wallet_in_sparrow() {
        let w = wallet(0, Net::Mainnet);
        let key = w.cosigner_key(MultisigKind::NativeSegwit, true, "Family savings 2026".into()).unwrap();
        assert_eq!((key.fingerprint.as_str(), key.derivation.as_str()), ("73c5da0a", "m/48'/0'/0'/2'"));
        assert!(key.xpub.starts_with("xpub") && key.mainnet && key.network == "mainnet (real bitcoin)");
        assert_eq!(key.key_with_origin, format!("[73c5da0a/48h/0h/0h/2h]{}", key.xpub));
        assert_eq!((key.label.as_str(), key.label_shortened), ("Family savings 2", true));
        assert_eq!(key.kind_name, "Native Segwit (P2WSH)");
        assert!(key.qr.starts_with("UR:CRYPTO-ACCOUNT/"), "{}", key.qr);
        let account = account_of(&key);
        assert_eq!(account.get(1).and_then(Value::as_uint), Some(0x73c5_da0a), "the master fingerprint");
        let outputs = account.get(2).and_then(Value::as_array).unwrap();
        assert!(matches!(&outputs[0], Value::Tag(401, inner) if matches!(&**inner, Value::Tag(303, _))), "wsh(), then the key");
        for (wanted, why) in [
            ("Seed 73c5da0a as a cosigner of a multisig wallet: Native Segwit (P2WSH), mainnet (real bitcoin)", "first line"),
            ("names the keystore \"Family savings 2\" (Sparrow allows 16 characters), or type:", "the label"),
            ("  Derivation         : m/48'/0'/0'/2'", "the fields"),
            (key.qr.as_str(), "the QR code's text"),
        ] {
            assert!(key.text.contains(wanted), "{why}: {}", key.text);
        }
        assert!(!key.text.contains("menu"), "nothing about the command-line program's menus");

        // a test network writes the same key as tpub, at coin type 1'; the other kinds at their own paths
        let test = w.cosigner_key(MultisigKind::NativeSegwit, false, "x".into()).unwrap();
        assert!(test.xpub.starts_with("tpub") && test.derivation == "m/48'/1'/0'/2'" && !test.label_shortened);
        let opened_for_test = wallet(0, Net::Testnet).cosigner_key(MultisigKind::NativeSegwit, false, "x".into()).unwrap();
        assert_eq!(opened_for_test, test, "the same, whichever network the wallet was opened for");
        assert_eq!(w.cosigner_key(MultisigKind::NestedSegwit, true, "x".into()).unwrap().derivation, "m/48'/0'/0'/1'");
        assert_eq!(w.cosigner_key(MultisigKind::Legacy, false, "x".into()).unwrap().derivation, "m/45'");
        assert_eq!(w.cosigner_key(MultisigKind::Legacy, false, " ".into()).unwrap().label, "SeedLock 73c5");
    }

    #[test]
    fn takes_its_key_from_a_wallet_it_is_a_cosigner_of() {
        let ms = MultisigWallet::from_text(descriptor("sh(wsh(", "sortedmulti", "48h/1h/0h/1h")).unwrap();
        let second = wallet(1, Net::Mainnet); // opened for mainnet: the wallet's network decides
        let key = second.cosigner_key_in(ms.clone(), "bob".into()).unwrap();
        assert_eq!(key.fingerprint, ms.cosigners()[1].fingerprint);
        assert_eq!((key.derivation.as_str(), key.xpub.as_str()), ("m/48'/1'/0'/1'", ms.cosigners()[1].xpub.as_str()));
        assert_eq!((key.kind_name.as_str(), key.mainnet), ("Nested Segwit (P2SH-P2WSH)", false));
        assert_eq!(key, wallet(1, Net::Testnet).cosigner_key(MultisigKind::NestedSegwit, false, "bob".into()).unwrap());
        // another seed, or the same seed with a passphrase: not a cosigner
        let stranger = Seed::from_words(SEEDS[0].into()).unwrap().wallet(b"a passphrase".to_vec(), Net::Testnet).unwrap();
        let e = stranger.cosigner_key_in(ms.clone(), "x".into()).unwrap_err().to_string();
        assert!(e.contains("is not one of the cosigners") && e.contains("passphrase"), "{e}");
        // a wallet that names this seed's fingerprint, but with another seed's key: this seed cannot sign for it
        let path: DerivationPath = "m/48'/1'/0'/1'".parse().unwrap();
        let other = Seed::from_words("zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong".into()).unwrap().wallet(Vec::new(), Net::Testnet).unwrap();
        let keys = [
            format!("[73c5da0a/48h/1h/0h/1h]{}/<0;1>/*", other.inner.xpub_at(&path).unwrap()),
            format!("[{}/48h/1h/0h/1h]{}/<0;1>/*", ms.cosigners()[1].fingerprint, ms.cosigners()[1].xpub),
        ];
        let body = format!("sh(wsh(sortedmulti(1,{})))", keys.join(","));
        let forged = MultisigWallet::from_text(format!("{body}#{}", descriptor_checksum(&body).unwrap())).unwrap();
        let e = wallet(0, Net::Testnet).cosigner_key_in(forged, "x".into()).unwrap_err().to_string();
        assert!(e.contains("is not this seed's key at that key path"), "{e}");
    }

    /// Spend 100 000 sat from `ms`'s first receive address: 60 000 to someone else and 39 000 to its first
    /// change address, as Sparrow makes it: every cosigner's key path (under `account`, like "m/48'/1'/0'/2'"),
    /// the scripts and, for a legacy wallet, the whole previous transaction.
    pub(crate) fn spend(ms: &MultisigWallet, account: &str) -> Arc<Transaction> {
        let ms = &ms.inner;
        let prev = seedlock_core::bitcoin::Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: vec![TxOut { value: Amount::from_sat(100_000), script_pubkey: ms.script_pubkey(0, 0).unwrap() }],
        };
        let tx = seedlock_core::bitcoin::Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint { txid: prev.compute_txid(), vout: 0 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut { value: Amount::from_sat(60_000), script_pubkey: ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([9; 20])) },
                TxOut { value: Amount::from_sat(39_000), script_pubkey: ms.script_pubkey(1, 0).unwrap() },
            ],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        if ms.script == MultisigScript::Sh {
            psbt.inputs[0].non_witness_utxo = Some(prev.clone());
        } else {
            psbt.inputs[0].witness_utxo = Some(prev.output[0].clone());
        }
        (psbt.inputs[0].redeem_script, psbt.inputs[0].witness_script) = ms.psbt_scripts(0, 0).unwrap();
        (psbt.outputs[1].redeem_script, psbt.outputs[1].witness_script) = ms.psbt_scripts(1, 0).unwrap();
        for i in 0..3 {
            let w = wallet(i, Net::Testnet);
            for chain in [0, 1] {
                let path: DerivationPath = format!("{account}/{chain}/0").parse().unwrap();
                let origin = (w.inner.public_key_at(&path).unwrap(), (w.inner.fingerprint(), path));
                if chain == 0 {
                    psbt.inputs[0].bip32_derivation.insert(origin.0, origin.1);
                } else {
                    psbt.outputs[1].bip32_derivation.insert(origin.0, origin.1);
                }
            }
        }
        Arc::new(Transaction::new(psbt))
    }

    #[test]
    fn checks_and_signs_its_part_of_a_multisig_payment_as_one_of_the_cosigners() {
        let ms = MultisigWallet::from_text(descriptor("wsh(", "sortedmulti", "48h/1h/0h/2h")).unwrap();
        let registered = vec![RegisteredMultisig { name: "vault".into(), wallet: ms.clone() }];
        let tx = spend(&ms, "m/48'/1'/0'/2'");
        assert_eq!(tx.network_hint_with(registered.clone()), NetworkHint::TestNetwork);
        let a = wallet(0, Net::Testnet);
        // a wallet that is not on this phone: refused, saying how to add it here (and nothing about menus)
        let e = a.review(tx.clone(), Vec::new()).unwrap_err().to_string();
        assert!(e.contains("is not registered here") && e.contains("Add the wallet to this phone first") && !e.contains("menu"), "{e}");
        assert!(a.sign(tx.clone(), Vec::new()).unwrap_err().to_string().contains("Add the wallet to this phone first"));

        let r = a.review(tx.clone(), registered.clone()).unwrap();
        assert!(r.inputs[0].mine, "{:?}", r.inputs[0]);
        assert_eq!(r.inputs[0].owner, "yours, from the multisig wallet \"vault\" (m/48'/1'/0'/2'/0/0)");
        assert!(r.outputs[0].external && !r.outputs[1].external);
        assert_eq!(r.outputs[1].note, "change back to the multisig wallet \"vault\" (verified from all its cosigners' keys: m/48'/1'/0'/2'/1/0)");
        assert_eq!((r.sending.as_str(), r.fee.as_str(), r.inputs_to_sign), ("0.00060000", "0.00001000", 1));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(
            r.multisig.as_deref(),
            Some(
                "Multisig \"vault\", 2 of 3: this seed is cosigner 1 and adds its signature (1 of the 2 needed, with it). \
                 1 more cosigner must sign it too before Sparrow can finalize it."
            )
        );
        let cosigners: Vec<String> = ms.cosigners().into_iter().map(|c| c.fingerprint).collect();
        let mut named = tx.input_fingerprints();
        named.sort();
        let mut all = cosigners.clone();
        all.sort();
        assert_eq!(named, all, "the transaction names all three cosigners");
        let signed = a.sign(tx.clone(), registered.clone()).unwrap();
        assert_eq!(signed.inputs, 1);
        assert_eq!(
            signed.multisig.as_deref(),
            Some("Multisig \"vault\": it now has 1 of the 2 signatures it needs. 1 more cosigner must sign it too before Sparrow can finalize it.")
        );
        assert!(tx.psbt.inputs[0].partial_sigs.is_empty(), "the transaction given stays as it was");

        // then the second cosigner: its signature is the last one needed
        let b = wallet(1, Net::Testnet);
        let position = ms.position_of(b.fingerprint()).unwrap();
        let r = b.review(signed.transaction.clone(), registered.clone()).unwrap();
        assert_eq!(
            r.multisig.unwrap(),
            format!(
                "Multisig \"vault\", 2 of 3: this seed is cosigner {position} and adds its signature (2 of the 2 needed, with \
                 it). That is enough: after this, Sparrow can finalize and broadcast the transaction."
            )
        );
        let both = b.sign(signed.transaction.clone(), registered.clone()).unwrap();
        assert_eq!(both.transaction.psbt.inputs[0].partial_sigs.len(), 2);
        assert_eq!(both.multisig.as_deref(), Some("Multisig \"vault\": it now has the 2 signatures it needs. Sparrow can finalize and broadcast it."));
        // a seed that is not a cosigner has nothing to sign
        let stranger = Seed::from_words("zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong".into()).unwrap().wallet(Vec::new(), Net::Testnet).unwrap();
        assert_eq!(stranger.review(tx, registered).unwrap().inputs_to_sign, 0);
    }

    #[test]
    fn a_legacy_multisig_payment_takes_its_network_from_the_wallet_on_this_phone() {
        let ms = MultisigWallet::from_text(descriptor("sh(", "multi", "45h")).unwrap();
        let tx = spend(&ms, "m/45'");
        assert_eq!(tx.network_hint(), NetworkHint::Unknown, "m/45'/0/0 names no network");
        assert_eq!(tx.network_hint_with(Vec::new()), NetworkHint::Unknown);
        let registered = vec![RegisteredMultisig { name: "old".into(), wallet: ms.clone() }];
        assert_eq!(tx.network_hint_with(registered.clone()), NetworkHint::TestNetwork, "the wallet's keys are tpub keys");
        let c = wallet(2, Net::Testnet);
        let r = c.review(tx.clone(), registered.clone()).unwrap();
        assert!(r.multisig.unwrap().starts_with("Multisig \"old\", 2 of 3: this seed is cosigner 3 and adds"));
        assert_eq!(c.sign(tx, registered).unwrap().inputs, 1);
    }
}
