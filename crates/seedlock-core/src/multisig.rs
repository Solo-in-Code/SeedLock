//! Multisig wallets: several keys, a number of which must sign (for example 2 of 3). SeedLock
//! supports the three kinds Sparrow makes: native SegWit `wsh(...)`, nested SegWit `sh(wsh(...))`
//! and legacy `sh(...)`, each with its keys sorted (`sortedmulti`, BIP 67, Sparrow's default) or in
//! the order the descriptor gives (`multi`). Usual key paths: BIP 48 (m/48'/0'/0'/2' native,
//! m/48'/0'/0'/1' nested) and BIP 45 (m/45', legacy). Taproot multisig is not supported (Sparrow
//! cannot make it either).
//!
//! A multisig wallet is *registered* once, from its output descriptor: as text (in Sparrow: File >
//! Export Wallet > Output Descriptor) or as the UR `crypto-output` QR code that SeedSigner reads. With
//! every cosigner's key known, SeedLock rebuilds the exact script of each input and change output of
//! a transaction, so an output that holds this seed's key but someone else's other keys is never
//! taken for change. This module also writes one cosigner's key as the UR `crypto-account` QR code
//! that Sparrow scans from SeedSigner (Airgapped Hardware Wallet > SeedSigner).

use crate::cbor::{self, Value};
use crate::wallet::{descriptor_checksum, path_to_string};
use crate::{Error, Result};
use bitcoin::bip32::{ChainCode, ChildNumber, DerivationPath, Fingerprint, Xpub};
use bitcoin::opcodes::all::OP_CHECKMULTISIG;
use bitcoin::script::Builder;
use bitcoin::secp256k1::{self, Secp256k1};
use bitcoin::{Address, Network, NetworkKind, PublicKey, ScriptBuf};
use std::str::FromStr;

/// The most keys a multisig wallet may have here (a legacy P2SH script of 15 keys still fits).
pub const MAX_COSIGNERS: usize = 15;

fn err(text: impl Into<String>) -> Error {
    Error::Multisig(text.into())
}

/// How a multisig wallet's script is wrapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultisigScript {
    /// Native SegWit, `wsh(...)`: P2WSH, addresses bc1q... / tb1q...
    Wsh,
    /// Nested SegWit, `sh(wsh(...))`: P2SH-P2WSH, addresses 3... / 2...
    ShWsh,
    /// Legacy, `sh(...)`: P2SH, addresses 3... / 2...
    Sh,
}

impl MultisigScript {
    pub const ALL: [MultisigScript; 3] = [MultisigScript::Wsh, MultisigScript::ShWsh, MultisigScript::Sh];

    /// The name Sparrow gives it in its "Script Type" list.
    pub fn sparrow_name(self) -> &'static str {
        match self {
            MultisigScript::Wsh => "Native Segwit (P2WSH)",
            MultisigScript::ShWsh => "Nested Segwit (P2SH-P2WSH)",
            MultisigScript::Sh => "Legacy (P2SH)",
        }
    }

    /// The usual account key path of a cosigner: BIP 48 for SegWit, BIP 45 for legacy.
    pub fn account_path(self, mainnet: bool) -> DerivationPath {
        let coin = if mainnet { 0 } else { 1 };
        let text = match self {
            MultisigScript::Wsh => format!("m/48'/{coin}'/0'/2'"),
            MultisigScript::ShWsh => format!("m/48'/{coin}'/0'/1'"),
            MultisigScript::Sh => "m/45'".to_string(),
        };
        text.parse().expect("a valid key path")
    }

    fn wrap(self, inner: &str) -> String {
        match self {
            MultisigScript::Wsh => format!("wsh({inner})"),
            MultisigScript::ShWsh => format!("sh(wsh({inner}))"),
            MultisigScript::Sh => format!("sh({inner})"),
        }
    }

    /// The UR script-expression tags, outermost first (BCR-2020-010: 400 sh, 401 wsh).
    fn tags(self) -> &'static [u64] {
        match self {
            MultisigScript::Wsh => &[401],
            MultisigScript::ShWsh => &[400, 401],
            MultisigScript::Sh => &[400],
        }
    }
}

/// One key of a multisig wallet: where it comes from (fingerprint and path) and its account xpub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cosigner {
    pub fingerprint: Fingerprint,
    pub path: DerivationPath,
    pub xpub: Xpub,
}

/// A multisig wallet: `threshold` of the cosigners must sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultisigWallet {
    pub threshold: usize,
    pub cosigners: Vec<Cosigner>,
    pub script: MultisigScript,
    /// Keys sorted as BIP 67 says (`sortedmulti`), or in the descriptor's order (`multi`).
    pub sorted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Branch {
    Both,
    Receive,
    Change,
}

impl MultisigWallet {
    /// Read a multisig output descriptor, for example `wsh(sortedmulti(2,[fp/48h/0h/0h/2h]xpub.../<0;1>/*,...))#checksum`,
    /// `sh(wsh(...))`, `sh(...)`, with `sortedmulti` or `multi`. A whole exported file is fine (comment
    /// lines are skipped); the receive-only (`.../0/*`) and change-only forms are accepted, and all
    /// descriptors in the text must be of the same wallet.
    pub fn from_descriptor_text(text: &str) -> Result<MultisigWallet> {
        let candidates: Vec<&str> =
            text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#') && l.contains("multi")).collect();
        if candidates.is_empty() {
            return Err(err(
                "no multisig output descriptor was found. It looks like wsh(sortedmulti(2,[fingerprint/path]xpub...,...))#checksum",
            ));
        }
        let mut found: Option<MultisigWallet> = None;
        let mut first_error = None;
        for line in candidates {
            let start = ["sh(wsh(", "wsh(", "tr(", "sh(", "sortedmulti(", "multi("].iter().filter_map(|p| line.find(p)).min().unwrap_or(0);
            match Self::parse_one(&line[start..]) {
                Ok(wallet) => match &found {
                    None => found = Some(wallet),
                    Some(known) if known.same_as(&wallet) => {}
                    Some(_) => {
                        return Err(err("the text holds the descriptors of more than one wallet: give SeedLock one wallet at a time"))
                    }
                },
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        match (found, first_error) {
            (Some(wallet), _) => Ok(wallet),
            (None, Some(e)) => Err(e),
            (None, None) => Err(err("no multisig output descriptor was found")),
        }
    }

    fn parse_one(desc: &str) -> Result<MultisigWallet> {
        let desc = desc.trim();
        let (body, checksum) = match desc.rsplit_once('#') {
            Some((body, checksum)) => (body, Some(checksum)),
            None => (desc, None),
        };
        if let Some(checksum) = checksum {
            if checksum != descriptor_checksum(body)? {
                return Err(err("the descriptor's checksum (after #) does not match: it was changed or mistyped"));
            }
        }
        let incomplete = || err("the descriptor is incomplete: its brackets do not close");
        let taproot = || err("this is a Taproot wallet; SeedLock supports native, nested and legacy multisig, not Taproot multisig");
        let (script, rest) = if let Some(r) = body.strip_prefix("wsh(") {
            (MultisigScript::Wsh, r.strip_suffix(')').ok_or_else(incomplete)?)
        } else if let Some(r) = body.strip_prefix("sh(wsh(") {
            (MultisigScript::ShWsh, r.strip_suffix("))").ok_or_else(incomplete)?)
        } else if let Some(r) = body.strip_prefix("sh(") {
            (MultisigScript::Sh, r.strip_suffix(')').ok_or_else(incomplete)?)
        } else if body.starts_with("tr(") {
            return Err(taproot());
        } else {
            return Err(err("this is not a multisig output descriptor: it should start with wsh(, sh(wsh( or sh("));
        };
        let (sorted, inner) = if let Some(r) = rest.strip_prefix("sortedmulti(") {
            (true, r.strip_suffix(')').ok_or_else(incomplete)?)
        } else if let Some(r) = rest.strip_prefix("multi(") {
            (false, r.strip_suffix(')').ok_or_else(incomplete)?)
        } else if rest.starts_with("sortedmulti_a(") || rest.starts_with("multi_a(") {
            return Err(taproot());
        } else {
            return Err(err("this is not a multisig descriptor: inside it there should be sortedmulti(...) or multi(...)"));
        };
        let mut parts = inner.split(',');
        let threshold: usize = parts
            .next()
            .unwrap_or("")
            .trim()
            .parse()
            .map_err(|_| err("the number of signatures needed (the first number in it) cannot be read"))?;
        let mut cosigners = Vec::new();
        let mut branch = None;
        for (i, key) in parts.enumerate() {
            let (cosigner, key_branch) = parse_key(key.trim()).map_err(|why| err(format!("key {}: {why}", i + 1)))?;
            if branch.is_some_and(|b| b != key_branch) {
                return Err(err("the keys do not all end the same way (/<0;1>/*, /0/* or /1/*)"));
            }
            branch = Some(key_branch);
            cosigners.push(cosigner);
        }
        Self::checked(threshold, cosigners, script, sorted)
    }

    /// The rules every multisig wallet must meet, however it was read.
    fn checked(threshold: usize, cosigners: Vec<Cosigner>, script: MultisigScript, sorted: bool) -> Result<MultisigWallet> {
        let n = cosigners.len();
        if !(2..=MAX_COSIGNERS).contains(&n) {
            return Err(err(format!("a multisig wallet has 2 to {MAX_COSIGNERS} keys; this one has {n}")));
        }
        if threshold == 0 || threshold > n {
            return Err(err(format!("{threshold} signatures out of {n} keys cannot be right")));
        }
        for (i, a) in cosigners.iter().enumerate() {
            let same_key = |b: &Cosigner| b.xpub.public_key == a.xpub.public_key && b.xpub.chain_code == a.xpub.chain_code;
            if cosigners[..i].iter().any(|b| same_key(b) || (b.fingerprint == a.fingerprint && b.path == a.path)) {
                return Err(err(format!("key {} appears twice", i + 1)));
            }
        }
        if cosigners.iter().any(|c| c.xpub.network != cosigners[0].xpub.network) {
            return Err(err("some keys are for mainnet and others for a test network"));
        }
        Ok(MultisigWallet { threshold, cosigners, script, sorted })
    }

    /// Read the UR `crypto-output` a QR code holds (BCR-2020-010, as SeedSigner and Sparrow write it):
    /// the script tags (sh, wsh), then `sortedmulti` or `multi`, then the cosigners' keys (crypto-hdkey
    /// with their origin). Master keys and private keys are refused.
    pub fn from_crypto_output(value: &Value) -> Result<MultisigWallet> {
        let not_multisig = || err("this QR code does not hold a multisig wallet's descriptor");
        let mut v = value;
        if let Value::Tag(308, inner) = v {
            v = inner;
        }
        let (script, rest) = match v {
            Value::Tag(401, inner) => (MultisigScript::Wsh, &**inner),
            Value::Tag(400, inner) => match &**inner {
                Value::Tag(401, inner2) => (MultisigScript::ShWsh, &**inner2),
                other => (MultisigScript::Sh, other),
            },
            Value::Tag(409, _) => return Err(err("this is a Taproot wallet; SeedLock does not support Taproot multisig")),
            _ => return Err(not_multisig()),
        };
        let (sorted, multi) = match rest {
            Value::Tag(407, m) => (true, &**m),
            Value::Tag(406, m) => (false, &**m),
            _ => return Err(not_multisig()),
        };
        let threshold = multi.get(1).and_then(Value::as_uint).ok_or_else(|| err("the number of signatures needed is missing"))?;
        let keys = multi.get(2).and_then(Value::as_array).ok_or_else(|| err("the keys are missing"))?;
        let cosigners = keys
            .iter()
            .enumerate()
            .map(|(i, key)| match key {
                Value::Tag(303, hdkey) => cosigner_from_hdkey(hdkey).map_err(|why| err(format!("key {}: {why}", i + 1))),
                _ => Err(err(format!("key {} is not an extended key with its origin", i + 1))),
            })
            .collect::<Result<Vec<_>>>()?;
        Self::checked(usize::try_from(threshold).unwrap_or(usize::MAX), cosigners, script, sorted)
    }

    /// The same wallet: same kind, same threshold, same keys (in the same order, when it matters).
    pub fn same_as(&self, other: &MultisigWallet) -> bool {
        if self.threshold != other.threshold || self.script != other.script || self.sorted != other.sorted || self.cosigners.len() != other.cosigners.len() {
            return false;
        }
        let key = |c: &Cosigner| (c.fingerprint, c.path.clone(), c.xpub.public_key.serialize(), c.xpub.chain_code.to_bytes());
        let mut a: Vec<_> = self.cosigners.iter().map(key).collect();
        let mut b: Vec<_> = other.cosigners.iter().map(key).collect();
        if self.sorted {
            a.sort();
            b.sort();
        }
        a == b
    }

    /// "2 of 3".
    pub fn label(&self) -> String {
        format!("{} of {}", self.threshold, self.cosigners.len())
    }

    /// True for a mainnet wallet (xpub keys), false for a test network (tpub keys).
    pub fn is_mainnet(&self) -> bool {
        self.cosigners[0].xpub.network == NetworkKind::Main
    }

    /// The whole wallet as one descriptor, the way Sparrow writes it (`.../<0;1>/*`, with its checksum).
    pub fn descriptor(&self) -> Result<String> {
        let keys: Vec<String> = self
            .cosigners
            .iter()
            .map(|c| format!("[{}/{}]{}/<0;1>/*", c.fingerprint, path_to_string(&c.path), c.xpub))
            .collect();
        let inner = format!("{}({},{})", if self.sorted { "sortedmulti" } else { "multi" }, self.threshold, keys.join(","));
        let body = self.script.wrap(&inner);
        let checksum = descriptor_checksum(&body)?;
        Ok(format!("{body}#{checksum}"))
    }

    /// The cosigners' public keys at `chain/index` (0 = receive, 1 = change), in script order: sorted
    /// as BIP 67 says for `sortedmulti`, as the descriptor gives them for `multi`.
    pub fn keys_at(&self, chain: u32, index: u32) -> Result<Vec<PublicKey>> {
        let secp = Secp256k1::verification_only();
        let steps = [
            ChildNumber::from_normal_idx(chain).map_err(|e| err(e.to_string()))?,
            ChildNumber::from_normal_idx(index).map_err(|e| err(e.to_string()))?,
        ];
        let mut keys = self
            .cosigners
            .iter()
            .map(|c| c.xpub.derive_pub(&secp, &steps).map(|x| PublicKey::new(x.public_key)))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| err(format!("key derivation failed: {e}")))?;
        if self.sorted {
            keys.sort_by(|a, b| a.inner.serialize().cmp(&b.inner.serialize()));
        }
        Ok(keys)
    }

    /// The multisig script at `chain/index`: `M <keys> N OP_CHECKMULTISIG`. It is the witness script of
    /// a native or nested SegWit wallet and the redeem script of a legacy one.
    pub fn witness_script(&self, chain: u32, index: u32) -> Result<ScriptBuf> {
        let keys = self.keys_at(chain, index)?;
        let mut builder = Builder::new().push_int(self.threshold as i64);
        for key in &keys {
            builder = builder.push_key(key);
        }
        Ok(builder.push_int(keys.len() as i64).push_opcode(OP_CHECKMULTISIG).into_script())
    }

    /// The scripts a PSBT carries for `chain/index`: (redeem script, witness script).
    pub fn psbt_scripts(&self, chain: u32, index: u32) -> Result<(Option<ScriptBuf>, Option<ScriptBuf>)> {
        let multisig = self.witness_script(chain, index)?;
        Ok(match self.script {
            MultisigScript::Wsh => (None, Some(multisig)),
            MultisigScript::ShWsh => (Some(ScriptBuf::new_p2wsh(&multisig.wscript_hash())), Some(multisig)),
            MultisigScript::Sh => (Some(multisig), None),
        })
    }

    /// The output script at `chain/index`.
    pub fn script_pubkey(&self, chain: u32, index: u32) -> Result<ScriptBuf> {
        let multisig = self.witness_script(chain, index)?;
        Ok(match self.script {
            MultisigScript::Wsh => ScriptBuf::new_p2wsh(&multisig.wscript_hash()),
            MultisigScript::ShWsh => ScriptBuf::new_p2sh(&ScriptBuf::new_p2wsh(&multisig.wscript_hash()).script_hash()),
            MultisigScript::Sh => ScriptBuf::new_p2sh(&multisig.script_hash()),
        })
    }

    /// The address at `chain/index`.
    pub fn address(&self, chain: u32, index: u32, network: Network) -> Result<Address> {
        Address::from_script(&self.script_pubkey(chain, index)?, network).map_err(|e| err(format!("no address for this script: {e}")))
    }

    /// Where a key path of one of the cosigners points inside this wallet: `(chain, index)`. The
    /// path must be that cosigner's account path followed by a chain (0 or 1) and an index.
    pub fn locate(&self, fingerprint: Fingerprint, path: &DerivationPath) -> Option<(u32, u32)> {
        let full: &[ChildNumber] = path.as_ref();
        self.cosigners.iter().filter(|c| c.fingerprint == fingerprint).find_map(|c| {
            let base: &[ChildNumber] = c.path.as_ref();
            if full.len() != base.len() + 2 || full[..base.len()] != *base {
                return None;
            }
            match (full[base.len()], full[base.len() + 1]) {
                (ChildNumber::Normal { index: chain }, ChildNumber::Normal { index }) if chain <= 1 => Some((chain, index)),
                _ => None,
            }
        })
    }

    /// Which cosigner (0-based) has this fingerprint.
    pub fn position(&self, fingerprint: Fingerprint) -> Option<usize> {
        self.cosigners.iter().position(|c| c.fingerprint == fingerprint)
    }
}

/// One key of a descriptor: `[fingerprint/path]xpub/<0;1>/*`.
fn parse_key(key: &str) -> std::result::Result<(Cosigner, Branch), String> {
    let rest = key.strip_prefix('[').ok_or("its origin, [fingerprint/path], is missing; SeedLock needs it to find each key")?;
    let (origin, rest) = rest.split_once(']').ok_or("its origin is not closed with ]")?;
    let (fp, path_text) = origin.split_once('/').ok_or("its origin has no key path")?;
    let fingerprint = Fingerprint::from_str(fp).map_err(|_| format!("\"{fp}\" is not a fingerprint (8 hexadecimal characters)"))?;
    let path = DerivationPath::from_str(&format!("m/{}", path_text.replace('H', "h")))
        .map_err(|_| format!("\"{path_text}\" is not a key path"))?;
    let (xpub_text, tail) = rest.split_once('/').ok_or("the key has no /<0;1>/* after it")?;
    let branch = match tail {
        "<0;1>/*" => Branch::Both,
        "0/*" => Branch::Receive,
        "1/*" => Branch::Change,
        other => return Err(format!("\"/{other}\" after the key is not supported (it should be /<0;1>/*)")),
    };
    let xpub = Xpub::from_str(xpub_text).map_err(|_| "its extended public key cannot be read (SeedLock reads xpub and tpub keys)".to_string())?;
    check_origin(&path, &xpub)?;
    Ok((Cosigner { fingerprint, path, xpub }, branch))
}

/// A key's origin path and the key must belong together: as many steps down, the same network.
fn check_origin(path: &DerivationPath, xpub: &Xpub) -> std::result::Result<(), String> {
    if usize::from(xpub.depth) != path.len() {
        return Err(format!(
            "its key path has {} steps, but its key sits {} steps down: they do not belong together",
            path.len(),
            xpub.depth
        ));
    }
    let steps: &[ChildNumber] = path.as_ref();
    let path_says_test = match (steps.first(), steps.get(1)) {
        (Some(ChildNumber::Hardened { index: 48 | 44 | 49 | 84 | 86 }), Some(ChildNumber::Hardened { index: 1 })) => Some(true),
        (Some(ChildNumber::Hardened { index: 48 | 44 | 49 | 84 | 86 }), Some(ChildNumber::Hardened { index: 0 })) => Some(false),
        _ => None,
    };
    if path_says_test.is_some_and(|test| test != (xpub.network == NetworkKind::Test)) {
        return Err("its key path and its key are for different networks (mainnet and a test network)".to_string());
    }
    Ok(())
}

/// A cosigner from a UR crypto-hdkey (BCR-2020-007): its public key and chain code, its origin (the
/// master fingerprint and the path to the account key) and, when given, its parent fingerprint and
/// network, from which the xpub is rebuilt.
fn cosigner_from_hdkey(hdkey: &Value) -> std::result::Result<Cosigner, String> {
    // Fields 1 and 2 are flags (is-master, is-private). Sparrow writes "is-private: false" for
    // every public key, so a flag counts only when it is true; anything that is not plainly false is
    // taken as a private or master key, and refused.
    match hdkey.get(1) {
        None | Some(Value::Bool(false)) => {}
        Some(_) => return Err("it is a master key, not a cosigner's account key".to_string()),
    }
    match hdkey.get(2) {
        None | Some(Value::Bool(false)) => {}
        Some(_) => return Err("it holds a private key; SeedLock never takes one from a QR code".to_string()),
    }
    let key = hdkey.get(3).and_then(Value::as_bytes).ok_or("its public key is missing")?;
    let public_key = secp256k1::PublicKey::from_slice(key).map_err(|_| "its public key is not valid".to_string())?;
    let chain_code: [u8; 32] =
        hdkey.get(4).and_then(Value::as_bytes).and_then(|b| b.try_into().ok()).ok_or("its chain code is missing")?;
    let network = match hdkey.get(5) {
        Some(Value::Tag(305, info)) => match info.get(2).and_then(Value::as_uint) {
            None | Some(0) => NetworkKind::Main,
            Some(1) => NetworkKind::Test,
            Some(_) => return Err("its network is unknown".to_string()),
        },
        None => NetworkKind::Main,
        Some(_) => return Err("its network information cannot be read".to_string()),
    };
    let Some(Value::Tag(304, origin)) = hdkey.get(6) else { return Err("its origin (master fingerprint and key path) is missing".to_string()) };
    let components = origin.get(1).and_then(Value::as_array).ok_or("its key path is missing")?;
    if components.len() % 2 != 0 {
        return Err("its key path cannot be read".to_string());
    }
    let mut steps = Vec::new();
    for pair in components.chunks(2) {
        let (Value::Uint(index), Value::Bool(hardened)) = (&pair[0], &pair[1]) else {
            return Err("its key path holds something other than plain steps".to_string());
        };
        let index = u32::try_from(*index).map_err(|_| "a step of its key path is too large".to_string())?;
        steps.push(if *hardened { ChildNumber::from_hardened_idx(index) } else { ChildNumber::from_normal_idx(index) }.map_err(|e| e.to_string())?);
    }
    let fingerprint = origin
        .get(2)
        .and_then(Value::as_uint)
        .and_then(|f| u32::try_from(f).ok())
        .map(|f| Fingerprint::from(f.to_be_bytes()))
        .ok_or("its master fingerprint is missing")?;
    if let Some(depth) = origin.get(3).and_then(Value::as_uint) {
        if depth as usize != steps.len() {
            return Err("its key path and its depth do not agree".to_string());
        }
    }
    if let Some(Value::Tag(304, children)) = hdkey.get(7) {
        let parts = children.get(1).and_then(Value::as_array).unwrap_or(&[]);
        let wildcard_last = parts.len() >= 2 && parts[parts.len() - 2] == Value::Array(vec![]);
        if !parts.is_empty() && !wildcard_last {
            return Err("it uses key paths after the account key that SeedLock does not support (it expects /<0;1>/*)".to_string());
        }
    }
    let parent_fingerprint = hdkey
        .get(8)
        .and_then(Value::as_uint)
        .and_then(|f| u32::try_from(f).ok())
        .map(|f| Fingerprint::from(f.to_be_bytes()))
        .unwrap_or_default();
    let child_number = *steps.last().ok_or("its key path is empty: that is a master key")?;
    let depth = u8::try_from(steps.len()).map_err(|_| "its key path is too long".to_string())?;
    let xpub = Xpub { network, depth, parent_fingerprint, child_number, public_key, chain_code: ChainCode::from(chain_code) };
    let path = DerivationPath::from(steps);
    check_origin(&path, &xpub)?;
    Ok(Cosigner { fingerprint, path, xpub })
}

/// The label Sparrow gives the keystore when it scans this seed's key: the backup's name, cut to the
/// 16 characters Sparrow allows for a keystore label (counted as Sparrow counts them, in UTF-16 units).
/// With no usable name: "SeedLock" and the start of the master fingerprint ("SeedLock 73c5").
pub fn keystore_label(name: &str, master: Fingerprint) -> String {
    let mut label = String::new();
    let mut units = 0;
    for c in name.trim().chars().filter(|c| !c.is_control()) {
        units += c.len_utf16();
        if units > 16 {
            break;
        }
        label.push(c);
    }
    let label = label.trim_end();
    if label.is_empty() { format!("SeedLock {}", &master.to_string()[..4]) } else { label.to_string() }
}

/// One cosigner's key as a UR `crypto-account` (BCR-2020-015), the standard format Sparrow scans with
/// the camera button of a keystore's xpub field: the master fingerprint, then one output (the script
/// tags of `script` around a crypto-hdkey with its origin, parent fingerprint, network and name,
/// `label`, which Sparrow takes as the keystore's label: see [`keystore_label`]). Returns the CBOR bytes.
pub fn cosigner_account(master: Fingerprint, path: &DerivationPath, xpub: &Xpub, script: MultisigScript, label: &str) -> Vec<u8> {
    let u = Value::Uint;
    let steps: &[ChildNumber] = path.as_ref();
    let mut components = Vec::with_capacity(steps.len() * 2);
    for step in steps {
        let (index, hardened) = match step {
            ChildNumber::Hardened { index } => (*index, true),
            ChildNumber::Normal { index } => (*index, false),
        };
        components.push(u(u64::from(index)));
        components.push(Value::Bool(hardened));
    }
    let fingerprint = |f: Fingerprint| u(u64::from(u32::from_be_bytes(f.to_bytes())));
    let origin = Value::Map(vec![(u(1), Value::Array(components)), (u(2), fingerprint(master)), (u(3), u(steps.len() as u64))]);
    let use_info = Value::Map(vec![(u(1), u(0)), (u(2), u(if xpub.network == NetworkKind::Main { 0 } else { 1 }))]);
    let hdkey = Value::Map(vec![
        (u(3), Value::Bytes(xpub.public_key.serialize().to_vec())),
        (u(4), Value::Bytes(xpub.chain_code.to_bytes().to_vec())),
        (u(5), Value::tag(305, use_info)),
        (u(6), Value::tag(304, origin)),
        (u(8), fingerprint(xpub.parent_fingerprint)),
        (u(9), Value::Text(label.to_string())),
    ]);
    let mut output = Value::tag(303, hdkey);
    for tag in script.tags().iter().rev() {
        output = Value::tag(*tag, output);
    }
    cbor::encode(&Value::Map(vec![(u(1), fingerprint(master)), (u(2), Value::Array(vec![output]))]))
}

/// [`cosigner_account`] as the text of one QR code: `UR:CRYPTO-ACCOUNT/...` (capitals, which QR codes store compactly).
pub fn cosigner_account_ur(master: Fingerprint, path: &DerivationPath, xpub: &Xpub, script: MultisigScript, label: &str) -> String {
    ur::ur::encode(&cosigner_account(master, path, xpub, script, label), &ur::Type::Custom("crypto-account")).to_uppercase()
}

/// What happened with one scanned QR code, see [`DescriptorScanner`].
pub enum DescriptorUpdate {
    /// A whole multisig wallet was read.
    Done(MultisigWallet),
    /// Part of an animated code: `have` of `total` parts so far.
    Progress { have: usize, total: usize },
    /// Not what is needed here; the text says what it is.
    Ignored(String),
    /// A wallet descriptor, but one that cannot be used; the text says why.
    Refused(String),
}

/// Collects a multisig wallet's descriptor from scanned QR codes: a UR `crypto-output` (one code or
/// an animated series) or a plain-text descriptor in one code.
pub struct DescriptorScanner {
    decoder: ur::Decoder,
    bbqr: crate::bbqr::BbqrCollector,
}

impl Default for DescriptorScanner {
    fn default() -> Self {
        DescriptorScanner::new()
    }
}

impl DescriptorScanner {
    pub fn new() -> Self {
        DescriptorScanner { decoder: ur::Decoder::default(), bbqr: crate::bbqr::BbqrCollector::new() }
    }

    /// Feed the text of one scanned QR code.
    pub fn add(&mut self, text: &str) -> DescriptorUpdate {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        if text.contains("multi(") || text.contains("multi_a(") {
            return match MultisigWallet::from_descriptor_text(text) {
                Ok(wallet) => DescriptorUpdate::Done(wallet),
                Err(e) => DescriptorUpdate::Refused(e.to_string()),
            };
        }
        if crate::bbqr::is_bbqr(text) {
            use crate::bbqr::BbqrUpdate;
            return match self.bbqr.add(text) {
                // Sparrow shows the descriptor as text (U); JSON (J) from other wallets is tried the same way
                BbqrUpdate::Done { file_type: 'U' | 'J', data } => match String::from_utf8(data) {
                    Ok(text) => match MultisigWallet::from_descriptor_text(&text) {
                        Ok(wallet) => DescriptorUpdate::Done(wallet),
                        Err(e) => DescriptorUpdate::Refused(e.to_string()),
                    },
                    Err(_) => DescriptorUpdate::Ignored("this BBQr code's text cannot be read".into()),
                },
                BbqrUpdate::Done { file_type: 'P' | 'T', .. } => {
                    DescriptorUpdate::Ignored("that is a transaction's QR code, not the wallet's descriptor".into())
                }
                BbqrUpdate::Done { file_type, .. } => {
                    DescriptorUpdate::Ignored(format!("this BBQr code holds a file of type {file_type}, not a wallet descriptor"))
                }
                BbqrUpdate::Progress { have, total } => DescriptorUpdate::Progress { have, total },
                BbqrUpdate::Ignored(why) => DescriptorUpdate::Ignored(format!("this BBQr code cannot be read: {why}")),
            };
        }
        let Some(rest) = lower.strip_prefix("ur:") else {
            if text.to_uppercase().starts_with(crate::backup::TEXT_PREFIX) {
                return DescriptorUpdate::Ignored("that is a SeedLock backup, not a wallet descriptor".into());
            }
            return DescriptorUpdate::Ignored("this QR code is not a wallet descriptor".into());
        };
        match rest.split('/').next().unwrap_or("") {
            "crypto-output" => {}
            "crypto-psbt" | "psbt" => return DescriptorUpdate::Ignored("that is a transaction's QR code, not the wallet's descriptor".into()),
            "crypto-account" | "crypto-hdkey" => {
                return DescriptorUpdate::Ignored("that is one cosigner's key, not the whole wallet's descriptor".into())
            }
            other => return DescriptorUpdate::Ignored(format!("this QR code holds a \"{other}\", not a wallet descriptor")),
        }
        let finished = |payload: &[u8]| match cbor::decode(payload).and_then(|v| MultisigWallet::from_crypto_output(&v)) {
            Ok(wallet) => DescriptorUpdate::Done(wallet),
            Err(e) => DescriptorUpdate::Refused(e.to_string()),
        };
        match ur::ur::decode(&lower) {
            Ok((ur::ur::Kind::SinglePart, payload)) => return finished(&payload),
            Ok(_) => {}
            Err(_) => return DescriptorUpdate::Ignored("damaged QR code".into()),
        }
        if self.decoder.receive(&lower).is_err() {
            self.decoder = ur::Decoder::default();
            if self.decoder.receive(&lower).is_err() {
                return DescriptorUpdate::Ignored("damaged QR code".into());
            }
        }
        if self.decoder.complete() {
            let message = self.decoder.message();
            self.decoder = ur::Decoder::default();
            return match message {
                Ok(Some(payload)) => finished(&payload),
                _ => DescriptorUpdate::Ignored("the QR data is incomplete".into()),
            };
        }
        DescriptorUpdate::Progress { have: self.decoder.resolved_fragment_count().unwrap_or(0), total: self.decoder.fragment_count() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::Wallet;

    const SEEDS: [&str; 3] = [
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        "legal winner thank year wave sausage worth useful legal winner thank yellow",
        "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
    ];

    fn wallet(i: usize) -> Wallet {
        Wallet::new(&crate::mnemonic::parse(SEEDS[i]).unwrap(), "", Network::Testnet).unwrap()
    }

    /// A 2-of-3 descriptor the way Sparrow exports it, with `hardened` as the marker (h or ').
    fn sparrow_descriptor(hardened: &str, tail: &str) -> String {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let keys: Vec<String> = (0..3)
            .map(|i| {
                let w = wallet(i);
                format!("[{}/48{hardened}/1{hardened}/0{hardened}/2{hardened}]{}/{tail}", w.fingerprint(), w.xpub_at(&path).unwrap())
            })
            .collect();
        let body = format!("wsh(sortedmulti(2,{}))", keys.join(","));
        format!("{body}#{}", descriptor_checksum(&body).unwrap())
    }

    #[test]
    fn reads_sparrow_descriptors_in_every_form() {
        let wallet = MultisigWallet::from_descriptor_text(&sparrow_descriptor("h", "<0;1>/*")).unwrap();
        assert_eq!(wallet.label(), "2 of 3");
        assert!(!wallet.is_mainnet());
        assert_eq!(wallet.descriptor().unwrap(), sparrow_descriptor("h", "<0;1>/*"), "written back exactly");
        // apostrophes, the receive-only form, and a whole exported file: the same wallet
        assert!(MultisigWallet::from_descriptor_text(&sparrow_descriptor("'", "<0;1>/*")).unwrap().same_as(&wallet));
        assert!(MultisigWallet::from_descriptor_text(&sparrow_descriptor("h", "0/*")).unwrap().same_as(&wallet));
        let file = format!(
            "# Receive and change\n{}\n\n# Receive only\nReceive: {}\n# Change only\n{}\n",
            sparrow_descriptor("h", "<0;1>/*"),
            sparrow_descriptor("h", "0/*"),
            sparrow_descriptor("h", "1/*")
        );
        assert!(MultisigWallet::from_descriptor_text(&file).unwrap().same_as(&wallet));
    }

    #[test]
    fn refuses_what_it_cannot_check() {
        let good = sparrow_descriptor("h", "<0;1>/*");
        let (body, _) = good.rsplit_once('#').unwrap();
        let refused = |text: &str, why: &str| {
            let e = MultisigWallet::from_descriptor_text(text).unwrap_err().to_string();
            assert!(e.contains(why), "{text:.40}: {e}");
        };
        refused(&format!("{body}#qqqqqqqq"), "checksum");
        refused(&body.replace("wsh(sortedmulti(", "tr(sortedmulti_a("), "Taproot");
        refused(&body.replace("wsh(sortedmulti(", "wsh(pk("), "no multisig output descriptor");
        refused(&body.replace("sortedmulti(2,", "sortedmulti(4,"), "cannot be right");
        refused(&body.replace("sortedmulti(2,", "sortedmulti(0,"), "cannot be right");
        let first_key = body.split(',').nth(1).unwrap();
        refused(&body.replacen(first_key, &first_key[first_key.find(']').unwrap() + 1..], 1), "origin");
        refused(&body.replacen("/2h]", "]", 1), "do not belong together");
        let keys: Vec<&str> = body.trim_start_matches("wsh(sortedmulti(2,").trim_end_matches("))").split(',').collect();
        refused(&format!("wsh(sortedmulti(2,{},{},{}))", keys[0], keys[1], keys[0]), "appears twice");
        refused(&format!("wsh(sortedmulti(1,{}))", keys[0]), "2 to 15 keys");
        refused("just some text", "no multisig output descriptor");
    }

    /// A descriptor of the three test seeds with account path `account` (like "48h/1h/0h/1h"),
    /// wrapped in `wrap` ("wsh(", "sh(wsh(" or "sh(") around `multi` ("sortedmulti" or "multi").
    fn build(wrap: &str, multi: &str, account: &str) -> String {
        let path: DerivationPath = format!("m/{}", account.replace('h', "'")).parse().unwrap();
        let keys: Vec<String> =
            (0..3).map(|i| format!("[{}/{account}]{}/<0;1>/*", wallet(i).fingerprint(), wallet(i).xpub_at(&path).unwrap())).collect();
        let closing = ")".repeat(wrap.matches('(').count()); // "(2,...)" closes multi() itself
        let body = format!("{wrap}{multi}(2,{}){closing}", keys.join(","));
        format!("{body}#{}", descriptor_checksum(&body).unwrap())
    }

    /// A wallet as the UR crypto-output a QR code carries. `sparrow`: exactly as Sparrow writes it
    /// (SettingsController and hummingbird): "is-private: false", no children, each key named by its
    /// keystore label. Otherwise as urtypes writes it with children (`/0/*`).
    fn crypto_output(ms: &MultisigWallet, sparrow: bool) -> Value {
        let u = Value::Uint;
        let keys = ms
            .cosigners
            .iter()
            .map(|c| {
                let steps: &[ChildNumber] = c.path.as_ref();
                let components = steps
                    .iter()
                    .flat_map(|s| match s {
                        ChildNumber::Hardened { index } => [u(u64::from(*index)), Value::Bool(true)],
                        ChildNumber::Normal { index } => [u(u64::from(*index)), Value::Bool(false)],
                    })
                    .collect();
                let fp = |f: Fingerprint| u(u64::from(u32::from_be_bytes(f.to_bytes())));
                let depth = u(steps.len() as u64);
                let mut map = Vec::new();
                if sparrow {
                    map.push((u(2), Value::Bool(false)));
                }
                map.push((u(3), Value::Bytes(c.xpub.public_key.serialize().to_vec())));
                map.push((u(4), Value::Bytes(c.xpub.chain_code.to_bytes().to_vec())));
                if sparrow {
                    map.push((u(5), Value::tag(305, Value::Map(vec![(u(1), u(0)), (u(2), u(1))]))));
                    map.push((u(6), Value::tag(304, Value::Map(vec![(u(1), Value::Array(components)), (u(2), fp(c.fingerprint)), (u(3), depth)]))));
                } else {
                    map.push((u(5), Value::tag(305, Value::Map(vec![(u(2), u(1))]))));
                    map.push((u(6), Value::tag(304, Value::Map(vec![(u(1), Value::Array(components)), (u(2), fp(c.fingerprint))]))));
                    let children = vec![u(0), Value::Bool(false), Value::Array(vec![]), Value::Bool(false)];
                    map.push((u(7), Value::tag(304, Value::Map(vec![(u(1), Value::Array(children))]))));
                }
                map.push((u(8), fp(c.xpub.parent_fingerprint)));
                if sparrow {
                    map.push((u(9), Value::Text(format!("Keystore {}", c.fingerprint))));
                }
                Value::tag(303, Value::Map(map))
            })
            .collect();
        let mut value = Value::tag(if ms.sorted { 407 } else { 406 }, Value::Map(vec![(u(1), u(ms.threshold as u64)), (u(2), Value::Array(keys))]));
        for tag in ms.script.tags().iter().rev() {
            value = Value::tag(*tag, value);
        }
        value
    }

    #[test]
    fn reads_nested_legacy_and_unsorted_wallets_and_builds_their_scripts() {
        let nested = MultisigWallet::from_descriptor_text(&build("sh(wsh(", "sortedmulti", "48h/1h/0h/1h")).unwrap();
        assert_eq!((nested.script, nested.sorted), (MultisigScript::ShWsh, true));
        let (redeem, witness) = nested.psbt_scripts(0, 0).unwrap();
        let (redeem, witness) = (redeem.unwrap(), witness.unwrap());
        assert_eq!(redeem, ScriptBuf::new_p2wsh(&witness.wscript_hash()), "nested: the redeem script is the P2WSH program");
        assert_eq!(nested.script_pubkey(0, 0).unwrap(), ScriptBuf::new_p2sh(&redeem.script_hash()));
        assert!(nested.address(0, 0, Network::Testnet).unwrap().to_string().starts_with('2'));

        let legacy = MultisigWallet::from_descriptor_text(&build("sh(", "multi", "45h")).unwrap();
        assert_eq!((legacy.script, legacy.sorted), (MultisigScript::Sh, false));
        let (redeem, witness) = legacy.psbt_scripts(0, 0).unwrap();
        assert!(witness.is_none(), "legacy: no witness script");
        assert_eq!(redeem.clone().unwrap(), legacy.witness_script(0, 0).unwrap(), "legacy: the multisig script is the redeem script");
        assert_eq!(legacy.script_pubkey(0, 0).unwrap(), ScriptBuf::new_p2sh(&redeem.unwrap().script_hash()));
        // multi(): the keys stay in the descriptor's order
        let in_order: Vec<PublicKey> =
            (0..3).map(|i| PublicKey::new(wallet(i).public_key_at(&"m/45'/0/0".parse().unwrap()).unwrap())).collect();
        assert_eq!(legacy.keys_at(0, 0).unwrap(), in_order);
        assert_eq!(legacy.descriptor().unwrap(), build("sh(", "multi", "45h"), "written back exactly as read");
        assert!(!legacy.same_as(&MultisigWallet::from_descriptor_text(&build("sh(", "sortedmulti", "45h")).unwrap()));
    }

    #[test]
    fn reads_the_crypto_output_qr_format_and_refuses_private_or_master_keys() {
        for (wrap, multi, account) in [("wsh(", "sortedmulti", "48h/1h/0h/2h"), ("sh(wsh(", "sortedmulti", "48h/1h/0h/1h"), ("sh(", "multi", "45h")] {
            let ms = MultisigWallet::from_descriptor_text(&build(wrap, multi, account)).unwrap();
            for sparrow in [true, false] {
                let read = MultisigWallet::from_crypto_output(&cbor::decode(&cbor::encode(&crypto_output(&ms, sparrow))).unwrap()).unwrap();
                assert!(read.same_as(&ms), "{wrap}{multi} (Sparrow's form: {sparrow})");
                assert_eq!(read.descriptor().unwrap(), ms.descriptor().unwrap(), "{wrap}{multi}: even the xpubs come back the same");
            }
        }
        let u = Value::Uint;
        let one_key = |hdkey: Value| {
            let keys = Value::Array(vec![Value::tag(303, hdkey.clone()), Value::tag(303, hdkey)]);
            Value::tag(401, Value::tag(407, Value::Map(vec![(u(1), u(1)), (u(2), keys)])))
        };
        let private = one_key(Value::Map(vec![(u(2), Value::Bytes(vec![1; 32])), (u(3), Value::Bytes(vec![2; 33]))]));
        assert!(MultisigWallet::from_crypto_output(&private).unwrap_err().to_string().contains("private key"));
        let flagged = one_key(Value::Map(vec![(u(2), Value::Bool(true)), (u(3), Value::Bytes(vec![2; 33]))]));
        assert!(MultisigWallet::from_crypto_output(&flagged).unwrap_err().to_string().contains("private key"), "is-private: true");
        let master = one_key(Value::Map(vec![(u(1), Value::Bool(true)), (u(3), Value::Bytes(vec![2; 33]))]));
        assert!(MultisigWallet::from_crypto_output(&master).unwrap_err().to_string().contains("master key"));
        assert!(MultisigWallet::from_crypto_output(&Value::tag(409, Value::Null)).unwrap_err().to_string().contains("Taproot"));
    }

    #[test]
    fn shares_a_cosigner_key_as_a_crypto_account_the_way_seedsigner_does() {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let w = wallet(0);
        let xpub = w.xpub_at(&path).unwrap();
        let account = cbor::decode(&cosigner_account(w.fingerprint(), &path, &xpub, MultisigScript::Wsh, "alice")).unwrap();
        assert_eq!(account.get(1).and_then(Value::as_uint), Some(u64::from(u32::from_be_bytes(w.fingerprint().to_bytes()))));
        let outputs = account.get(2).and_then(Value::as_array).unwrap();
        let Value::Tag(401, inner) = &outputs[0] else { panic!("wsh() first") };
        let Value::Tag(303, hdkey) = &**inner else { panic!("then the key") };
        assert_eq!(hdkey.get(9), Some(&Value::Text("alice".into())), "named for Sparrow's keystore label");
        let cosigner = cosigner_from_hdkey(hdkey).unwrap();
        assert_eq!((cosigner.fingerprint, cosigner.path, cosigner.xpub), (w.fingerprint(), path.clone(), xpub), "it reads back as the same key");
        let nested = cbor::decode(&cosigner_account(w.fingerprint(), &path, &xpub, MultisigScript::ShWsh, "alice")).unwrap();
        assert!(matches!(&nested.get(2).and_then(Value::as_array).unwrap()[0], Value::Tag(400, i) if matches!(&**i, Value::Tag(401, _))));
        let ur = cosigner_account_ur(w.fingerprint(), &path, &xpub, MultisigScript::Wsh, "alice");
        assert!(ur.starts_with("UR:CRYPTO-ACCOUNT/") && ur.matches('/').count() == 1, "one single-part code: {ur}");
        let (_, payload) = ur::ur::decode(&ur.to_lowercase()).unwrap();
        assert_eq!(payload, cosigner_account(w.fingerprint(), &path, &xpub, MultisigScript::Wsh, "alice"));
        // the label: the backup's name, within Sparrow's 16 characters (UTF-16 units, as Java counts them)
        let fp = w.fingerprint();
        assert_eq!(keystore_label("alice", fp), "alice");
        assert_eq!(keystore_label("my-family-savings-2026", fp), "my-family-saving");
        assert_eq!(keystore_label("wallet one       x", fp), "wallet one", "no space left dangling at the end");
        assert_eq!(keystore_label("caja de ahorros ñandú", fp), "caja de ahorros");
        assert_eq!(keystore_label("bitcoin 🔐🔐🔐🔐", fp), "bitcoin 🔐🔐🔐🔐", "an emoji counts twice: 8 + 4 x 2 = 16");
        assert_eq!(keystore_label("bitcoin! 🔐🔐🔐🔐", fp), "bitcoin! 🔐🔐🔐");
        assert_eq!(keystore_label("   ", fp), "SeedLock 73c5");
    }

    #[test]
    fn scans_a_descriptor_from_text_or_crypto_output_codes() {
        let text = build("wsh(", "sortedmulti", "48h/1h/0h/2h");
        let ms = MultisigWallet::from_descriptor_text(&text).unwrap();
        let mut scanner = DescriptorScanner::new();
        assert!(matches!(scanner.add(&text), DescriptorUpdate::Done(w) if w.same_as(&ms)));
        let payload = cbor::encode(&crypto_output(&ms, true));
        let single = ur::ur::encode(&payload, &ur::Type::Custom("crypto-output")).to_uppercase();
        assert!(matches!(scanner.add(&single), DescriptorUpdate::Done(w) if w.same_as(&ms)));
        // animated: several parts, taken in any order, with progress in between
        let mut encoder = ur::Encoder::new(&payload, 60, "crypto-output").unwrap();
        let parts: Vec<String> = (0..2 * encoder.fragment_count()).map(|_| encoder.next_part().unwrap().to_uppercase()).collect();
        assert!(parts.len() > 4);
        let mut scanner = DescriptorScanner::new();
        let mut read = None;
        let mut progressed = false;
        for part in parts.iter().rev() {
            match scanner.add(part) {
                DescriptorUpdate::Done(w) => {
                    read = Some(w);
                    break;
                }
                DescriptorUpdate::Progress { .. } => progressed = true,
                _ => panic!("an animated part was not taken"),
            }
        }
        assert!(progressed && read.unwrap().same_as(&ms));
        // as Sparrow shows it when BBQr is chosen: the descriptor as text (type U), compressed, in parts
        let mut scanner = DescriptorScanner::new();
        let parts = crate::bbqr::test_parts('U', text.as_bytes(), 3);
        assert!(matches!(scanner.add(&parts[1]), DescriptorUpdate::Progress { have: 1, total: 3 }));
        assert!(matches!(scanner.add(&parts[0]), DescriptorUpdate::Progress { have: 2, total: 3 }));
        assert!(matches!(scanner.add(&parts[2]), DescriptorUpdate::Done(w) if w.same_as(&ms)));
        let psbt_code = ur::ur::encode(&[0x40], &ur::Type::Custom("crypto-psbt")).to_uppercase();
        for (code, says) in [(psbt_code.as_str(), "transaction"), ("B$2P0100ABCDEFGH", "transaction"), ("SEEDLOCK1:ABCDEF", "backup"), ("hello", "not a wallet")] {
            assert!(matches!(DescriptorScanner::new().add(code), DescriptorUpdate::Ignored(why) if why.contains(says)), "{code}");
        }
        assert!(matches!(DescriptorScanner::new().add(&format!("{}q", &text[..text.len() - 1])), DescriptorUpdate::Refused(_)), "a wrong checksum");
    }

    #[test]
    fn builds_sorted_2_of_3_scripts_and_finds_its_keys() {
        let ms = MultisigWallet::from_descriptor_text(&sparrow_descriptor("h", "<0;1>/*")).unwrap();
        for (chain, index) in [(0, 0), (1, 7)] {
            let keys = ms.keys_at(chain, index).unwrap();
            assert!(keys.windows(2).all(|p| p[0].inner.serialize() < p[1].inner.serialize()), "sorted as BIP 67 says");
            for i in 0..3 {
                let own: DerivationPath = format!("m/48'/1'/0'/2'/{chain}/{index}").parse().unwrap();
                let key = PublicKey::new(wallet(i).public_key_at(&own).unwrap());
                assert!(keys.contains(&key), "cosigner {i}'s own key is in the script");
                assert_eq!(ms.locate(wallet(i).fingerprint(), &own), Some((chain, index)));
            }
            let script = ms.witness_script(chain, index).unwrap();
            assert_eq!(script.len(), 1 + 3 * 34 + 1 + 1, "OP_2, three pushed keys, OP_3, OP_CHECKMULTISIG");
            assert_eq!(ms.script_pubkey(chain, index).unwrap(), ScriptBuf::new_p2wsh(&script.wscript_hash()));
            let address = ms.address(chain, index, Network::Testnet).unwrap().to_string();
            assert!(address.starts_with("tb1q") && address.len() == 62, "{address}");
        }
        let fp = wallet(1).fingerprint();
        assert_eq!(ms.position(fp), Some(1));
        assert_eq!(ms.locate(fp, &"m/48'/1'/0'/2'/2/0".parse().unwrap()), None, "chain 2 is neither receive nor change");
        assert_eq!(ms.locate(fp, &"m/84'/1'/0'/0/0".parse().unwrap()), None, "another account");
    }
}
