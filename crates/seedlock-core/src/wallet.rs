//! Wallet derivation from a mnemonic: master fingerprint, account xpubs,
//! output descriptors and addresses (BIP32 / 44 / 49 / 84 / 86).
//!
//! This is what SeedSigner's "export xpub" and "address explorer" screens do:
//! everything that a watch-only wallet (Sparrow, Nunchuk, Specter, ...) needs.

use crate::cbor::{self, Value};
use crate::{Error, Result};
use bip39::Mnemonic;
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::secp256k1::{self, All, Secp256k1};
use bitcoin::{Address, CompressedPublicKey, Network, NetworkKind, ScriptBuf, XOnlyPublicKey};
use unicode_normalization::UnicodeNormalization;
use zeroize::{Zeroize, Zeroizing};

/// Single-signature script types, identified by their BIP purpose number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptType {
    /// Legacy P2PKH (BIP44, addresses start with `1`).
    Pkh,
    /// Nested SegWit P2SH-P2WPKH (BIP49, addresses start with `3`).
    ShWpkh,
    /// Native SegWit P2WPKH (BIP84, `bc1q...`). The usual choice.
    Wpkh,
    /// Taproot key-path P2TR (BIP86, `bc1p...`).
    Tr,
}

impl ScriptType {
    /// All supported script types.
    pub const ALL: [ScriptType; 4] = [ScriptType::Wpkh, ScriptType::Tr, ScriptType::ShWpkh, ScriptType::Pkh];

    /// BIP43 purpose number (44, 49, 84 or 86).
    pub fn purpose(self) -> u32 {
        match self {
            ScriptType::Pkh => 44,
            ScriptType::ShWpkh => 49,
            ScriptType::Wpkh => 84,
            ScriptType::Tr => 86,
        }
    }

    /// Inverse of [`ScriptType::purpose`].
    pub fn from_purpose(p: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.purpose() == p)
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            ScriptType::Pkh => "Legacy (BIP44, P2PKH)",
            ScriptType::ShWpkh => "Nested SegWit (BIP49, P2SH-P2WPKH)",
            ScriptType::Wpkh => "Native SegWit (BIP84, P2WPKH)",
            ScriptType::Tr => "Taproot (BIP86, P2TR)",
        }
    }

    /// The UR script-expression tags of the script type, outermost first (BCR-2020-010: 400 sh, 403 pkh,
    /// 404 wpkh, 409 tr).
    fn ur_tags(self) -> &'static [u64] {
        match self {
            ScriptType::Pkh => &[403],
            ScriptType::ShWpkh => &[400, 404],
            ScriptType::Wpkh => &[404],
            ScriptType::Tr => &[409],
        }
    }

    pub fn wrap_descriptor(self, key: &str) -> String {
        match self {
            ScriptType::Pkh => format!("pkh({key})"),
            ScriptType::ShWpkh => format!("sh(wpkh({key}))"),
            ScriptType::Wpkh => format!("wpkh({key})"),
            ScriptType::Tr => format!("tr({key})"),
        }
    }
}

/// A wallet unlocked from a mnemonic (and optional BIP39 passphrase).
///
/// Holds the master private key in memory; it is wiped on drop (best effort).
pub struct Wallet {
    master: Xpriv,
    network: Network,
    secp: Secp256k1<All>,
    fingerprint: Fingerprint,
}

impl Drop for Wallet {
    fn drop(&mut self) {
        self.master.private_key.non_secure_erase();
    }
}

impl Wallet {
    /// Derive the master key. `passphrase` is the optional BIP39 "25th word"
    /// (pass `""` for none); it is NFKD-normalised as the standard requires.
    pub fn new(mnemonic: &Mnemonic, passphrase: &str, network: Network) -> Result<Self> {
        let pass: Zeroizing<String> = Zeroizing::new(passphrase.nfkd().collect());
        let mut seed = mnemonic.to_seed_normalized(&pass);
        let master = Xpriv::new_master(network, &seed).map_err(|e| Error::Wallet(e.to_string()));
        seed.zeroize();
        let master = master?;
        let secp = Secp256k1::new();
        let fingerprint = master.fingerprint(&secp);
        Ok(Self { master, network, secp, fingerprint })
    }

    /// The network this wallet was opened for.
    pub fn network(&self) -> Network {
        self.network
    }

    /// The 4-byte master key fingerprint (what SeedSigner shows on screen).
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// BIP44 coin type: 0 for mainnet, 1 for every test network.
    pub fn coin_type(&self) -> u32 {
        if self.network == Network::Bitcoin {
            0
        } else {
            1
        }
    }

    pub(crate) fn master(&self) -> &Xpriv {
        &self.master
    }

    pub(crate) fn secp(&self) -> &Secp256k1<All> {
        &self.secp
    }

    /// `m / purpose' / coin' / account'`
    pub fn account_path(&self, script: ScriptType, account: u32) -> Result<DerivationPath> {
        let h = |i: u32| {
            ChildNumber::from_hardened_idx(i).map_err(|e| Error::Wallet(format!("bad path index: {e}")))
        };
        Ok(DerivationPath::from(vec![
            h(script.purpose())?,
            h(self.coin_type())?,
            h(account)?,
        ]))
    }

    fn derive(&self, path: &DerivationPath) -> Result<Xpriv> {
        self.master
            .derive_priv(&self.secp, path)
            .map_err(|e| Error::Wallet(format!("key derivation failed: {e}")))
    }

    /// The extended public key at an arbitrary path, for example a multisig cosigner's account
    /// key at m/48'/0'/0'/2'.
    pub fn xpub_at(&self, path: &DerivationPath) -> Result<Xpub> {
        Ok(Xpub::from_priv(&self.secp, &self.derive(path)?))
    }

    /// Public key at an arbitrary derivation path from the master key.
    pub fn public_key_at(&self, path: &DerivationPath) -> Result<secp256k1::PublicKey> {
        let xpriv = self.derive(path)?;
        Ok(xpriv.private_key.public_key(&self.secp))
    }

    /// The account-level extended public key.
    pub fn account_xpub(&self, script: ScriptType, account: u32) -> Result<Xpub> {
        let path = self.account_path(script, account)?;
        let xpriv = self.derive(&path)?;
        Ok(Xpub::from_priv(&self.secp, &xpriv))
    }

    /// Key origin in descriptor notation, e.g. `[73c5da0a/84h/0h/0h]`.
    pub fn key_origin(&self, script: ScriptType, account: u32) -> Result<String> {
        let path = self.account_path(script, account)?;
        Ok(format!("[{}/{}]", self.fingerprint, path_to_string(&path)))
    }

    /// Origin + xpub, ready to paste into a watch-only wallet:
    /// `[73c5da0a/84h/0h/0h]xpub6...`
    pub fn xpub_with_origin(&self, script: ScriptType, account: u32) -> Result<String> {
        Ok(format!(
            "{}{}",
            self.key_origin(script, account)?,
            self.account_xpub(script, account)?
        ))
    }

    /// Output descriptor (with BIP380 checksum) for the receive (`change =
    /// false`) or change (`change = true`) chain.
    pub fn descriptor(&self, script: ScriptType, account: u32, change: bool) -> Result<String> {
        let key = format!(
            "{}/{}/*",
            self.xpub_with_origin(script, account)?,
            u32::from(change)
        );
        let body = script.wrap_descriptor(&key);
        let checksum = descriptor_checksum(&body)?;
        Ok(format!("{body}#{checksum}"))
    }

    /// The account as a UR `crypto-output` (BCR-2020-010), the QR code that Sparrow's wallet import reads
    /// (File > Import Wallet > Output Descriptor > Scan...): the script type's tags around a crypto-hdkey
    /// with the account key, its origin (master fingerprint and path), parent fingerprint and network, and
    /// `label` as its name, which Sparrow takes as the keystore's label (see
    /// [`crate::multisig::keystore_label`]). `UR:CRYPTO-OUTPUT/...` in capitals, which QR codes store
    /// compactly.
    pub fn crypto_output_ur(&self, script: ScriptType, account: u32, label: &str) -> Result<String> {
        let path = self.account_path(script, account)?;
        let xpub = self.account_xpub(script, account)?;
        let u = Value::Uint;
        let fingerprint = |f: Fingerprint| u(u64::from(u32::from_be_bytes(f.to_bytes())));
        let steps: &[ChildNumber] = path.as_ref();
        let mut components = Vec::with_capacity(steps.len() * 2);
        for step in steps {
            let (index, hardened) = match *step {
                ChildNumber::Hardened { index } => (index, true),
                ChildNumber::Normal { index } => (index, false),
            };
            components.push(u(u64::from(index)));
            components.push(Value::Bool(hardened));
        }
        let origin = Value::Map(vec![(u(1), Value::Array(components)), (u(2), fingerprint(self.fingerprint)), (u(3), u(steps.len() as u64))]);
        let network = if xpub.network == NetworkKind::Main { 0 } else { 1 };
        let mut hdkey = vec![
            (u(3), Value::Bytes(xpub.public_key.serialize().to_vec())),
            (u(4), Value::Bytes(xpub.chain_code.to_bytes().to_vec())),
            (u(5), Value::tag(305, Value::Map(vec![(u(1), u(0)), (u(2), u(network))]))),
            (u(6), Value::tag(304, origin)),
            (u(8), fingerprint(xpub.parent_fingerprint)),
        ];
        if !label.trim().is_empty() {
            hdkey.push((u(9), Value::Text(label.trim().to_string())));
        }
        let mut output = Value::tag(303, Value::Map(hdkey));
        for tag in script.ur_tags().iter().rev() {
            output = Value::tag(*tag, output);
        }
        Ok(ur::ur::encode(&cbor::encode(&output), &ur::Type::Custom("crypto-output")).to_uppercase())
    }

    /// Address `m / purpose' / coin' / account' / chain / index`.
    pub fn address(&self, script: ScriptType, account: u32, change: bool, index: u32) -> Result<Address> {
        let xpub = self.account_xpub(script, account)?;
        let idx = |i: u32| {
            ChildNumber::from_normal_idx(i).map_err(|e| Error::Wallet(format!("bad address index: {e}")))
        };
        let child = xpub
            .derive_pub(&self.secp, &[idx(u32::from(change))?, idx(index)?])
            .map_err(|e| Error::Wallet(e.to_string()))?;
        Ok(self.address_for_key(script, &child.public_key))
    }

    /// The address a public key would have under a given script type.
    pub fn address_for_key(&self, script: ScriptType, pk: &secp256k1::PublicKey) -> Address {
        let cpk = CompressedPublicKey(*pk);
        match script {
            ScriptType::Pkh => Address::p2pkh(cpk, self.network),
            ScriptType::ShWpkh => Address::p2shwpkh(&cpk, self.network),
            ScriptType::Wpkh => Address::p2wpkh(&cpk, self.network),
            ScriptType::Tr => Address::p2tr(&self.secp, XOnlyPublicKey::from(*pk), None, self.network),
        }
    }

    /// The `scriptPubKey` a public key would have under a given script type.
    pub fn script_pubkey(&self, script: ScriptType, pk: &secp256k1::PublicKey) -> ScriptBuf {
        self.address_for_key(script, pk).script_pubkey()
    }
}

/// `84h/0h/0h` (no leading `m/`), the notation used inside descriptors.
pub fn path_to_string(path: &DerivationPath) -> String {
    path.into_iter()
        .map(|c| match c {
            ChildNumber::Normal { index } => index.to_string(),
            ChildNumber::Hardened { index } => format!("{index}h"),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// `m/84'/0'/0'/0/3`, the notation people are used to seeing.
pub fn path_to_display(path: &DerivationPath) -> String {
    let parts: Vec<String> = path
        .into_iter()
        .map(|c| match c {
            ChildNumber::Normal { index } => index.to_string(),
            ChildNumber::Hardened { index } => format!("{index}'"),
        })
        .collect();
    if parts.is_empty() {
        "m".to_string()
    } else {
        format!("m/{}", parts.join("/"))
    }
}

/// BIP380 descriptor checksum (the 8 characters after `#`).
pub fn descriptor_checksum(desc: &str) -> Result<String> {
    const INPUT_CHARSET: &str =
        "0123456789()[],'/*abcdefgh@:$%{}IJKLMNOPQRSTUVWXYZ&+-.;<=>?!^_|~ijklmnopqrstuvwxyzABCDEFGH`#\"\\ ";
    const CHECKSUM_CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    const GENERATOR: [u64; 5] = [
        0xf5dee51989,
        0xa9fdca3312,
        0x1bab10e32d,
        0x3706b1677a,
        0x644d626ffd,
    ];

    fn polymod(c: u64, val: u64) -> u64 {
        let c0 = c >> 35;
        let mut c = ((c & 0x7_ffff_ffff) << 5) ^ val;
        for (i, g) in GENERATOR.iter().enumerate() {
            if (c0 >> i) & 1 == 1 {
                c ^= g;
            }
        }
        c
    }

    let mut c: u64 = 1;
    let mut cls: u64 = 0;
    let mut clscount = 0;
    for ch in desc.chars() {
        let pos = INPUT_CHARSET
            .find(ch)
            .ok_or_else(|| Error::Wallet(format!("invalid character '{ch}' in descriptor")))? as u64;
        c = polymod(c, pos & 31);
        cls = cls * 3 + (pos >> 5);
        clscount += 1;
        if clscount == 3 {
            c = polymod(c, cls);
            cls = 0;
            clscount = 0;
        }
    }
    if clscount > 0 {
        c = polymod(c, cls);
    }
    for _ in 0..8 {
        c = polymod(c, 0);
    }
    c ^= 1;
    Ok((0..8)
        .map(|j| CHECKSUM_CHARSET[((c >> (5 * (7 - j))) & 31) as usize] as char)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABANDON: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn wallet(network: Network, passphrase: &str) -> Wallet {
        Wallet::new(&crate::mnemonic::parse(ABANDON).unwrap(), passphrase, network).unwrap()
    }

    fn addr(w: &Wallet, s: ScriptType, change: bool, i: u32) -> String {
        w.address(s, 0, change, i).unwrap().to_string()
    }

    #[test]
    fn master_fingerprint_of_the_standard_test_seed() {
        assert_eq!(wallet(Network::Bitcoin, "").fingerprint().to_string(), "73c5da0a");
    }

    #[test]
    fn bip84_test_vectors() {
        let w = wallet(Network::Bitcoin, "");
        assert_eq!(addr(&w, ScriptType::Wpkh, false, 0), "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
        assert_eq!(addr(&w, ScriptType::Wpkh, false, 1), "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g");
        assert_eq!(addr(&w, ScriptType::Wpkh, true, 0), "bc1q8c6fshw2dlwun7ekn9qwf37cu2rn755upcp6el");
    }

    #[test]
    fn bip86_test_vectors() {
        let w = wallet(Network::Bitcoin, "");
        assert_eq!(addr(&w, ScriptType::Tr, false, 0), "bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr");
        assert_eq!(addr(&w, ScriptType::Tr, false, 1), "bc1p4qhjn9zdvkux4e44uhx8tc55attvtyu358kutcqkudyccelu0was9fqzwh");
        assert_eq!(addr(&w, ScriptType::Tr, true, 0), "bc1p3qkhfews2uk44qtvauqyr2ttdsw7svhkl9nkm9s9c3x4ax5h60wqwruhk7");
    }

    #[test]
    fn bip49_and_bip44_vectors() {
        let t = wallet(Network::Testnet, "");
        assert_eq!(addr(&t, ScriptType::ShWpkh, false, 0), "2Mww8dCYPUpKHofjgcXcBCEGmniw9CoaiD2");
        let m = wallet(Network::Bitcoin, "");
        assert_eq!(addr(&m, ScriptType::Pkh, false, 0), "1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA");
    }

    #[test]
    fn account_xpub_and_origin() {
        let w = wallet(Network::Bitcoin, "");
        assert_eq!(
            w.account_xpub(ScriptType::Wpkh, 0).unwrap().to_string(),
            "xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYVUhLv1VMrjPC7PW6V"
        );
        assert_eq!(w.key_origin(ScriptType::Wpkh, 0).unwrap(), "[73c5da0a/84h/0h/0h]");
        let t = wallet(Network::Testnet, "");
        assert!(t.account_xpub(ScriptType::Wpkh, 0).unwrap().to_string().starts_with("tpub"));
        assert_eq!(t.key_origin(ScriptType::Wpkh, 0).unwrap(), "[73c5da0a/84h/1h/0h]");
    }

    #[test]
    fn the_wallet_for_sparrow_as_a_ur_crypto_output_named_for_its_keystore() {
        let w = wallet(Network::Bitcoin, "");
        let text = w.crypto_output_ur(ScriptType::Wpkh, 0, "My wallet").unwrap();
        assert!(text.starts_with("UR:CRYPTO-OUTPUT/") && text.matches('/').count() == 1, "one single-part code: {text}");
        let (_, bytes) = ur::ur::decode(&text.to_lowercase()).unwrap();
        let Value::Tag(404, inner) = cbor::decode(&bytes).unwrap() else { panic!("wpkh() first") };
        let Value::Tag(303, hdkey) = *inner else { panic!("then the key") };
        assert_eq!(hdkey.get(9), Some(&Value::Text("My wallet".into())), "the keystore's label in Sparrow");
        let Some(Value::Tag(304, origin)) = hdkey.get(6) else { panic!("the key's origin") };
        let path: Vec<Value> = [84, 0, 0].iter().flat_map(|&i| [Value::Uint(i), Value::Bool(true)]).collect();
        assert_eq!(origin.get(1), Some(&Value::Array(path)));
        assert_eq!(origin.get(2).and_then(Value::as_uint), Some(0x73c5_da0a), "the master fingerprint");
        // the fields rebuild BIP84's account xpub
        let xpub = w.account_xpub(ScriptType::Wpkh, 0).unwrap();
        assert_eq!(hdkey.get(3).and_then(Value::as_bytes), Some(&xpub.public_key.serialize()[..]));
        assert_eq!(hdkey.get(4).and_then(Value::as_bytes), Some(&xpub.chain_code.to_bytes()[..]));
        assert_eq!(hdkey.get(8).and_then(Value::as_uint), Some(u64::from(u32::from_be_bytes(xpub.parent_fingerprint.to_bytes()))));
        // each script type has its tags, and a test network says so
        let t = wallet(Network::Testnet, "");
        for (script, tags) in [(ScriptType::ShWpkh, vec![400, 404]), (ScriptType::Pkh, vec![403]), (ScriptType::Tr, vec![409])] {
            let (_, bytes) = ur::ur::decode(&t.crypto_output_ur(script, 0, "x").unwrap().to_lowercase()).unwrap();
            let mut value = cbor::decode(&bytes).unwrap();
            let mut found = Vec::new();
            while let Value::Tag(tag, inner) = value {
                if tag == 303 {
                    let Some(Value::Tag(305, use_info)) = inner.get(5) else { panic!("the network") };
                    assert_eq!(use_info.get(2).and_then(Value::as_uint), Some(1), "a test network");
                    break;
                }
                found.push(tag);
                value = *inner;
            }
            assert_eq!(found, tags, "{script:?}");
        }
    }

    #[test]
    fn descriptors_carry_a_valid_checksum() {
        let w = wallet(Network::Bitcoin, "");
        for s in ScriptType::ALL {
            for change in [false, true] {
                let d = w.descriptor(s, 0, change).unwrap();
                let (body, sum) = d.split_once('#').unwrap();
                assert_eq!(sum, descriptor_checksum(body).unwrap());
                assert_eq!(sum.len(), 8);
                assert!(body.contains("[73c5da0a/"));
                assert!(body.ends_with(&format!("/{}/*)", u32::from(change))) || body.ends_with(&format!("/{}/*))", u32::from(change))));
            }
        }
    }

    #[test]
    fn descriptor_checksum_known_vectors() {
        // BIP380 / Bitcoin Core documentation examples
        assert_eq!(descriptor_checksum("raw(deadbeef)").unwrap(), "89f8spxm");
        assert_eq!(
            descriptor_checksum("pkh([d34db33f/44'/0'/0']xpub6ERApfZwUNrhLCkDtcHTcxd75RbzS1ed54G1LkBUHQVHQKqhMkhgbmJbZRkrgZw4koxb5JaHWkY4ALHY2grBGRjaDMzQLcgJvLJuZZvRcEL/1/*)").unwrap(),
            "ml40v0wf"
        );
        assert!(descriptor_checksum("wpkh(é)").is_err());
    }

    #[test]
    fn bip39_passphrase_creates_a_different_wallet() {
        let plain = wallet(Network::Bitcoin, "");
        let with_pass = wallet(Network::Bitcoin, "TREZOR");
        assert_ne!(plain.fingerprint(), with_pass.fingerprint());
        // NFKD normalisation: composed and decomposed spellings are the same wallet
        let a = wallet(Network::Bitcoin, "caf\u{e9}");
        let b = wallet(Network::Bitcoin, "cafe\u{301}");
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn path_formatting() {
        let w = wallet(Network::Bitcoin, "");
        let p = w.account_path(ScriptType::Tr, 2).unwrap();
        assert_eq!(path_to_string(&p), "86h/0h/2h");
        assert_eq!(path_to_display(&p), "m/86'/0'/2'");
    }
}
