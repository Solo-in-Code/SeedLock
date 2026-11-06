//! Silent payments in a PSBT of version 2, worked out for a wallet before it is reviewed and signed:
//!
//! * spending what the wallet's silent payment address received (BIP 376): an input with a tweak
//!   (`PSBT_IN_SP_TWEAK`) is the wallet's when its spend key plus the tweak gives exactly the key of the output it
//!   spends; it is then signed with that key, as a taproot key-path spend;
//! * paying silent payment addresses, someone else's or the wallet's own (its change, with label 0) (BIP 375):
//!   their outputs come without a script, as Sparrow cannot work it out without the inputs' private keys. From
//!   those keys SeedLock makes the ECDH share for each scan key, with its proof (BIP 374), and the outputs'
//!   scripts (BIP 352), and puts them into the PSBT. Only then can it be signed, and nothing in it may change
//!   afterwards (its "modifiable" flags are cleared), as the scripts depend on all its inputs.

use crate::multisig::MultisigWallet;
use crate::psbt::{input_txout, ownership, Ownership, PathShape};
use crate::psbt_v2::*;
use crate::silent::{self, SilentKeys};
use crate::wallet::{path_to_display, ScriptType, Wallet};
use crate::{Error, Result};
use bitcoin::bip32::Fingerprint;
use bitcoin::consensus;
use bitcoin::hashes::{hash160, Hash};
use bitcoin::key::TapTweak;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{All, Keypair, Parity, PublicKey, Secp256k1, SecretKey};
use std::collections::BTreeMap;

/// BIP 341's NUMS point H, the internal key of taproot outputs that can only be spent by a script: such inputs do
/// not count for silent payments.
const NUMS_H: [u8; 32] = [
    0x50, 0x92, 0x9b, 0x74, 0xc1, 0xa0, 0x49, 0x54, 0xb7, 0x8b, 0x4b, 0x60, 0x35, 0xe9, 0x7a, 0x5e, 0x07, 0x8a, 0x5a,
    0x0f, 0x28, 0xec, 0x96, 0xd5, 0x47, 0xbf, 0xee, 0x9a, 0xce, 0x80, 0x3a, 0xc0,
];

/// A group may hold at most this many outputs for the same scan key (BIP 352's K_max).
const K_MAX: usize = 2323;

/// An input that spends a silent payment to this wallet.
pub(crate) struct Spend {
    /// The key that signs it (BIP 376).
    pub(crate) key: SecretKey,
    /// What the review shows as its key path.
    pub(crate) path: String,
}

impl Drop for Spend {
    fn drop(&mut self) {
        self.key.non_secure_erase();
    }
}

/// An output to this wallet's own silent payment address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Own {
    /// Its change: the address with label 0.
    Change,
    /// The address itself (None), or with another label.
    Receive(Option<u32>),
}

/// An output to a silent payment address.
pub(crate) struct Payment {
    /// The silent payment address (`sp1q...`, `tsp1q...`).
    pub(crate) address: String,
    /// Whether it is this wallet's own address.
    pub(crate) own: Option<Own>,
}

/// What review and signing need to know of a PSBT's silent payments.
#[derive(Default)]
pub(crate) struct Extras {
    /// For each input: whether it spends a silent payment to this wallet.
    pub(crate) spends: Vec<Option<Spend>>,
    /// For each output: whether it pays a silent payment address.
    pub(crate) payments: Vec<Option<Payment>>,
}

/// The silent payment address an output pays: its scan key, its spend key (with its label added, if it has
/// one), and its label, when the PSBT says it (for change).
#[derive(Clone, Copy)]
struct Target {
    scan: PublicKey,
    spend: PublicKey,
    label: Option<u32>,
}

/// An input, as silent payments look at it.
struct InputKey {
    /// Whether it counts for silent payments (BIP 352: P2TR, P2WPKH, P2SH-P2WPKH, P2PKH).
    eligible: bool,
    /// The wallet's private key for it, when it counts (for a taproot output: the one of its output key, with an
    /// even y).
    key: Option<SecretKey>,
    /// Its public key as the PSBT gives it, when it counts.
    public: Option<PublicKey>,
}

impl Drop for InputKey {
    fn drop(&mut self) {
        if let Some(k) = &mut self.key {
            k.non_secure_erase();
        }
    }
}

fn refuse(text: String) -> Error {
    Error::Psbt(text)
}

/// The private key of a taproot (BIP 86) output whose internal key is `internal`: tweaked as BIP 341 says, and
/// negated when the output key has an odd y (as BIP 352 needs).
fn taproot_output_key(secp: &Secp256k1<All>, internal: &SecretKey) -> SecretKey {
    let tweaked = Keypair::from_secret_key(secp, internal).tap_tweak(secp, None).to_keypair();
    let key = tweaked.secret_key();
    if tweaked.x_only_public_key().1 == Parity::Odd {
        key.negate()
    } else {
        key
    }
}

/// The lengths of the silent payment fields (BIP 375, 376): a PSBT with a wrong one is refused as a whole.
fn check_fields(raw: &RawPsbt) -> Result<()> {
    let wrong = |what: String| Err(refuse(format!("invalid PSBT: {what} has the wrong length")));
    // fields whose key data is a 33-byte key, and fields without key data
    let keyed = |map: &Map, t: u8, ok: fn(usize) -> bool| map.entries(t).all(|(key, value)| key.len() == 33 && ok(value.len()));
    let plain = |map: &Map, t: u8, len: usize| map.entries(t).all(|(key, value)| key.is_empty() && value.len() == len);
    if !keyed(&raw.global, GLOBAL_SP_ECDH_SHARE, |n| n == 33) {
        return wrong("a global ECDH share".into());
    }
    if !keyed(&raw.global, GLOBAL_SP_DLEQ, |n| n == 64) {
        return wrong("a global DLEQ proof".into());
    }
    for (i, map) in raw.inputs.iter().enumerate() {
        if !keyed(map, IN_SP_ECDH_SHARE, |n| n == 33) {
            return wrong(format!("input #{i}'s ECDH share"));
        }
        if !keyed(map, IN_SP_DLEQ, |n| n == 64) {
            return wrong(format!("input #{i}'s DLEQ proof"));
        }
        if !keyed(map, IN_SP_SPEND_BIP32_DERIVATION, |n| n >= 4 && n % 4 == 0) {
            return wrong(format!("input #{i}'s silent payment key path"));
        }
        if !plain(map, IN_SP_TWEAK, 32) {
            return wrong(format!("input #{i}'s silent payment tweak"));
        }
    }
    for (j, map) in raw.outputs.iter().enumerate() {
        if !plain(map, OUT_SP_V0_INFO, 66) {
            return wrong(format!("output #{j}'s silent payment address"));
        }
        if !plain(map, OUT_SP_V0_LABEL, 4) {
            return wrong(format!("output #{j}'s silent payment label"));
        }
    }
    Ok(())
}

/// The silent payment address each output pays, if any.
fn targets(raw: &RawPsbt) -> Result<Vec<Option<Target>>> {
    let mut targets = Vec::with_capacity(raw.outputs.len());
    for (j, map) in raw.outputs.iter().enumerate() {
        let label = map.get(OUT_SP_V0_LABEL).map(|v| u32_field(v, "a silent payment label")).transpose()?;
        targets.push(match map.get(OUT_SP_V0_INFO) {
            Some(info) => {
                let key = |b: &[u8]| {
                    PublicKey::from_slice(b).map_err(|_| refuse(format!("output #{j}: its silent payment address holds an invalid key")))
                };
                Some(Target { scan: key(&info[..33])?, spend: key(&info[33..66])?, label })
            }
            None if label.is_some() => {
                return Err(refuse(format!("output #{j} has a silent payment label, but no silent payment address")))
            }
            None if !map.has(OUT_SCRIPT) => {
                return Err(refuse(format!("output #{j} has neither a script nor a silent payment address")))
            }
            None => None,
        });
    }
    Ok(targets)
}

/// Whether input `i` counts for silent payments (BIP 352), and its public key as the PSBT gives it: a taproot
/// output's key (with an even y), or the key in `PSBT_IN_BIP32_DERIVATION` whose hash its script holds (an input
/// of an uncompressed key does not count).
fn eligibility(view: &Psbt, map: &Map, i: usize) -> Result<(bool, Option<PublicKey>)> {
    let script = input_txout(view, i)?.script_pubkey;
    let input = &view.inputs[i];
    let bytes = script.as_bytes();
    let hash: &[u8] = if script.is_p2tr() {
        if input.tap_internal_key.is_some_and(|k| k.serialize() == NUMS_H) {
            return Ok((false, None));
        }
        let mut compressed = [2u8; 33];
        compressed[1..].copy_from_slice(&bytes[2..34]);
        return Ok((true, PublicKey::from_slice(&compressed).ok()));
    } else if script.is_p2wpkh() {
        &bytes[2..22]
    } else if script.is_p2pkh() {
        &bytes[3..23]
    } else if script.is_p2sh() {
        match &input.redeem_script {
            Some(redeem) if redeem.is_p2wpkh() => &redeem.as_bytes()[2..22],
            Some(_) => return Ok((false, None)),
            None => {
                return Err(refuse(format!(
                    "input #{i} spends a P2SH output without saying its redeem script, so whether it counts for a \
                     silent payment cannot be told"
                )))
            }
        }
    } else {
        return Ok((false, None));
    };
    let mut public = None;
    for (data, _) in map.entries(IN_BIP32_DERIVATION) {
        if hash160::Hash::hash(data).as_byte_array()[..] == *hash {
            if data.len() != 33 {
                return Ok((false, None));
            }
            public = PublicKey::from_slice(data).ok();
        }
    }
    Ok((true, public))
}

/// Works out a version 2 PSBT's silent payments for this wallet. Returns the PSBT with what it needs before it
/// can be signed (ECDH shares, proofs, output scripts), its version-0 form to review and sign, and the
/// silent payment details of its inputs and outputs. Fails, saying why, when something about them is wrong.
pub(crate) fn prepare(raw: &RawPsbt, wallet: &Wallet, multisig: &[(String, MultisigWallet)]) -> Result<(RawPsbt, Psbt, Extras)> {
    check_fields(raw)?;
    let secp = Secp256k1::new();
    let keys = SilentKeys::new(wallet, 0)?;
    let targets = targets(raw)?;
    let pays = targets.iter().any(Option::is_some);
    // outputs not worked out yet get a stand-in script here, only to read the inputs
    let view = to_v0_with_stand_ins(raw)?;

    // ---- the inputs: which are the wallet's, and with which keys
    let mut spends: Vec<Option<Spend>> = Vec::with_capacity(raw.inputs.len());
    let mut input_keys: Vec<InputKey> = Vec::with_capacity(raw.inputs.len());
    let mut owns_any = false;
    for (i, map) in raw.inputs.iter().enumerate() {
        let txout = input_txout(&view, i)?;
        let script = &txout.script_pubkey;
        let mut key: Option<SecretKey> = None;
        let mut spend: Option<Spend> = None;

        // spending a silent payment (BIP 376): the wallet's when its spend key and the tweak give the output's key
        let spend_public = keys.spend_public().serialize();
        let claims_ours = map.entries(IN_SP_SPEND_BIP32_DERIVATION).any(|(spend_key, path)| {
            spend_key == spend_public.as_slice() || <[u8; 4]>::try_from(&path[..4]).ok().map(Fingerprint::from) == Some(keys.fingerprint())
        });
        match map.get(IN_SP_TWEAK) {
            Some(tweak) => {
                let tweak: [u8; 32] = tweak.try_into().map_err(|_| refuse(format!("input #{i}: its silent payment tweak has the wrong length")))?;
                let d = keys.spending_key(&tweak).ok().filter(|d| {
                    script.is_p2tr() && d.x_only_public_key(&secp).0.serialize()[..] == script.as_bytes()[2..34]
                });
                match d {
                    Some(d) => {
                        key = Some(d);
                        spend = Some(Spend { key: d, path: format!("silent payment, {}", path_to_display(&keys.spend_path())) });
                    }
                    None if claims_ours => {
                        return Err(refuse(format!(
                            "input #{i} says it is a silent payment to your wallet, but its tweak does not give the key of \
                             the output it spends (another account, or a wrong tweak); refusing to sign"
                        )))
                    }
                    None => {}
                }
            }
            None if claims_ours => {
                return Err(refuse(format!("input #{i} says it is a silent payment to your wallet, but has no tweak; refusing to sign")))
            }
            None => {}
        }

        // or an input of the wallet's own (BIP 44, 49, 84, 86), as the review finds them
        if key.is_none() {
            let input = &view.inputs[i];
            let claim = ownership(
                wallet,
                &txout,
                &input.bip32_derivation,
                &input.tap_key_origins,
                input.witness_script.as_ref(),
                input.redeem_script.as_ref(),
                multisig,
                &mut Vec::new(),
                "",
            )?;
            match claim {
                Ownership::Verified(o) => {
                    let mut private = wallet.private_key_at(&o.path)?;
                    key = Some(match PathShape::parse(&o.path).map(|s| s.script) {
                        Some(ScriptType::Tr) => taproot_output_key(&secp, &private),
                        _ => private,
                    });
                    private.non_secure_erase();
                }
                Ownership::VerifiedMultisig(_) => owns_any = true,
                Ownership::NotClaimed | Ownership::Unverified => {}
            }
        }
        owns_any |= key.is_some();
        // whether it counts matters only when paying a silent payment address
        let (eligible, public) = if pays { eligibility(&view, map, i)? } else { (false, None) };
        input_keys.push(InputKey { eligible, key: if eligible { key } else { None }, public });
        spends.push(spend);
    }

    // ---- the outputs' scripts
    if pays && !owns_any {
        return Err(refuse(format!(
            "none of the inputs belong to the wallet with fingerprint {}: wrong seed or missing BIP39 passphrase?",
            wallet.fingerprint()
        )));
    }
    let mut raw = raw.clone();
    if pays {
        complete(&mut raw, &view, &targets, &input_keys)?;
    }

    // ---- what the review shows of the outputs
    let mut payments = Vec::with_capacity(targets.len());
    for target in &targets {
        payments.push(match target {
            None => None,
            Some(t) => {
                let own = if t.scan != keys.scan_public() {
                    None
                } else {
                    match t.label {
                        Some(m) if t.spend == keys.labeled_spend(m)? => {
                            Some(if m == silent::CHANGE_LABEL { Own::Change } else { Own::Receive(Some(m)) })
                        }
                        Some(_) => None,
                        None if t.spend == keys.spend_public() => Some(Own::Receive(None)),
                        None if t.spend == keys.labeled_spend(silent::CHANGE_LABEL)? => Some(Own::Change),
                        None => None,
                    }
                };
                Some(Payment { address: silent::encode_address(&t.scan, &t.spend, wallet.network()), own })
            }
        });
    }

    let psbt = to_v0(&raw)?;
    Ok((raw, psbt, Extras { spends, payments }))
}

/// The ECDH shares (with their proofs) and the scripts of the outputs that pay silent payment addresses, as BIP
/// 375's signer makes them: a global share for each scan key when the wallet holds the keys of all the inputs
/// that count; otherwise one for each of its own inputs, the other inputs' shares (or a global one) checked by
/// their proofs. A script the PSBT already has must be the same.
fn complete(raw: &mut RawPsbt, view: &Psbt, targets: &[Option<Target>], input_keys: &[InputKey]) -> Result<()> {
    let secp = Secp256k1::new();
    for (i, input) in view.inputs.iter().enumerate() {
        let script = input_txout(view, i)?.script_pubkey;
        if script.witness_version().is_some_and(|v| v.to_num() > 1) {
            return Err(refuse(format!("input #{i} spends an output of a SegWit version above 1, which a silent payment cannot have")));
        }
        // SIGHASH_ALL, or for taproot SIGHASH_DEFAULT, which signs the same
        let all = match input.sighash_type {
            None => true,
            Some(t) => t.to_u32() == 1 || (t.to_u32() == 0 && script.is_p2tr()),
        };
        if !all {
            return Err(refuse(format!(
                "input #{i} asks for a signature-hash type other than SIGHASH_ALL, which silent payments do not allow"
            )));
        }
    }
    let flags = raw.global.get(GLOBAL_TX_MODIFIABLE).and_then(|f| f.first().copied()).unwrap_or(0);
    if flags & 0b11 != 0 && targets.iter().zip(&raw.outputs).any(|(t, map)| t.is_some() && map.has(OUT_SCRIPT)) {
        return Err(refuse("invalid PSBT: a silent payment output has its script, but inputs or outputs may still be changed".into()));
    }
    let eligible: Vec<usize> = (0..input_keys.len()).filter(|&i| input_keys[i].eligible).collect();
    if eligible.is_empty() {
        return Err(refuse(
            "a payment to a silent payment address needs at least one input of a single key (P2WPKH, P2TR, P2SH-P2WPKH \
             or P2PKH), and this transaction has none"
                .into(),
        ));
    }
    let nothing = || refuse("the inputs' keys add up to nothing: this transaction cannot pay a silent payment address".into());
    let mut smallest: Option<[u8; 36]> = None;
    for i in 0..raw.inputs.len() {
        let outpoint: [u8; 36] =
            consensus::serialize(&raw.outpoint(i)?).try_into().map_err(|_| Error::Internal("an outpoint is not 36 bytes".into()))?;
        smallest = Some(smallest.map_or(outpoint, |s| s.min(outpoint)));
    }
    let smallest = smallest.ok_or_else(|| refuse("the transaction has no inputs".into()))?;
    let mut sum = if eligible.iter().all(|&i| input_keys[i].key.is_some()) {
        let mut ours: Vec<SecretKey> = eligible.iter().filter_map(|&i| input_keys[i].key).collect();
        let sum = silent::sum_private_keys(&ours);
        ours.iter_mut().for_each(SecretKey::non_secure_erase);
        Some(sum.ok_or_else(nothing)?)
    } else {
        None
    };
    let add = |points: &[PublicKey]| PublicKey::combine_keys(&points.iter().collect::<Vec<_>>()).map_err(|_| nothing());
    let random = || -> Result<[u8; 32]> {
        let mut aux = [0u8; 32];
        crate::entropy::random_bytes(&mut aux)?;
        Ok(aux)
    };
    let given = |map: &Map, t: u8, scan: &[u8; 33]| map.entries(t).find(|(k, _)| *k == scan.as_slice()).map(|(_, v)| v.to_vec());

    let mut groups: BTreeMap<[u8; 33], Vec<usize>> = BTreeMap::new();
    for (j, t) in targets.iter().enumerate() {
        if let Some(t) = t {
            groups.entry(t.scan.serialize()).or_default().push(j);
        }
    }
    for (scan_bytes, mut outputs) in groups {
        let scan = PublicKey::from_slice(&scan_bytes).map_err(|_| Error::Internal("a scan key".into()))?;
        if outputs.len() > K_MAX {
            return Err(refuse(format!("more than {K_MAX} outputs pay the same silent payment address; refusing to sign")));
        }
        let global_share = given(&raw.global, GLOBAL_SP_ECDH_SHARE, &scan_bytes);
        let (share, input_sum) = match &sum {
            Some(a) => {
                // the wallet holds the keys of all the inputs that count: one share for the scan key
                let share = silent::ecdh_share(a, &scan)?;
                if global_share.is_some_and(|g| g != share.serialize()) {
                    return Err(refuse("the PSBT already holds another ECDH share for a silent payment; refusing to sign".into()));
                }
                let proof = silent::dleq_prove(a, &scan, &random()?)?;
                raw.global.set(GLOBAL_SP_ECDH_SHARE, &scan_bytes, share.serialize().to_vec());
                raw.global.set(GLOBAL_SP_DLEQ, &scan_bytes, proof.to_vec());
                (share, PublicKey::from_secret_key(&secp, a))
            }
            None => {
                // some of them are someone else's
                let mut publics = Vec::with_capacity(eligible.len());
                for &i in &eligible {
                    publics.push(match input_keys[i].key {
                        Some(a) => PublicKey::from_secret_key(&secp, &a),
                        None => input_keys[i]
                            .public
                            .ok_or_else(|| refuse(format!("input #{i} does not say its public key, which a silent payment needs")))?,
                    });
                }
                let input_sum = add(&publics)?;
                match global_share {
                    Some(share) => {
                        // one share for all of them, made by a signer that holds all their keys
                        let proof = given(&raw.global, GLOBAL_SP_DLEQ, &scan_bytes)
                            .ok_or_else(|| refuse("the PSBT has a global ECDH share without its proof; refusing to sign".into()))?;
                        let share = PublicKey::from_slice(&share).map_err(|_| refuse("a global ECDH share is not a valid key".into()))?;
                        let proof: [u8; 64] = proof.try_into().map_err(|_| refuse("a global DLEQ proof has the wrong length".into()))?;
                        if !silent::dleq_verify(&input_sum, &scan, &share, &proof) {
                            return Err(refuse("the proof of the global ECDH share is wrong; refusing to sign".into()));
                        }
                        (share, input_sum)
                    }
                    None => {
                        // a share for each input: the wallet's own made here, the others' checked by their proofs
                        let mut shares = Vec::with_capacity(eligible.len());
                        for (n, &i) in eligible.iter().enumerate() {
                            match input_keys[i].key {
                                Some(a) => {
                                    let share = silent::ecdh_share(&a, &scan)?;
                                    let proof = silent::dleq_prove(&a, &scan, &random()?)?;
                                    raw.inputs[i].set(IN_SP_ECDH_SHARE, &scan_bytes, share.serialize().to_vec());
                                    raw.inputs[i].set(IN_SP_DLEQ, &scan_bytes, proof.to_vec());
                                    shares.push(share);
                                }
                                None => {
                                    let missing = || {
                                        refuse(format!(
                                            "input #{i} is not yours, and its owner's ECDH share for a silent payment is not in \
                                             the PSBT yet: the other signers must add theirs first"
                                        ))
                                    };
                                    let share = given(&raw.inputs[i], IN_SP_ECDH_SHARE, &scan_bytes).ok_or_else(missing)?;
                                    let proof = given(&raw.inputs[i], IN_SP_DLEQ, &scan_bytes).ok_or_else(|| {
                                        refuse(format!("input #{i}'s ECDH share for a silent payment has no proof; refusing to sign"))
                                    })?;
                                    let share =
                                        PublicKey::from_slice(&share).map_err(|_| refuse(format!("input #{i}: its ECDH share is not a valid key")))?;
                                    let proof: [u8; 64] =
                                        proof.try_into().map_err(|_| refuse(format!("input #{i}: its DLEQ proof has the wrong length")))?;
                                    if !silent::dleq_verify(&publics[n], &scan, &share, &proof) {
                                        return Err(refuse(format!("input #{i}: the proof of its ECDH share is wrong; refusing to sign")));
                                    }
                                    shares.push(share);
                                }
                            }
                        }
                        (add(&shares)?, input_sum)
                    }
                }
            }
        };
        let input_hash = silent::input_hash(&smallest, &input_sum)?;
        let shared = silent::shared_secret(&input_hash, &share)?;
        // k counts up by spend key, then by output index (BIP 375)
        outputs.sort_by_key(|&j| (targets[j].map(|t| t.spend.serialize()), j));
        for (k, &j) in outputs.iter().enumerate() {
            let t = targets[j].ok_or_else(|| Error::Internal("a silent payment output".into()))?;
            let mut script = vec![0x51, 0x20];
            script.extend_from_slice(&silent::output_key(&shared, &t.spend, k as u32)?.serialize());
            if raw.outputs[j].get(OUT_SCRIPT).is_some_and(|given| given != script.as_slice()) {
                return Err(refuse(format!(
                    "output #{j}: its script is not the one of the silent payment address it pays; refusing to sign"
                )));
            }
            raw.outputs[j].set(OUT_SCRIPT, &[], script);
        }
    }
    if let Some(a) = &mut sum {
        a.non_secure_erase();
    }
    // nothing may be added or removed any more: the scripts depend on every input and output
    raw.global.set(GLOBAL_TX_MODIFIABLE, &[], vec![flags & !0b11]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const IN_WITNESS_UTXO: u8 = 0x01;

    /// The one vector that counts k in output order, as BIP 375's own validator does, against the BIP's text,
    /// which counts it by silent payment code (then by output index). SeedLock follows the text, as Sparrow does.
    const K_IN_OUTPUT_ORDER: &str = "can finalize: two sp outputs - output 0 uses label=3 / output 1 uses label=1";

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    /// A BIP 375 test vector: whether it is valid, its description, its PSBT (None when it cannot even be read),
    /// and the private keys of its inputs that the vectors give, by input index.
    struct Vector {
        valid: bool,
        description: String,
        raw: Option<RawPsbt>,
        keys: BTreeMap<usize, SecretKey>,
    }

    /// The vectors' P2WPKH inputs spend scripts that are not those of their keys: only the keys and the outpoints
    /// count for silent payments, and BIP 375's validator does not compare them. SeedLock does (a wrong key there
    /// would make it pay an output nobody finds), so the inputs get the scripts of their keys first.
    fn with_scripts_of_their_keys(raw: &mut RawPsbt) {
        for map in &mut raw.inputs {
            let Some(key) = map.entries(IN_BIP32_DERIVATION).next().map(|(k, _)| k.to_vec()) else { continue };
            if let Some((_, utxo)) = map.0.iter_mut().find(|(k, _)| k.as_slice() == [IN_WITNESS_UTXO]) {
                // the amount (8 bytes), the script's length, the script
                if utxo.len() == 31 && utxo[8..11] == [22, 0x00, 0x14] {
                    utxo[11..].copy_from_slice(hash160::Hash::hash(&key).as_byte_array());
                }
            }
        }
    }

    fn vectors() -> Vec<Vector> {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/bip375.txt")).unwrap();
        let mut out = Vec::new();
        let mut current: Option<Vector> = None;
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            let v = current.as_mut();
            match word {
                "valid" | "invalid" => {
                    current = Some(Vector { valid: word == "valid", description: rest.to_string(), raw: None, keys: BTreeMap::new() })
                }
                "psbt" => {
                    let mut raw = RawPsbt::parse(&data_encoding::BASE64.decode(rest.as_bytes()).unwrap()).ok();
                    raw.iter_mut().for_each(with_scripts_of_their_keys);
                    v.unwrap().raw = raw;
                }
                "key" => {
                    let parts: Vec<&str> = rest.split(' ').collect();
                    v.unwrap().keys.insert(parts[0].parse().unwrap(), SecretKey::from_slice(&hex(parts[1])).unwrap());
                }
                "end" => out.push(current.take().unwrap()),
                _ => panic!("unknown line {line}"),
            }
        }
        out
    }

    /// Why an invalid vector must be refused, as words of SeedLock's message.
    fn refusal(description: &str) -> &'static str {
        let has = |words: &str| description.contains(words);
        if has("incorrect byte length") {
            "wrong length"
        } else if has("PSBT_OUT_SP_V0_LABEL set") {
            "no silent payment address"
        } else if has("TX_MODIFIABLE") {
            "may still be changed"
        } else if has("sending to non-sp output") {
            "neither a script nor"
        } else if has("ineligible") || has("NUMS") {
            "needs at least one input"
        } else if has("missing PSBT_IN_SP_DLEQ") {
            "has no proof"
        } else if has("missing PSBT_GLOBAL_SP_DLEQ") {
            "without its proof"
        } else if has("invalid proof") {
            "is wrong"
        } else if has("missing PSBT_IN_BIP32_DERIVATION") {
            "does not say its public key"
        } else if has("missing ECDH share") || has("missing PSBT_IN_SP_ECDH_SHARE") {
            "must add theirs first"
        } else if has("segwit version") {
            "SegWit version above 1"
        } else if has("SIGHASH_ALL") {
            "SIGHASH_ALL"
        } else if description.starts_with("output scripts") {
            "is not the one of the silent payment address"
        } else {
            panic!("no reason known for {description}")
        }
    }

    /// The private key a vector's input adds for silent payments: for a taproot output, the one of its output key
    /// with an even y (a vector gives it, or the internal key it comes from).
    fn vector_key(view: &Psbt, i: usize, key: SecretKey) -> SecretKey {
        let secp = Secp256k1::new();
        let script = input_txout(view, i).unwrap().script_pubkey;
        if !script.is_p2tr() {
            return key;
        }
        let (x, parity) = key.x_only_public_key(&secp);
        if x.serialize()[..] == script.as_bytes()[2..34] {
            return if parity == Parity::Odd { key.negate() } else { key };
        }
        taproot_output_key(&secp, &key)
    }

    /// The PSBT without the fields a signer adds for silent payments: shares, proofs, the scripts of the silent
    /// payment outputs, and its "modifiable" flags not cleared yet.
    fn before_signing(raw: &RawPsbt) -> RawPsbt {
        let mut raw = raw.clone();
        raw.global.0.retain(|(k, _)| ![GLOBAL_SP_ECDH_SHARE, GLOBAL_SP_DLEQ].contains(&k[0]));
        for map in &mut raw.inputs {
            map.0.retain(|(k, _)| ![IN_SP_ECDH_SHARE, IN_SP_DLEQ].contains(&k[0]));
        }
        without_scripts(&mut raw);
        raw.global.set(GLOBAL_TX_MODIFIABLE, &[], vec![0b11]);
        raw
    }

    fn without_scripts(raw: &mut RawPsbt) {
        for map in raw.outputs.iter_mut().filter(|m| m.has(OUT_SP_V0_INFO)) {
            map.0.retain(|(k, _)| k[0] != OUT_SCRIPT);
        }
    }

    fn eligible_inputs(raw: &RawPsbt) -> Vec<usize> {
        let view = to_v0_with_stand_ins(raw).unwrap();
        (0..raw.inputs.len()).filter(|&i| eligibility(&view, &raw.inputs[i], i).unwrap().0).collect()
    }

    /// What a signer that holds these keys (by input index) does with a PSBT's silent payments.
    fn sign_as(raw: &RawPsbt, keys: &BTreeMap<usize, SecretKey>) -> Result<RawPsbt> {
        check_fields(raw)?;
        let targets = targets(raw)?;
        let view = to_v0_with_stand_ins(raw)?;
        let mut input_keys = Vec::new();
        for (i, map) in raw.inputs.iter().enumerate() {
            let (eligible, public) = eligibility(&view, map, i)?;
            let key = keys.get(&i).filter(|_| eligible).map(|k| vector_key(&view, i, *k));
            input_keys.push(InputKey { eligible, key, public });
        }
        let mut done = raw.clone();
        if targets.iter().any(Option::is_some) {
            complete(&mut done, &view, &targets, &input_keys)?;
        }
        Ok(done)
    }

    fn same_scripts(done: &RawPsbt, raw: &RawPsbt, description: &str) {
        for (j, map) in done.outputs.iter().enumerate() {
            assert_eq!(map.get(OUT_SCRIPT), raw.outputs[j].get(OUT_SCRIPT), "{description}: output #{j}");
        }
    }

    #[test]
    fn bip375_vectors_as_a_signer_that_holds_none_of_the_keys() {
        let all = vectors();
        assert_eq!(all.len(), 43);
        let (mut finalizable, mut refused) = (0, 0);
        for v in all {
            let result = v.raw.as_ref().ok_or_else(|| refuse("unreadable".into())).and_then(|raw| sign_as(raw, &BTreeMap::new()));
            if !v.valid {
                let e = result.err().unwrap_or_else(|| panic!("{}: accepted", v.description));
                assert!(e.to_string().contains(refusal(&v.description)), "{}: {e}", v.description);
                refused += 1;
            } else if v.description == K_IN_OUTPUT_ORDER {
                let e = result.err().unwrap();
                assert!(e.to_string().contains("is not the one of the silent payment address"), "{e}");
            } else if v.description.starts_with("can finalize") {
                // its shares and proofs check out, and its scripts come out the same
                same_scripts(&result.unwrap_or_else(|e| panic!("{}: {e}", v.description)), &v.raw.unwrap(), &v.description);
                finalizable += 1;
            } else if let Err(e) = result {
                // in progress, or an input of a bare OP_2 script (no SegWit version 2): never a broken PSBT
                let e = e.to_string();
                assert!(["must add theirs first", "needs at least one input"].iter().any(|w| e.contains(w)), "{}: {e}", v.description);
            }
        }
        assert_eq!((finalizable, refused), (13, 23));
    }

    #[test]
    fn bip375_vectors_as_the_signer_that_holds_all_the_keys() {
        // what such a signer adds comes out as in the vectors: the same shares, the same scripts, good proofs
        let secp = Secp256k1::new();
        let mut signed = 0;
        for v in vectors().into_iter().filter(|v| v.valid && v.description.starts_with("can finalize")) {
            let raw = v.raw.unwrap();
            let eligible = eligible_inputs(&raw);
            if eligible.iter().any(|i| !v.keys.contains_key(i)) {
                continue;
            }
            let done = sign_as(&before_signing(&raw), &v.keys).unwrap_or_else(|e| panic!("{}: {e}", v.description));
            if v.description != K_IN_OUTPUT_ORDER {
                same_scripts(&done, &raw, &v.description);
            }
            let view = to_v0_with_stand_ins(&raw).unwrap();
            let a: Vec<SecretKey> = eligible.iter().map(|&i| vector_key(&view, i, v.keys[&i])).collect();
            let big_a = PublicKey::from_secret_key(&secp, &silent::sum_private_keys(&a).unwrap());
            let shares: Vec<(&[u8], &[u8])> = done.global.entries(GLOBAL_SP_ECDH_SHARE).collect();
            assert!(!shares.is_empty(), "{}", v.description);
            for (scan, share) in shares {
                // the vector's share for this scan key: global, or the sum of its eligible inputs' shares
                let theirs = match raw.global.entries(GLOBAL_SP_ECDH_SHARE).find(|(k, _)| *k == scan) {
                    Some((_, s)) => s.to_vec(),
                    None => {
                        let per_input: Vec<PublicKey> = eligible
                            .iter()
                            .filter_map(|&i| raw.inputs[i].entries(IN_SP_ECDH_SHARE).find(|(k, _)| *k == scan))
                            .map(|(_, s)| PublicKey::from_slice(s).unwrap())
                            .collect();
                        PublicKey::combine_keys(&per_input.iter().collect::<Vec<_>>()).unwrap().serialize().to_vec()
                    }
                };
                assert_eq!(share, theirs.as_slice(), "{}", v.description);
                let proof: [u8; 64] = done.global.entries(GLOBAL_SP_DLEQ).find(|(k, _)| *k == scan).unwrap().1.try_into().unwrap();
                let (scan, share) = (PublicKey::from_slice(scan).unwrap(), PublicKey::from_slice(share).unwrap());
                assert!(silent::dleq_verify(&big_a, &scan, &share, &proof), "{}", v.description);
            }
            assert_eq!(done.global.get(GLOBAL_TX_MODIFIABLE), Some(&[0u8][..]), "{}", v.description);
            // what it made is checked by a signer that holds none of the keys
            same_scripts(&sign_as(&done, &BTreeMap::new()).unwrap(), &done, &v.description);
            signed += 1;
        }
        assert!(signed >= 8, "only {signed} vectors had all their keys");
    }

    #[test]
    fn a_signer_with_some_of_the_keys_adds_its_shares_and_checks_the_others() {
        // the vectors with per-input shares from two inputs or more: one input's share and proof taken away, then
        // put back by its owner, who checks the others' shares
        let mut checked = 0;
        for v in vectors().into_iter().filter(|v| v.valid && v.description.starts_with("can finalize")) {
            let raw = v.raw.unwrap();
            let eligible = eligible_inputs(&raw);
            if raw.global.has(GLOBAL_SP_ECDH_SHARE) || eligible.len() < 2 {
                continue;
            }
            for &i in eligible.iter().filter(|i| v.keys.contains_key(i)) {
                let owner = BTreeMap::from([(i, v.keys[&i])]);
                let mut bare = raw.clone();
                bare.inputs[i].0.retain(|(k, _)| ![IN_SP_ECDH_SHARE, IN_SP_DLEQ].contains(&k[0]));
                without_scripts(&mut bare);
                let done = sign_as(&bare, &owner).unwrap_or_else(|e| panic!("{}: {e}", v.description));
                same_scripts(&done, &raw, &v.description);
                assert_eq!(
                    done.inputs[i].entries(IN_SP_ECDH_SHARE).collect::<Vec<_>>(),
                    raw.inputs[i].entries(IN_SP_ECDH_SHARE).collect::<Vec<_>>(),
                    "{}",
                    v.description
                );
                // another input's proof broken, or its share missing
                let other = *eligible.iter().find(|&&o| o != i).unwrap();
                let mut broken = bare.clone();
                broken.inputs[other].0.iter_mut().find(|(k, _)| k[0] == IN_SP_DLEQ).unwrap().1[63] ^= 1;
                let e = sign_as(&broken, &owner).err().unwrap();
                assert!(e.to_string().contains("the proof of its ECDH share is wrong"), "{}: {e}", v.description);
                let mut missing = bare.clone();
                missing.inputs[other].0.retain(|(k, _)| ![IN_SP_ECDH_SHARE, IN_SP_DLEQ].contains(&k[0]));
                let e = sign_as(&missing, &owner).err().unwrap();
                assert!(e.to_string().contains("must add theirs first"), "{}: {e}", v.description);
                checked += 1;
            }
        }
        assert!(checked >= 2, "only {checked}");
    }

    // ------------------------------------------------------------------ PSBTs as Sparrow sends them

    use crate::psbt::{OutputKind, Request};
    use bitcoin::bip32::DerivationPath;
    use bitcoin::secp256k1::Message;
    use bitcoin::sighash::{EcdsaSighashType, Prevouts, SighashCache};
    use bitcoin::{Address, Amount, Network, OutPoint, ScriptBuf, TxOut, Txid};
    use std::str::FromStr;

    const ABANDON: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn wallet() -> Wallet {
        Wallet::new(&crate::mnemonic::parse(ABANDON).unwrap(), "", Network::Bitcoin).unwrap()
    }

    fn path(text: &str) -> DerivationPath {
        DerivationPath::from_str(text).unwrap()
    }

    /// A key origin as a PSBT writes it: the fingerprint, then the path's steps.
    fn origin(fingerprint: Fingerprint, path: &DerivationPath) -> Vec<u8> {
        let mut value = fingerprint.to_bytes().to_vec();
        for step in path {
            value.extend_from_slice(&u32::from(*step).to_le_bytes());
        }
        value
    }

    fn input(outpoint: OutPoint, utxo: &TxOut) -> Map {
        let mut map = Map::default();
        map.set(IN_WITNESS_UTXO, &[], consensus::serialize(utxo));
        map.set(IN_PREVIOUS_TXID, &[], outpoint.txid.to_byte_array().to_vec());
        map.set(IN_OUTPUT_INDEX, &[], outpoint.vout.to_le_bytes().to_vec());
        map.set(IN_SEQUENCE, &[], 0xffff_fffd_u32.to_le_bytes().to_vec());
        map
    }

    /// An input of the wallet's, at a BIP 84 path.
    fn wallet_input(w: &Wallet, outpoint: OutPoint, at: &str, sats: u64) -> (Map, TxOut, PublicKey) {
        let at = path(at);
        let a = w.public_key_at(&at).unwrap();
        let utxo = TxOut { value: Amount::from_sat(sats), script_pubkey: w.script_pubkey(ScriptType::Wpkh, &a) };
        let mut map = input(outpoint, &utxo);
        map.set(IN_BIP32_DERIVATION, &a.serialize(), origin(w.fingerprint(), &at));
        (map, utxo, a)
    }

    fn paying(sats: u64, script: &ScriptBuf) -> Map {
        let mut map = Map::default();
        map.set(OUT_AMOUNT, &[], sats.to_le_bytes().to_vec());
        map.set(OUT_SCRIPT, &[], script.to_bytes());
        map
    }

    /// An output to a silent payment address, without its script (Sparrow leaves it to the signer).
    fn paying_silently(sats: u64, scan: &PublicKey, spend: &PublicKey, label: Option<u32>) -> Map {
        let mut map = Map::default();
        map.set(OUT_AMOUNT, &[], sats.to_le_bytes().to_vec());
        let mut info = scan.serialize().to_vec();
        info.extend_from_slice(&spend.serialize());
        map.set(OUT_SP_V0_INFO, &[], info);
        if let Some(m) = label {
            map.set(OUT_SP_V0_LABEL, &[], m.to_le_bytes().to_vec());
        }
        map
    }

    fn sparrow(inputs: Vec<Map>, outputs: Vec<Map>) -> RawPsbt {
        let mut global = Map::default();
        global.set(GLOBAL_TX_VERSION, &[], 2u32.to_le_bytes().to_vec());
        global.set(GLOBAL_FALLBACK_LOCKTIME, &[], 0u32.to_le_bytes().to_vec());
        global.set(GLOBAL_INPUT_COUNT, &[], vec![inputs.len() as u8]);
        global.set(GLOBAL_OUTPUT_COUNT, &[], vec![outputs.len() as u8]);
        global.set(GLOBAL_TX_MODIFIABLE, &[], vec![0b11]);
        global.set(GLOBAL_VERSION, &[], 2u32.to_le_bytes().to_vec());
        RawPsbt { global, inputs, outputs }
    }

    fn external() -> ScriptBuf {
        ScriptBuf::from_bytes(hex("0014751e76e8199196d454941c45d1b3a323f1433bd6"))
    }

    fn outpoint(byte: u8, vout: u32) -> OutPoint {
        OutPoint { txid: Txid::from_byte_array([byte; 32]), vout }
    }

    /// The key of the output that pays (scan key, spend key) with this k, as its receiver finds it (BIP 352): from
    /// the scan key's private key, the smallest outpoint, and the sum of the inputs' public keys.
    fn found(b_scan: &SecretKey, spend: &PublicKey, smallest: &OutPoint, a: &PublicKey, k: u32) -> [u8; 32] {
        let smallest: [u8; 36] = consensus::serialize(smallest).try_into().unwrap();
        let input_hash = silent::input_hash(&smallest, a).unwrap();
        let shared = silent::shared_secret(&input_hash, &silent::ecdh_share(b_scan, a).unwrap()).unwrap();
        silent::output_key(&shared, spend, k).unwrap().serialize()
    }

    fn output_key(raw: &RawPsbt, j: usize) -> [u8; 32] {
        let script = raw.outputs[j].get(OUT_SCRIPT).unwrap();
        assert_eq!(script[..2], [0x51, 0x20]);
        script[2..].try_into().unwrap()
    }

    fn scan_private(w: &Wallet) -> SecretKey {
        w.private_key_at(&path("m/352'/0'/0'/1'/0")).unwrap()
    }

    #[test]
    fn pays_a_silent_payment_address_and_its_own_as_change() {
        let secp = Secp256k1::new();
        let w = wallet();
        let keys = SilentKeys::new(&w, 0).unwrap();
        let (b_scan, b_spend) = (SecretKey::from_slice(&[0x11; 32]).unwrap(), SecretKey::from_slice(&[0x22; 32]).unwrap());
        let (scan, spend) = (b_scan.public_key(&secp), b_spend.public_key(&secp));
        // two inputs of the wallet's: the smallest outpoint is the second one's
        let (first, second) = (outpoint(0xbb, 0), outpoint(0xaa, 7));
        let (i0, u0, a0) = wallet_input(&w, first, "m/84'/0'/0'/0/0", 70_000);
        let (i1, u1, a1) = wallet_input(&w, second, "m/84'/0'/0'/0/1", 30_000);
        let change = keys.labeled_spend(silent::CHANGE_LABEL).unwrap();
        let raw = sparrow(
            vec![i0, i1],
            vec![paying_silently(60_000, &scan, &spend, None), paying_silently(39_000, &keys.scan_public(), &change, Some(0))],
        );

        let mut request = Request::from_bytes(&raw.serialize()).unwrap();
        assert_eq!(request.version(), 2);
        assert!(request.has_silent_payments());
        let review = request.review(&w, &[]).unwrap();
        assert_eq!(review.outputs[0].kind, OutputKind::External);
        assert_eq!(review.outputs[0].address, silent::encode_address(&scan, &spend, Network::Bitcoin));
        assert_eq!(review.outputs[1].kind, OutputKind::SilentChange);
        assert_eq!(review.outputs[1].address, keys.labeled_address(silent::CHANGE_LABEL).unwrap());
        assert_eq!((review.sending.to_sat(), review.fee.to_sat(), review.own_inputs), (60_000, 1_000, 2));
        assert!(review.warnings.is_empty(), "{:?}", review.warnings);
        assert_eq!(request.sign(&w, &[]).unwrap(), 2);

        // what goes back to Sparrow: version 2, with the shares, their proofs, the scripts, nothing modifiable
        let signed = RawPsbt::parse(&request.serialize()).unwrap();
        assert_eq!(signed.version(), 2);
        assert_eq!(signed.global.get(GLOBAL_TX_MODIFIABLE), Some(&[0u8][..]));
        assert_eq!(signed.global.entries(GLOBAL_SP_ECDH_SHARE).count(), 2);
        let a = PublicKey::combine_keys(&[&a0, &a1]).unwrap();
        for (scan_key, share) in signed.global.entries(GLOBAL_SP_ECDH_SHARE) {
            let proof = signed.global.entries(GLOBAL_SP_DLEQ).find(|(k, _)| *k == scan_key).unwrap().1;
            let (scan_key, share) = (PublicKey::from_slice(scan_key).unwrap(), PublicKey::from_slice(share).unwrap());
            assert!(silent::dleq_verify(&a, &scan_key, &share, proof.try_into().unwrap()));
        }
        // the receiver finds its payment, the wallet its change
        assert_eq!(output_key(&signed, 0), found(&b_scan, &spend, &second, &a, 0));
        assert_eq!(output_key(&signed, 1), found(&scan_private(&w), &change, &second, &a, 0));
        // the review showed the scripts that were signed, and the signatures are over them
        let v0 = to_v0(&signed).unwrap();
        for (j, out) in v0.unsigned_tx.output.iter().enumerate() {
            assert_eq!(review.outputs[j].silent_payment, Some(Address::from_script(&out.script_pubkey, Network::Bitcoin).unwrap().to_string()));
        }
        for (i, (utxo, a)) in [(&u0, a0), (&u1, a1)].into_iter().enumerate() {
            let hash = SighashCache::new(&v0.unsigned_tx).p2wpkh_signature_hash(i, &utxo.script_pubkey, utxo.value, EcdsaSighashType::All).unwrap();
            let signature = v0.inputs[i].partial_sigs[&bitcoin::PublicKey::new(a)].signature;
            secp.verify_ecdsa(&Message::from_digest(hash.to_byte_array()), &signature, &a).unwrap();
        }
        // and a signer that holds none of the keys finds it all in order
        assert_eq!(sign_as(&signed, &BTreeMap::new()).unwrap().outputs, signed.outputs);
    }

    #[test]
    fn several_payments_to_one_silent_payment_address_get_k_by_spend_key() {
        // two labels of one receiver and its address twice: k counts by spend key, then by output index (the
        // receiver tries k = 0, 1, 2... against every output, so it finds them in any order)
        let w = wallet();
        let receiver = SilentKeys::from_private_keys(SecretKey::from_slice(&[0x44; 32]).unwrap(), SecretKey::from_slice(&[0x55; 32]).unwrap(), Network::Bitcoin);
        let b_scan = SecretKey::from_slice(&[0x44; 32]).unwrap();
        let spends = [receiver.labeled_spend(3).unwrap(), receiver.spend_public(), receiver.labeled_spend(1).unwrap(), receiver.spend_public()];
        let (i0, _, a) = wallet_input(&w, outpoint(0xcc, 2), "m/84'/0'/0'/0/0", 100_000);
        let outputs = spends.iter().map(|s| paying_silently(20_000, &receiver.scan_public(), s, None)).collect();
        let mut request = Request::from_bytes(&sparrow(vec![i0], outputs).serialize()).unwrap();
        request.sign(&w, &[]).unwrap();
        let signed = RawPsbt::parse(&request.serialize()).unwrap();
        let mut order: Vec<usize> = (0..4).collect();
        order.sort_by_key(|&j| (spends[j].serialize(), j));
        for (k, &j) in order.iter().enumerate() {
            assert_eq!(output_key(&signed, j), found(&b_scan, &spends[j], &outpoint(0xcc, 2), &a, k as u32), "output #{j}");
        }
    }

    #[test]
    fn spends_what_its_silent_payment_address_received() {
        let secp = Secp256k1::new();
        let w = wallet();
        let keys = SilentKeys::new(&w, 0).unwrap();
        // an output that a silent payment made to the wallet: its spend key plus a tweak (BIP 376)
        let tweak = [0x33u8; 32];
        let d = keys.spending_key(&tweak).unwrap();
        let (x, _) = d.x_only_public_key(&secp);
        let mut script = vec![0x51, 0x20];
        script.extend_from_slice(&x.serialize());
        let utxo = TxOut { value: Amount::from_sat(100_000), script_pubkey: ScriptBuf::from_bytes(script) };
        let spent = outpoint(0xdd, 3);
        let mut i0 = input(spent, &utxo);
        i0.set(IN_SP_TWEAK, &[], tweak.to_vec());
        i0.set(IN_SP_SPEND_BIP32_DERIVATION, &keys.spend_public().serialize(), origin(w.fingerprint(), &keys.spend_path()));
        let change = keys.labeled_spend(silent::CHANGE_LABEL).unwrap();
        let raw = sparrow(vec![i0], vec![paying(60_000, &external()), paying_silently(39_000, &keys.scan_public(), &change, Some(0))]);

        let mut request = Request::from_bytes(&raw.serialize()).unwrap();
        let review = request.review(&w, &[]).unwrap();
        assert_eq!(review.inputs[0].path.as_deref(), Some("silent payment, m/352'/0'/0'/0'/0"));
        assert_eq!(review.outputs[0].kind, OutputKind::External);
        assert_eq!(review.outputs[1].kind, OutputKind::SilentChange);
        assert_eq!((review.sending.to_sat(), review.own_inputs), (60_000, 1));
        assert!(review.warnings.is_empty(), "{:?}", review.warnings);
        assert_eq!(request.sign(&w, &[]).unwrap(), 1);

        // a BIP 340 signature by the output's key, over the transaction with its change worked out
        let signed = RawPsbt::parse(&request.serialize()).unwrap();
        let v0 = to_v0(&signed).unwrap();
        let sig = v0.inputs[0].tap_key_sig.unwrap();
        let hash = SighashCache::new(&v0.unsigned_tx)
            .taproot_key_spend_signature_hash(0, &Prevouts::All(std::slice::from_ref(&utxo)), sig.sighash_type)
            .unwrap();
        secp.verify_schnorr(&sig.signature, &Message::from_digest(hash.to_byte_array()), &x).unwrap();
        // the wallet finds its change: the input's key is the output's (with an even y)
        let a = PublicKey::from_x_only_public_key(x, Parity::Even);
        assert_eq!(output_key(&signed, 1), found(&scan_private(&w), &change, &spent, &a, 0));

        // a tweak that does not give the output's key, on an input that says it is the wallet's: refused
        let mut wrong = raw.clone();
        wrong.inputs[0].set(IN_SP_TWEAK, &[], vec![0x34; 32]);
        let e = Request::from_bytes(&wrong.serialize()).unwrap().review(&w, &[]).err().unwrap();
        assert!(e.to_string().contains("its tweak does not give the key"), "{e}");
        // as is one without a tweak
        let mut untweaked = raw.clone();
        untweaked.inputs[0].0.retain(|(k, _)| k[0] != IN_SP_TWEAK);
        let e = Request::from_bytes(&untweaked.serialize()).unwrap().review(&w, &[]).err().unwrap();
        assert!(e.to_string().contains("has no tweak"), "{e}");
        // another seed finds nothing of its own to sign
        let other = Wallet::new(
            &crate::mnemonic::parse("legal winner thank year wave sausage worth useful legal winner thank yellow").unwrap(),
            "",
            Network::Bitcoin,
        )
        .unwrap();
        let mut anonymous = raw.clone();
        anonymous.inputs[0].0.retain(|(k, _)| k[0] != IN_SP_SPEND_BIP32_DERIVATION);
        let e = Request::from_bytes(&anonymous.serialize()).unwrap().sign(&other, &[]).err().unwrap();
        assert!(e.to_string().contains("none of the inputs belong"), "{e}");
    }

    #[test]
    fn a_plain_version_2_psbt_is_signed_and_goes_back_as_version_2() {
        let w = wallet();
        let (i0, _, _) = wallet_input(&w, outpoint(0xee, 0), "m/84'/0'/0'/0/0", 100_000);
        let raw = sparrow(vec![i0], vec![paying(99_000, &external())]);
        let mut request = Request::from_bytes(&raw.serialize()).unwrap();
        assert!(!request.has_silent_payments());
        assert_eq!(request.sign(&w, &[]).unwrap(), 1);
        let signed = RawPsbt::parse(&request.serialize()).unwrap();
        assert_eq!(signed.version(), 2);
        // the signature added, and nothing modifiable any more (BIP 370)
        assert_eq!(signed.inputs[0].0.len(), raw.inputs[0].0.len() + 1);
        assert_eq!(signed.outputs, raw.outputs);
        assert_eq!(signed.global.get(GLOBAL_TX_MODIFIABLE), Some(&[0u8][..]));
        // a version 0 PSBT may not carry fields of silent payments
        let mut v0 = to_v0(&raw).unwrap();
        v0.outputs[0].unknown.insert(bitcoin::psbt::raw::Key { type_value: OUT_SP_V0_LABEL, key: vec![] }, vec![0; 4]);
        let e = Request::from_bytes(&v0.serialize()).err().unwrap();
        assert!(e.to_string().contains("version 2"), "{e}");
    }
}
