//! PSBT review and signing for single-signature wallets.
//!
//! The flow mirrors SeedSigner:
//!
//! 1. [`decode`] the PSBT (binary, base64 or hex),
//! 2. [`review`] it: every input amount, every output address, which outputs are
//!    verified change, the fee, and warnings,
//! 3. the person confirms, then [`sign`] adds the signatures.
//!
//! Rules enforced here (the PSBT is refused otherwise):
//!
//! * every input must carry its previous output, so amounts and fee are known;
//! * a legacy input's previous transaction must hash to the referenced txid;
//! * only `SIGHASH_ALL` (or taproot default) is signed;
//! * an output is labelled *change* only if its script is **re-derived from the
//!   seed** and matches. A PSBT that merely says "this is change" is not trusted;
//! * inputs that claim to be ours but are not plain single-signature scripts
//!   are refused instead of being blindly signed, unless they belong to a
//!   **registered multisig wallet** ([`review_with`], [`sign_with`]) that
//!   rebuilds exactly their script from all its cosigners' keys; the same goes
//!   for multisig change.

use crate::multisig::{MultisigScript, MultisigWallet};
use crate::wallet::{path_to_display, ScriptType, Wallet};
use crate::{Error, Result};
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, KeySource};
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1;
use bitcoin::sighash::{EcdsaSighashType, TapSighashType};
use bitcoin::taproot::TapLeafHash;
use bitcoin::{Address, Amount, Network, OutPoint, PublicKey, ScriptBuf, TxOut, XOnlyPublicKey};
use std::collections::BTreeMap;

/// What an output is, as far as this seed can prove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// Not provably ours: a payment to someone else.
    External,
    /// Verified change: re-derived from this seed on the change chain.
    Change { path: String },
    /// Verified: pays to one of this wallet's own receive addresses.
    OwnReceive { path: String },
    /// Verified change of a registered multisig wallet (its script rebuilt from all its cosigners' keys).
    MultisigChange { path: String, wallet: String },
    /// Verified: one of a registered multisig wallet's receive addresses.
    MultisigReceive { path: String, wallet: String },
}

/// One input as shown to the person.
#[derive(Clone, Debug)]
pub struct InputReview {
    pub index: usize,
    pub outpoint: OutPoint,
    pub value: Amount,
    pub address: Option<String>,
    /// Derivation path, only when the input is verified to belong to this wallet.
    pub path: Option<String>,
    /// The registered multisig wallet the input belongs to, if it is a multisig input.
    pub multisig: Option<String>,
    /// The key this seed signs the input with (for signatures that go into partial_sigs).
    pub key: Option<secp256k1::PublicKey>,
}

/// About the multisig part of a transaction.
#[derive(Clone, Debug)]
pub struct MultisigSummary {
    /// The name the wallet was registered under.
    pub name: String,
    pub threshold: usize,
    pub cosigners: usize,
    /// This seed's place among the cosigners (1-based, in the order of the descriptor).
    pub position: usize,
    /// Signatures of other cosigners already in the transaction (the fewest over its inputs).
    pub signed_by_others: usize,
}

/// One output as shown to the person.
#[derive(Clone, Debug)]
pub struct OutputReview {
    pub index: usize,
    pub value: Amount,
    pub address: String,
    pub kind: OutputKind,
}

/// Everything a person should look at before agreeing to sign.
#[derive(Clone, Debug)]
pub struct Review {
    pub network: Network,
    pub fingerprint: Fingerprint,
    pub inputs: Vec<InputReview>,
    pub outputs: Vec<OutputReview>,
    pub total_in: Amount,
    pub total_out: Amount,
    pub fee: Amount,
    /// Sum of the outputs that leave the wallet (everything not verified own/change).
    pub sending: Amount,
    /// Number of inputs this seed will sign.
    pub own_inputs: usize,
    /// Present when the inputs this seed signs belong to a registered multisig wallet.
    pub multisig: Option<MultisigSummary>,
    pub warnings: Vec<String>,
}

/// Format satoshis as BTC with 8 decimals, e.g. `0.00150000`.
pub fn format_btc(amount: Amount) -> String {
    let sats = amount.to_sat();
    format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000)
}

/// Read a PSBT from raw bytes: binary, base64 text or hex text.
pub fn decode(data: &[u8]) -> Result<Psbt> {
    let bytes: Vec<u8> = if data.starts_with(b"psbt\xff") {
        data.to_vec()
    } else {
        let text = std::str::from_utf8(data)
            .map_err(|_| Error::Psbt("this is not a PSBT (unrecognised file format)".into()))?;
        let compact: String = text.split_whitespace().collect();
        if let Ok(b) = data_encoding::BASE64.decode(compact.as_bytes()) {
            b
        } else if let Ok(b) = data_encoding::HEXLOWER_PERMISSIVE.decode(compact.as_bytes()) {
            b
        } else {
            return Err(Error::Psbt("this is not a PSBT (expected binary, base64 or hex)".into()));
        }
    };
    Psbt::deserialize(&bytes).map_err(|e| Error::Psbt(format!("invalid PSBT: {e}")))
}

/// Base64 text form, as accepted by Sparrow, Bitcoin Core, etc.
pub fn encode_base64(psbt: &Psbt) -> String {
    data_encoding::BASE64.encode(&psbt.serialize())
}

/// Inspect a PSBT against a wallet without changing it.
pub fn review(psbt: &Psbt, wallet: &Wallet) -> Result<Review> {
    review_with(psbt, wallet, &[])
}

/// [`review`], knowing these registered multisig wallets (name, wallet).
pub fn review_with(psbt: &Psbt, wallet: &Wallet, multisig: &[(String, MultisigWallet)]) -> Result<Review> {
    let network = wallet.network();
    let n_in = psbt.unsigned_tx.input.len();
    let n_out = psbt.unsigned_tx.output.len();
    if n_in == 0 || n_out == 0 {
        return Err(Error::Psbt("the transaction has no inputs or no outputs".into()));
    }
    if psbt.inputs.len() != n_in || psbt.outputs.len() != n_out {
        return Err(Error::Psbt("malformed PSBT: input/output counts do not match the transaction".into()));
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut inputs = Vec::with_capacity(n_in);
    let mut total_in = Amount::ZERO;
    let mut own_inputs = 0usize;
    let mut summary: Option<MultisigSummary> = None;

    for i in 0..n_in {
        let txout = input_txout(psbt, i)?;
        check_sighash(psbt, i, &txout)?;
        let inp = &psbt.inputs[i];
        let claim = ownership(
            wallet,
            &txout,
            &inp.bip32_derivation,
            &inp.tap_key_origins,
            inp.witness_script.as_ref(),
            inp.redeem_script.as_ref(),
            multisig,
            &mut warnings,
            &format!("input #{i}"),
        )?;
        let (path, in_multisig, key) = match claim {
            Ownership::NotClaimed => (None, None, None),
            Ownership::Verified(o) => {
                own_inputs += 1;
                (Some(path_to_display(&o.path)), None, o.key)
            }
            Ownership::VerifiedMultisig(o) => {
                let (name, ms) = &multisig[o.wallet];
                let (redeem, witness) = ms.psbt_scripts(o.chain, o.index)?;
                if (witness.is_some() && inp.witness_script.is_none()) || (redeem.is_some() && inp.redeem_script.is_none()) {
                    return Err(Error::Psbt(format!(
                        "input #{i} of the multisig wallet \"{name}\" comes without its script in the PSBT, so it cannot be signed"
                    )));
                }
                if ms.script == MultisigScript::Sh && inp.non_witness_utxo.is_none() {
                    return Err(Error::Psbt(format!(
                        "input #{i} is a legacy multisig input without its previous transaction, so its amount cannot be \
                         verified; refusing to sign"
                    )));
                }
                own_inputs += 1;
                let ours = PublicKey::new(o.key);
                let script_keys = ms.keys_at(o.chain, o.index)?;
                let others = inp.partial_sigs.keys().filter(|k| **k != ours && script_keys.contains(k)).count();
                let position = ms.position(wallet.fingerprint()).map_or(0, |p| p + 1);
                match &mut summary {
                    Some(s) => s.signed_by_others = s.signed_by_others.min(others),
                    None => {
                        summary = Some(MultisigSummary {
                            name: name.clone(),
                            threshold: ms.threshold,
                            cosigners: ms.cosigners.len(),
                            position,
                            signed_by_others: others,
                        })
                    }
                }
                (Some(path_to_display(&o.path)), Some(name.clone()), Some(o.key))
            }
            Ownership::Unverified => {
                let looks_multisig = inp.witness_script.as_ref().is_some_and(|w| w.is_multisig())
                    || inp.redeem_script.as_ref().is_some_and(|r| r.is_multisig());
                return Err(Error::Psbt(if looks_multisig {
                    format!(
                        "input #{i} belongs to a multisig wallet that is not registered here, or that does not match its \
                         registration. Register the wallet first (menu 10), with the output descriptor that Sparrow exports \
                         (File > Export Wallet > Output Descriptor); refusing to sign"
                    )
                } else {
                    format!(
                        "input #{i} says it belongs to your wallet, but it is not a plain single-signature \
                         input that this seed can reproduce, nor an input of a registered multisig wallet; refusing to sign"
                    )
                }));
            }
        };
        total_in = total_in
            .checked_add(txout.value)
            .ok_or_else(|| Error::Psbt("input amounts overflow".into()))?;
        inputs.push(InputReview {
            index: i,
            outpoint: psbt.unsigned_tx.input[i].previous_output,
            value: txout.value,
            address: Address::from_script(&txout.script_pubkey, network).ok().map(|a| a.to_string()),
            path,
            multisig: in_multisig,
            key,
        });
    }

    let mut outputs = Vec::with_capacity(n_out);
    let mut total_out = Amount::ZERO;
    let mut sending = Amount::ZERO;
    for (j, out) in psbt.unsigned_tx.output.iter().enumerate() {
        let po = &psbt.outputs[j];
        let kind = match ownership(
            wallet,
            out,
            &po.bip32_derivation,
            &po.tap_key_origins,
            po.witness_script.as_ref(),
            po.redeem_script.as_ref(),
            multisig,
            &mut warnings,
            &format!("output #{j}"),
        )? {
            Ownership::NotClaimed => OutputKind::External,
            Ownership::Unverified => {
                warnings.push(format!(
                    "output #{j} claims to be yours, but its address does not match your seed. It is treated as a payment to someone else."
                ));
                OutputKind::External
            }
            Ownership::Verified(o) => {
                let path = path_to_display(&o.path);
                if o.chain == 1 {
                    OutputKind::Change { path }
                } else {
                    OutputKind::OwnReceive { path }
                }
            }
            Ownership::VerifiedMultisig(o) => {
                let path = path_to_display(&o.path);
                let wallet = multisig[o.wallet].0.clone();
                if o.chain == 1 {
                    OutputKind::MultisigChange { path, wallet }
                } else {
                    OutputKind::MultisigReceive { path, wallet }
                }
            }
        };
        if out.script_pubkey.is_op_return() {
            warnings.push(format!("output #{j} is a data (OP_RETURN) output."));
        }
        total_out = total_out
            .checked_add(out.value)
            .ok_or_else(|| Error::Psbt("output amounts overflow".into()))?;
        if kind == OutputKind::External {
            sending = sending.checked_add(out.value).unwrap_or(Amount::MAX);
        }
        outputs.push(OutputReview {
            index: j,
            value: out.value,
            address: describe_script(out, network),
            kind,
        });
    }

    let fee = total_in
        .checked_sub(total_out)
        .ok_or_else(|| Error::Psbt("the outputs are worth more than the inputs: invalid transaction".into()))?;

    if sending.to_sat() > 0 && fee.to_sat().saturating_mul(20) > sending.to_sat() {
        warnings.push(format!(
            "the fee ({} BTC) is more than 5% of the amount being sent.",
            format_btc(fee)
        ));
    }
    if fee.to_sat() > 1_000_000 {
        warnings.push(format!("the fee is unusually high: {} BTC.", format_btc(fee)));
    }
    if own_inputs == 0 {
        warnings.push(format!(
            "none of the inputs belong to this seed (fingerprint {}). Wrong seed or missing BIP39 passphrase?",
            wallet.fingerprint()
        ));
    } else if own_inputs < n_in {
        warnings.push(format!(
            "{} of {} inputs are not yours; other parties must sign them.",
            n_in - own_inputs,
            n_in
        ));
    }

    Ok(Review {
        network,
        fingerprint: wallet.fingerprint(),
        inputs,
        outputs,
        total_in,
        total_out,
        fee,
        sending,
        own_inputs,
        multisig: summary,
        warnings,
    })
}

/// Sign every input that belongs to the wallet. Returns how many were signed.
///
/// [`review`] runs again inside, so a PSBT that fails the safety rules can
/// never be signed even if the caller forgets to review it first.
pub fn sign(psbt: &mut Psbt, wallet: &Wallet) -> Result<usize> {
    sign_with(psbt, wallet, &[])
}

/// [`sign`], knowing these registered multisig wallets. Afterwards every input that the review
/// approved must carry a signature made with this seed's own key for it (in a multisig input,
/// another cosigner's signature may already be there: that does not count).
pub fn sign_with(psbt: &mut Psbt, wallet: &Wallet, multisig: &[(String, MultisigWallet)]) -> Result<usize> {
    let review = review_with(psbt, wallet, multisig)?;
    if review.own_inputs == 0 {
        return Err(Error::Psbt(format!(
            "none of the inputs belong to the wallet with fingerprint {}: wrong seed or missing BIP39 passphrase?",
            wallet.fingerprint()
        )));
    }
    if let Err((_, errors)) = psbt.sign(wallet.master(), wallet.secp()) {
        return Err(Error::Psbt(format!("signing failed: {errors:?}")));
    }
    let mut signed = 0;
    for input in review.inputs.iter().filter(|i| i.path.is_some()) {
        let p = &psbt.inputs[input.index];
        let done = match input.key {
            Some(key) => p.partial_sigs.contains_key(&PublicKey::new(key)),
            None => p.tap_key_sig.is_some(),
        };
        if !done {
            return Err(Error::Psbt(format!("input #{} could not be signed", input.index)));
        }
        signed += 1;
    }
    Ok(signed)
}

// ---------------------------------------------------------------------------

/// The previous output an input spends, cross-checked when both forms exist.
fn input_txout(psbt: &Psbt, i: usize) -> Result<TxOut> {
    let prevout = psbt.unsigned_tx.input[i].previous_output;
    let inp = &psbt.inputs[i];
    if let Some(prev_tx) = &inp.non_witness_utxo {
        if prev_tx.compute_txid() != prevout.txid {
            return Err(Error::Psbt(format!(
                "input #{i}: the attached previous transaction does not match the txid it is supposed to spend"
            )));
        }
        let out = prev_tx
            .output
            .get(prevout.vout as usize)
            .ok_or_else(|| Error::Psbt(format!("input #{i}: output index {} does not exist in the previous transaction", prevout.vout)))?;
        if let Some(w) = &inp.witness_utxo {
            if w != out {
                return Err(Error::Psbt(format!("input #{i}: inconsistent UTXO data inside the PSBT")));
            }
        }
        return Ok(out.clone());
    }
    inp.witness_utxo.clone().ok_or_else(|| {
        Error::Psbt(format!(
            "input #{i} has no previous-output data, so its amount cannot be verified; refusing to sign"
        ))
    })
}

fn check_sighash(psbt: &Psbt, i: usize, txout: &TxOut) -> Result<()> {
    let Some(requested) = psbt.inputs[i].sighash_type else {
        return Ok(());
    };
    let ok = if txout.script_pubkey.is_p2tr() {
        matches!(requested.taproot_hash_ty(), Ok(TapSighashType::Default | TapSighashType::All))
    } else {
        matches!(requested.ecdsa_hash_ty(), Ok(EcdsaSighashType::All))
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Psbt(format!(
            "input #{i} asks for a non-standard signature-hash type (not SIGHASH_ALL); refusing to sign"
        )))
    }
}

fn describe_script(out: &TxOut, network: Network) -> String {
    match Address::from_script(&out.script_pubkey, network) {
        Ok(a) => a.to_string(),
        Err(_) if out.script_pubkey.is_op_return() => "OP_RETURN (data, no address)".to_string(),
        Err(_) => format!("non-standard script {}", out.script_pubkey.to_hex_string()),
    }
}

enum Ownership {
    /// The PSBT does not say this belongs to our seed.
    NotClaimed,
    /// It says so, and we re-derived the same script from our seed.
    Verified(Owned),
    /// It says so, and a registered multisig wallet rebuilds exactly this script from all its
    /// cosigners' keys, this seed's own key among them.
    VerifiedMultisig(OwnedMultisig),
    /// It says so, but we cannot reproduce it. Never trusted.
    Unverified,
}

struct Owned {
    path: DerivationPath,
    /// 0 = receive chain, 1 = change chain.
    chain: u32,
    /// The key that signs into partial_sigs (None for a taproot key-path input).
    key: Option<secp256k1::PublicKey>,
}

struct OwnedMultisig {
    path: DerivationPath,
    chain: u32,
    index: u32,
    /// Which of the registered wallets.
    wallet: usize,
    key: secp256k1::PublicKey,
}

/// `purpose' / coin' / account' / chain / index`
struct PathShape {
    script: ScriptType,
    coin: u32,
    chain: u32,
}

impl PathShape {
    fn parse(path: &DerivationPath) -> Option<Self> {
        let c: &[ChildNumber] = path.as_ref();
        if c.len() != 5 {
            return None;
        }
        let hardened = |n: &ChildNumber| match n {
            ChildNumber::Hardened { index } => Some(*index),
            _ => None,
        };
        let normal = |n: &ChildNumber| match n {
            ChildNumber::Normal { index } => Some(*index),
            _ => None,
        };
        let script = ScriptType::from_purpose(hardened(&c[0])?)?;
        let coin = hardened(&c[1])?;
        hardened(&c[2])?;
        let chain = normal(&c[3])?;
        normal(&c[4])?;
        if chain > 1 {
            return None;
        }
        Some(PathShape { script, coin, chain })
    }
}

/// Decide whether `txout` really belongs to this wallet, by re-deriving the
/// key at the claimed path and rebuilding the script from it.
#[allow(clippy::too_many_arguments)]
fn ownership(
    wallet: &Wallet,
    txout: &TxOut,
    ecdsa: &BTreeMap<secp256k1::PublicKey, KeySource>,
    tap: &BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>,
    witness_script: Option<&ScriptBuf>,
    redeem_script: Option<&ScriptBuf>,
    multisig: &[(String, MultisigWallet)],
    warnings: &mut Vec<String>,
    what: &str,
) -> Result<Ownership> {
    enum Claim {
        Ecdsa(secp256k1::PublicKey),
        Tap(XOnlyPublicKey),
    }
    let fp = wallet.fingerprint();
    let mut claims: Vec<(DerivationPath, Claim)> = Vec::new();
    for (pk, (f, path)) in ecdsa {
        if *f == fp {
            claims.push((path.clone(), Claim::Ecdsa(*pk)));
        }
    }
    for (xk, (_leaves, (f, path))) in tap {
        if *f == fp {
            claims.push((path.clone(), Claim::Tap(*xk)));
        }
    }
    if claims.is_empty() {
        return Ok(Ownership::NotClaimed);
    }

    let coin_warning = |path: &DerivationPath, coin: u32, warnings: &mut Vec<String>| {
        if coin != wallet.coin_type() {
            warnings.push(format!(
                "{what}: the key path {} uses coin type {coin}, which does not match the selected network ({:?}). Check that you chose the right network!",
                path_to_display(path),
                wallet.network()
            ));
        }
    };
    for (path, claim) in &claims {
        let Some(shape) = PathShape::parse(path) else { continue };
        let derived = wallet.public_key_at(path)?;
        let (claim_matches, key) = match claim {
            Claim::Ecdsa(pk) => (*pk == derived, Some(derived)),
            Claim::Tap(xk) => (*xk == XOnlyPublicKey::from(derived), None),
        };
        if !claim_matches {
            continue;
        }
        if wallet.script_pubkey(shape.script, &derived) != txout.script_pubkey {
            continue;
        }
        coin_warning(path, shape.coin, warnings);
        return Ok(Ownership::Verified(Owned { path: path.clone(), chain: shape.chain, key }));
    }
    // A multisig script: only a registered wallet that rebuilds exactly this script counts.
    for (path, claim) in &claims {
        let Claim::Ecdsa(pk) = claim else { continue };
        let derived = wallet.public_key_at(path)?;
        if *pk != derived {
            continue;
        }
        for (w, (_, ms)) in multisig.iter().enumerate() {
            let Some((chain, index)) = ms.locate(fp, path) else { continue };
            if ms.script_pubkey(chain, index)? != txout.script_pubkey {
                continue;
            }
            if !ms.keys_at(chain, index)?.contains(&PublicKey::new(derived)) {
                continue; // this seed is not the cosigner the wallet holds (another passphrase?)
            }
            // the PSBT's own copies of the scripts, when it has them, must be the real ones
            let (redeem, witness) = ms.psbt_scripts(chain, index)?;
            if witness_script.is_some_and(|given| Some(given) != witness.as_ref())
                || redeem_script.is_some_and(|given| Some(given) != redeem.as_ref())
            {
                continue;
            }
            let steps: &[ChildNumber] = path.as_ref();
            if let Some(ChildNumber::Hardened { index: coin }) = steps.get(1) {
                coin_warning(path, *coin, warnings);
            }
            return Ok(Ownership::VerifiedMultisig(OwnedMultisig { path: path.clone(), chain, index, wallet: w, key: derived }));
        }
    }
    Ok(Ownership::Unverified)
}

// ---------------------------------------------------------------------------
// Animated QR codes ("UR")
// ---------------------------------------------------------------------------

/// Payload bytes per animated-QR frame: small enough that every frame is an
/// easy-to-scan QR code, even from a screen.
pub const UR_FRAGMENT_LEN: usize = 80;

const UR_PSBT_TYPE: &str = "crypto-psbt";

/// The frames of an animated QR code that carries `psbt`, in the "UR" format
/// (`ur:crypto-psbt/...`) used by SeedSigner, Sparrow and most hardware wallets.
///
/// A PSBT that fits in one frame gives a single `ur:crypto-psbt/<data>`. Larger
/// ones give every fragment once (`1-N` ... `N-N`) followed by the same number of
/// extra "fountain" frames, so a scanner that missed a frame can still finish.
/// Show the frames in a loop; scanners ignore repeats.
pub fn ur_frames(psbt: &Psbt) -> Result<Vec<String>> {
    ur_frames_with(psbt, UR_FRAGMENT_LEN)
}

/// Like [`ur_frames`] with a custom fragment size.
pub fn ur_frames_with(psbt: &Psbt, max_fragment_len: usize) -> Result<Vec<String>> {
    let ur_err = |e: ur::ur::Error| Error::Psbt(format!("cannot build the QR frames: {e:?}"));
    // The payload of a `crypto-psbt` is the PSBT as a CBOR byte string (the tag is implied by the type).
    let payload = cbor_byte_string(&psbt.serialize());
    let mut encoder = ur::Encoder::new(&payload, max_fragment_len, UR_PSBT_TYPE).map_err(ur_err)?;
    let count = encoder.fragment_count();
    if count == 1 {
        return Ok(vec![ur::ur::encode(&payload, &ur::Type::Custom(UR_PSBT_TYPE))]);
    }
    (0..2 * count).map(|_| encoder.next_part().map_err(ur_err)).collect()
}

/// Rebuild a PSBT from UR frames: one single-part frame, or enough frames of a
/// multi-part QR code. Upper or lower case both work.
pub fn from_ur_frames<S: AsRef<str>>(frames: &[S]) -> Result<Psbt> {
    let ur_err = |e: ur::ur::Error| Error::Psbt(format!("invalid QR data: {e:?}"));
    let first = frames
        .first()
        .ok_or_else(|| Error::Psbt("there is no QR data".into()))?;
    let payload = match ur::ur::decode(first.as_ref()) {
        Ok((ur::ur::Kind::SinglePart, payload)) => payload,
        _ => {
            let mut decoder = ur::Decoder::default();
            for frame in frames {
                decoder.receive(frame.as_ref()).map_err(ur_err)?;
                if decoder.complete() {
                    break;
                }
            }
            match decoder.ur_type() {
                Some(UR_PSBT_TYPE) | Some("psbt") | None => {}
                Some(other) => {
                    return Err(Error::Psbt(format!(
                        "this QR code holds a \"{other}\", not a transaction"
                    )))
                }
            }
            decoder
                .message()
                .map_err(ur_err)?
                .ok_or_else(|| Error::Psbt("the QR data is incomplete: some frames are still missing".into()))?
        }
    };
    psbt_from_ur_payload(&payload)
}

fn psbt_from_ur_payload(payload: &[u8]) -> Result<Psbt> {
    let bytes = cbor_byte_string_content(payload)?;
    Psbt::deserialize(bytes).map_err(|e| Error::Psbt(format!("invalid PSBT inside the QR code: {e}")))
}

/// What happened when one more QR code was scanned.
#[derive(Debug)]
pub enum ScanUpdate {
    /// Not part of a transaction; the text says what it is.
    Ignored(String),
    /// One more piece of an animated QR code: `have` of `total` pieces are known.
    Progress { have: usize, total: usize },
    /// The whole transaction has been received.
    Done(Psbt),
}

/// Collects the QR codes of a transaction shown by another wallet (Sparrow, ...) one at
/// a time, in any order and with repeats, until the whole PSBT is known. It understands
/// animated `ur:crypto-psbt` codes, a single `ur:` code, BBQr codes (a series or one) and a plain
/// base64 / hex PSBT.
#[derive(Default)]
pub struct PsbtScanner {
    decoder: ur::Decoder,
    bbqr: crate::bbqr::BbqrCollector,
}

impl PsbtScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the text of one scanned QR code.
    pub fn add(&mut self, text: &str) -> ScanUpdate {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("ur:") {
            return self.add_ur(text, rest);
        }
        if crate::bbqr::is_bbqr(text) {
            use crate::bbqr::BbqrUpdate;
            return match self.bbqr.add(text) {
                BbqrUpdate::Done { file_type: 'P', data } => match decode(&data) {
                    Ok(psbt) => ScanUpdate::Done(psbt),
                    Err(e) => ScanUpdate::Ignored(e.to_string()),
                },
                BbqrUpdate::Done { file_type: 'T', .. } => {
                    ScanUpdate::Ignored("that is a finished transaction, not one to sign (a PSBT)".into())
                }
                BbqrUpdate::Done { file_type, .. } => ScanUpdate::Ignored(format!("this BBQr code holds a file of type {file_type}, not a transaction")),
                BbqrUpdate::Progress { have, total } => ScanUpdate::Progress { have, total },
                BbqrUpdate::Ignored(why) => ScanUpdate::Ignored(format!("this BBQr code cannot be read: {why}")),
            };
        }
        match decode(text.as_bytes()) {
            Ok(psbt) => ScanUpdate::Done(psbt),
            Err(_) => ScanUpdate::Ignored("this QR code is not a transaction".into()),
        }
    }

    fn add_ur(&mut self, text: &str, rest: &str) -> ScanUpdate {
        let ur_type = rest.split('/').next().unwrap_or("");
        if ur_type != UR_PSBT_TYPE && ur_type != "psbt" {
            return ScanUpdate::Ignored(format!("this QR code holds a \"{ur_type}\", not a transaction"));
        }
        let finished = |payload: Vec<u8>| match psbt_from_ur_payload(&payload) {
            Ok(psbt) => ScanUpdate::Done(psbt),
            Err(e) => ScanUpdate::Ignored(e.to_string()),
        };
        match ur::ur::decode(text) {
            Ok((ur::ur::Kind::SinglePart, payload)) => return finished(payload),
            Ok(_) => {}
            Err(_) => return ScanUpdate::Ignored("damaged QR code".into()),
        }
        if self.decoder.receive(text).is_err() {
            // Another transaction started, or this frame is damaged: start again with it.
            self.decoder = ur::Decoder::default();
            if self.decoder.receive(text).is_err() {
                return ScanUpdate::Ignored("damaged QR code".into());
            }
        }
        if self.decoder.complete() {
            let message = self.decoder.message();
            self.decoder = ur::Decoder::default();
            return match message {
                Ok(Some(payload)) => finished(payload),
                _ => ScanUpdate::Ignored("the QR data is incomplete".into()),
            };
        }
        ScanUpdate::Progress {
            have: self.decoder.resolved_fragment_count().unwrap_or(0),
            total: self.decoder.fragment_count(),
        }
    }
}

/// CBOR major type 2 (byte string) header + data.
fn cbor_byte_string(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut out = Vec::with_capacity(n + 5);
    match n {
        0..=23 => out.push(0x40 | n as u8),
        24..=255 => out.extend_from_slice(&[0x58, n as u8]),
        256..=65_535 => {
            out.push(0x59);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        _ => {
            out.push(0x5a);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
    }
    out.extend_from_slice(data);
    out
}

/// Inverse of [`cbor_byte_string`], strict about the length.
fn cbor_byte_string_content(payload: &[u8]) -> Result<&[u8]> {
    let bad = || Error::Psbt("the QR code does not contain a PSBT (unexpected data layout)".to_string());
    let (&head, rest) = payload.split_first().ok_or_else(bad)?;
    if head >> 5 != 2 {
        return Err(bad());
    }
    let (len, rest) = match head & 0x1f {
        n @ 0..=23 => (usize::from(n), rest),
        24 => (usize::from(*rest.first().ok_or_else(bad)?), &rest[1..]),
        25 => {
            let b = rest.get(..2).ok_or_else(bad)?;
            (usize::from(u16::from_be_bytes([b[0], b[1]])), &rest[2..])
        }
        26 => {
            let b = rest.get(..4).ok_or_else(bad)?;
            (u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize, &rest[4..])
        }
        _ => return Err(bad()),
    };
    if rest.len() != len {
        return Err(bad());
    }
    Ok(rest)
}

/// What the key paths inside a transaction say about the network it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkHint {
    /// Every key path uses coin type 0' (and every wallet key it carries is an xpub): mainnet.
    Mainnet,
    /// Every key path uses coin type 1': a test network. Testnet, signet and regtest use exactly the
    /// same transactions and key paths, so nothing in a transaction can say which of them it is.
    TestNetwork,
    /// There are no key paths with a known coin type (legacy multisig paths m/45'/... name none).
    Unknown,
    /// Some key paths say mainnet and others a test network: something is wrong with it.
    Conflicting,
}

/// Look at every key path in the transaction (its inputs, its outputs and the wallet keys it may
/// carry, m/84'/0'/... being mainnet and m/84'/1'/... a test network) and at the version of those
/// wallet keys (xpub or tpub). The command-line program and the phone app both find the network this way.
pub fn network_hint(psbt: &Psbt) -> NetworkHint {
    let coin = |path: &DerivationPath| -> Option<bool> {
        let steps: &[ChildNumber] = path.as_ref();
        match steps.get(1) {
            Some(ChildNumber::Hardened { index: 0 }) => Some(true),
            Some(ChildNumber::Hardened { index: 1 }) => Some(false),
            _ => None,
        }
    };
    let input_paths = psbt
        .inputs
        .iter()
        .flat_map(|i| i.bip32_derivation.values().map(|(_, p)| p).chain(i.tap_key_origins.values().map(|(_, (_, p))| p)));
    let output_paths = psbt
        .outputs
        .iter()
        .flat_map(|o| o.bip32_derivation.values().map(|(_, p)| p).chain(o.tap_key_origins.values().map(|(_, (_, p))| p)));
    let mut mainnet: Vec<bool> = input_paths.chain(output_paths).chain(psbt.xpub.values().map(|(_, p)| p)).filter_map(coin).collect();
    mainnet.extend(psbt.xpub.keys().map(|xpub| xpub.network == bitcoin::NetworkKind::Main));
    match (mainnet.iter().any(|&m| m), mainnet.iter().any(|&m| !m)) {
        (true, false) => NetworkHint::Mainnet,
        (false, true) => NetworkHint::TestNetwork,
        (false, false) => NetworkHint::Unknown,
        (true, true) => NetworkHint::Conflicting,
    }
}

#[cfg(test)]
mod network_hint_tests {
    use super::*;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use bitcoin::{absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};

    /// A transaction with one key path in its input (and optionally one in a change output).
    fn psbt_with_paths(input_path: Option<&str>, change_path: Option<&str>) -> Psbt {
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: vec![TxOut { value: Amount::from_sat(1000), script_pubkey: ScriptBuf::new() }],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        let key = SecretKey::from_slice(&[1u8; 32]).unwrap().public_key(&Secp256k1::new());
        let origin = |p: &str| (Fingerprint::from([1, 2, 3, 4]), p.parse::<DerivationPath>().unwrap());
        if let Some(p) = input_path {
            psbt.inputs[0].bip32_derivation.insert(key, origin(p));
        }
        if let Some(p) = change_path {
            psbt.outputs[0].bip32_derivation.insert(key, origin(p));
        }
        psbt
    }

    #[test]
    fn the_network_is_found_from_the_key_paths() {
        assert_eq!(network_hint(&psbt_with_paths(Some("m/84'/0'/0'/0/3"), None)), NetworkHint::Mainnet);
        assert_eq!(network_hint(&psbt_with_paths(Some("m/86'/1'/0'/0/3"), Some("m/86'/1'/0'/1/0"))), NetworkHint::TestNetwork);
        assert_eq!(network_hint(&psbt_with_paths(None, None)), NetworkHint::Unknown);
        assert_eq!(network_hint(&psbt_with_paths(Some("m/0/1"), None)), NetworkHint::Unknown, "a path without a coin type says nothing");
        // a change output that says otherwise than the input: something is wrong
        assert_eq!(network_hint(&psbt_with_paths(Some("m/84'/0'/0'/0/3"), Some("m/84'/1'/0'/1/0"))), NetworkHint::Conflicting);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::key::TapTweak;
    use bitcoin::secp256k1::{Message, Secp256k1};
    use bitcoin::sighash::{Prevouts, SighashCache};
    use bitcoin::{absolute, transaction, ScriptBuf, Sequence, Transaction, TxIn, Txid, Witness};
    use std::str::FromStr;

    const ABANDON: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const OTHER: &str = "legal winner thank year wave sausage worth useful legal winner thank yellow";

    fn wallet_from(phrase: &str) -> Wallet {
        Wallet::new(&crate::mnemonic::parse(phrase).unwrap(), "", Network::Bitcoin).unwrap()
    }

    fn external_script() -> ScriptBuf {
        Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")
            .unwrap()
            .assume_checked()
            .script_pubkey()
    }

    /// A PSBT spending 100_000 sat from `m/purpose'/0'/0'/0/0`, sending 60_000 to
    /// an outside address and 39_000 back to `m/purpose'/0'/0'/1/0` (fee 1_000).
    fn build(w: &Wallet, script: ScriptType) -> Psbt {
        let purpose = script.purpose();
        let in_path = DerivationPath::from_str(&format!("m/{purpose}'/0'/0'/0/0")).unwrap();
        let ch_path = DerivationPath::from_str(&format!("m/{purpose}'/0'/0'/1/0")).unwrap();
        let in_pk = w.public_key_at(&in_path).unwrap();
        let ch_pk = w.public_key_at(&ch_path).unwrap();
        let funding = TxOut { value: Amount::from_sat(100_000), script_pubkey: w.script_pubkey(script, &in_pk) };
        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint { txid: Txid::from_byte_array([0xaa; 32]), vout: 1 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut { value: Amount::from_sat(60_000), script_pubkey: external_script() },
                TxOut { value: Amount::from_sat(39_000), script_pubkey: w.script_pubkey(script, &ch_pk) },
            ],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(funding);
        if script == ScriptType::Tr {
            let (xi, xc) = (XOnlyPublicKey::from(in_pk), XOnlyPublicKey::from(ch_pk));
            psbt.inputs[0].tap_internal_key = Some(xi);
            psbt.inputs[0].tap_key_origins.insert(xi, (vec![], (w.fingerprint(), in_path)));
            psbt.outputs[1].tap_internal_key = Some(xc);
            psbt.outputs[1].tap_key_origins.insert(xc, (vec![], (w.fingerprint(), ch_path)));
        } else {
            psbt.inputs[0].bip32_derivation.insert(in_pk, (w.fingerprint(), in_path));
            psbt.outputs[1].bip32_derivation.insert(ch_pk, (w.fingerprint(), ch_path));
        }
        psbt
    }

    #[test]
    fn review_labels_change_and_computes_fee() {
        let w = wallet_from(ABANDON);
        let r = review(&build(&w, ScriptType::Wpkh), &w).unwrap();
        assert_eq!(r.total_in.to_sat(), 100_000);
        assert_eq!(r.fee.to_sat(), 1_000);
        assert_eq!(r.sending.to_sat(), 60_000);
        assert_eq!(r.own_inputs, 1);
        assert_eq!(r.outputs[0].kind, OutputKind::External);
        assert_eq!(r.outputs[0].address, "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4");
        assert_eq!(r.outputs[1].kind, OutputKind::Change { path: "m/84'/0'/0'/1/0".into() });
        assert_eq!(r.inputs[0].path.as_deref(), Some("m/84'/0'/0'/0/0"));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(format_btc(r.fee), "0.00001000");
    }

    #[test]
    fn signs_native_segwit_and_signature_verifies() {
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Wpkh);
        assert_eq!(sign(&mut psbt, &w).unwrap(), 1);

        let secp = Secp256k1::new();
        let funding = psbt.inputs[0].witness_utxo.clone().unwrap();
        let mut cache = SighashCache::new(&psbt.unsigned_tx);
        let sighash = cache
            .p2wpkh_signature_hash(0, &funding.script_pubkey, funding.value, EcdsaSighashType::All)
            .unwrap();
        let msg = Message::from_digest(sighash.to_byte_array());
        let (pk, sig) = psbt.inputs[0].partial_sigs.iter().next().unwrap();
        assert_eq!(sig.sighash_type, EcdsaSighashType::All);
        secp.verify_ecdsa(&msg, &sig.signature, &pk.inner).expect("signature must verify");
    }

    #[test]
    fn signs_taproot_and_signature_verifies() {
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Tr);
        let review_before = review(&psbt, &w).unwrap();
        assert_eq!(review_before.outputs[1].kind, OutputKind::Change { path: "m/86'/0'/0'/1/0".into() });
        assert_eq!(sign(&mut psbt, &w).unwrap(), 1);

        let secp = Secp256k1::new();
        let funding = psbt.inputs[0].witness_utxo.clone().unwrap();
        let mut cache = SighashCache::new(&psbt.unsigned_tx);
        let sighash = cache
            .taproot_key_spend_signature_hash(0, &Prevouts::All(&[funding.clone()]), TapSighashType::Default)
            .unwrap();
        let msg = Message::from_digest(sighash.to_byte_array());
        let sig = psbt.inputs[0].tap_key_sig.unwrap();
        let internal = psbt.inputs[0].tap_internal_key.unwrap();
        let (tweaked, _) = internal.tap_tweak(&secp, None);
        secp.verify_schnorr(&sig.signature, &msg, &tweaked.to_x_only_public_key()).expect("schnorr signature must verify");
    }

    #[test]
    fn signs_wrapped_segwit_and_legacy() {
        let w = wallet_from(ABANDON);
        for st in [ScriptType::ShWpkh, ScriptType::Pkh] {
            // wrapped segwit needs the redeem script; legacy needs the full previous tx.
            let mut psbt = build(&w, st);
            let path = DerivationPath::from_str(&format!("m/{}'/0'/0'/0/0", st.purpose())).unwrap();
            let pk = w.public_key_at(&path).unwrap();
            if st == ScriptType::ShWpkh {
                let wpkh = bitcoin::CompressedPublicKey(pk).wpubkey_hash();
                psbt.inputs[0].redeem_script = Some(ScriptBuf::new_p2wpkh(&wpkh));
            } else {
                // legacy: attach a full previous transaction whose txid we then reference
                let prev = Transaction {
                    version: transaction::Version::ONE,
                    lock_time: absolute::LockTime::ZERO,
                    input: vec![],
                    output: vec![TxOut { value: Amount::from_sat(100_000), script_pubkey: w.script_pubkey(st, &pk) }],
                };
                psbt.unsigned_tx.input[0].previous_output = OutPoint { txid: prev.compute_txid(), vout: 0 };
                psbt.inputs[0].non_witness_utxo = Some(prev);
                psbt.inputs[0].witness_utxo = None;
            }
            assert_eq!(sign(&mut psbt, &w).unwrap(), 1, "{st:?}");
            assert!(!psbt.inputs[0].partial_sigs.is_empty());
        }
    }

    #[test]
    fn a_swapped_change_address_is_not_trusted() {
        // Malware replaces the change script with the attacker's address but keeps
        // the "this is yours" derivation info. It must show up as an outside payment.
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Wpkh);
        psbt.unsigned_tx.output[1].script_pubkey = external_script();
        let r = review(&psbt, &w).unwrap();
        assert_eq!(r.outputs[1].kind, OutputKind::External);
        assert_eq!(r.sending.to_sat(), 99_000);
        assert!(r.warnings.iter().any(|m| m.contains("output #1 claims to be yours")), "{:?}", r.warnings);
    }

    #[test]
    fn wrong_seed_cannot_sign() {
        let w = wallet_from(ABANDON);
        let other = wallet_from(OTHER);
        let mut psbt = build(&w, ScriptType::Wpkh);
        let r = review(&psbt, &other).unwrap();
        assert_eq!(r.own_inputs, 0);
        assert!(sign(&mut psbt, &other).is_err());
        assert!(psbt.inputs[0].partial_sigs.is_empty());
    }

    #[test]
    fn refuses_missing_amounts_and_odd_sighash() {
        let w = wallet_from(ABANDON);
        let mut no_utxo = build(&w, ScriptType::Wpkh);
        no_utxo.inputs[0].witness_utxo = None;
        assert!(review(&no_utxo, &w).is_err());

        let mut odd = build(&w, ScriptType::Wpkh);
        odd.inputs[0].sighash_type = Some(EcdsaSighashType::None.into());
        assert!(review(&odd, &w).is_err());
        assert!(sign(&mut odd, &w).is_err());
    }

    #[test]
    fn refuses_unrecognised_inputs_that_claim_to_be_ours() {
        // Right key and path, but the previous output is some unrelated script.
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Wpkh);
        psbt.inputs[0].witness_utxo.as_mut().unwrap().script_pubkey = external_script();
        assert!(review(&psbt, &w).is_err());
    }

    #[test]
    fn network_mismatch_is_flagged() {
        let mainnet = wallet_from(ABANDON);
        let psbt = build(&mainnet, ScriptType::Wpkh);
        let testnet = Wallet::new(&crate::mnemonic::parse(ABANDON).unwrap(), "", Network::Testnet).unwrap();
        let r = review(&psbt, &testnet).unwrap();
        assert!(r.warnings.iter().any(|m| m.contains("coin type")), "{:?}", r.warnings);
    }

    #[test]
    fn animated_qr_frames_roundtrip() {
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Wpkh);
        sign(&mut psbt, &w).unwrap();

        let frames = ur_frames(&psbt).unwrap();
        assert!(frames.len() >= 4, "a signed PSBT needs several frames");
        assert!(frames.iter().all(|f| f.starts_with("ur:crypto-psbt/")));
        assert!(frames[0].starts_with(&format!("ur:crypto-psbt/1-{}/", frames.len() / 2)));
        assert_eq!(from_ur_frames(&frames).unwrap(), psbt);

        // a scanner sees the frames in a loop, starting anywhere, in the upper case used inside QR codes
        let looped: Vec<String> = frames
            .iter()
            .cycle()
            .skip(3)
            .take(frames.len() + 2)
            .map(|f| f.to_uppercase())
            .collect();
        assert_eq!(from_ur_frames(&looped).unwrap(), psbt);
        // one frame out of many is not enough
        assert!(from_ur_frames(&looped[..1]).is_err());
    }

    #[test]
    fn single_frame_and_large_transactions() {
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Wpkh);
        sign(&mut psbt, &w).unwrap();

        let one = ur_frames_with(&psbt, 10_000).unwrap();
        assert_eq!(one.len(), 1);
        assert!(one[0].starts_with("ur:crypto-psbt/") && !one[0].contains("/1-"), "{}", one[0]);
        assert_eq!(from_ur_frames(&one).unwrap(), psbt);

        let mut big = psbt.clone();
        for _ in 0..40 {
            big.unsigned_tx.output.push(big.unsigned_tx.output[0].clone());
            big.outputs.push(Default::default());
        }
        let frames = ur_frames(&big).unwrap();
        assert!(frames.len() > 40, "{} frames", frames.len());
        assert_eq!(from_ur_frames(&frames).unwrap(), big);
    }

    #[test]
    fn ur_input_that_is_not_a_psbt_is_rejected() {
        let none: [&str; 0] = [];
        assert!(from_ur_frames(&none).is_err());
        assert!(from_ur_frames(&["hello"]).is_err());
        let not_a_psbt = ur::ur::encode(b"\x43abc", &ur::Type::Bytes); // CBOR byte string "abc"
        assert!(from_ur_frames(&[not_a_psbt]).is_err());
    }

    #[test]
    fn scanner_collects_frames_in_any_order_with_repeats() {
        let w = wallet_from(ABANDON);
        let mut psbt = build(&w, ScriptType::Wpkh);
        sign(&mut psbt, &w).unwrap();
        let frames = ur_frames(&psbt).unwrap();
        let mut scanner = PsbtScanner::new();
        let mut done = None;
        let mut seen_progress = false;
        // A camera sees the loop from anywhere, repeats frames, and reads upper case.
        for f in frames.iter().cycle().skip(4).take(frames.len() * 3).flat_map(|f| [f.to_uppercase(), f.to_uppercase()]) {
            match scanner.add(&f) {
                ScanUpdate::Progress { have, total } => {
                    assert!(have <= total && total >= 4);
                    seen_progress = true;
                }
                ScanUpdate::Done(p) => {
                    done = Some(p);
                    break;
                }
                ScanUpdate::Ignored(why) => panic!("{why}"),
            }
        }
        assert!(seen_progress);
        assert_eq!(done.unwrap(), psbt);
    }

    #[test]
    fn scanner_starts_over_when_another_transaction_appears() {
        let w = wallet_from(ABANDON);
        let mut first = build(&w, ScriptType::Wpkh);
        sign(&mut first, &w).unwrap();
        let mut second = first.clone();
        second.unsigned_tx.output[0].value = bitcoin::Amount::from_sat(1_234);
        let (a, b) = (ur_frames(&first).unwrap(), ur_frames(&second).unwrap());
        let mut scanner = PsbtScanner::new();
        for f in &a[..2] {
            assert!(matches!(scanner.add(f), ScanUpdate::Progress { .. }));
        }
        // the camera now sees a different animation: the first pieces are dropped, not mixed up
        let done = b.iter().chain(b.iter()).find_map(|f| match scanner.add(f) {
            ScanUpdate::Done(p) => Some(p),
            _ => None,
        });
        assert_eq!(done.unwrap(), second);
    }

    #[test]
    fn scanner_reads_a_plain_text_psbt_and_ignores_other_codes() {
        let w = wallet_from(ABANDON);
        let psbt = build(&w, ScriptType::Wpkh);
        let mut scanner = PsbtScanner::new();
        match scanner.add(&encode_base64(&psbt)) {
            ScanUpdate::Done(p) => assert_eq!(p, psbt),
            other => panic!("{other:?}"),
        }
        let single = ur_frames_with(&psbt, 10_000).unwrap();
        assert!(matches!(scanner.add(&single[0]), ScanUpdate::Done(_)));
        let account = ur::ur::encode(b"abc", &ur::Type::Custom("crypto-account"));
        for junk in ["", "hello", "bitcoin:bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", "ur:crypto-psbt/garbage", "B$2P0100", account.as_str()] {
            assert!(matches!(scanner.add(junk), ScanUpdate::Ignored(_)), "{junk:?}");
        }
    }

    #[test]
    fn scanner_reads_a_bbqr_psbt_in_any_order() {
        let w = wallet_from(ABANDON);
        let psbt = build(&w, ScriptType::Wpkh);
        let parts = crate::bbqr::test_parts('P', &psbt.serialize(), 3);
        assert_eq!(parts.len(), 3);
        let mut scanner = PsbtScanner::new();
        assert!(matches!(scanner.add(&parts[2]), ScanUpdate::Progress { have: 1, total: 3 }));
        assert!(matches!(scanner.add(&parts[0]), ScanUpdate::Progress { have: 2, total: 3 }));
        match scanner.add(&parts[1]) {
            ScanUpdate::Done(p) => assert_eq!(p, psbt),
            other => panic!("{other:?}"),
        }
        // a finished transaction (type T) is not something to sign
        let finished = crate::bbqr::test_parts('T', &psbt.unsigned_tx.compute_txid().to_byte_array(), 1);
        assert!(matches!(PsbtScanner::new().add(&finished[0]), ScanUpdate::Ignored(why) if why.contains("finished transaction")));
    }

    #[test]
    fn decode_accepts_binary_base64_and_hex() {
        let w = wallet_from(ABANDON);
        let psbt = build(&w, ScriptType::Wpkh);
        let bin = psbt.serialize();
        assert_eq!(decode(&bin).unwrap(), psbt);
        assert_eq!(decode(encode_base64(&psbt).as_bytes()).unwrap(), psbt);
        assert_eq!(decode(data_encoding::HEXLOWER.encode(&bin).as_bytes()).unwrap(), psbt);
        assert!(decode(b"definitely not a psbt").is_err());
    }

    // ------------------------------------------------------------------ multisig (2 of 3, P2WSH)
    const COSIGNERS: [&str; 3] = [ABANDON, OTHER, "letter advice cage absurd amount doctor acoustic avoid letter advice cage above"];

    fn cosigner(i: usize) -> Wallet {
        Wallet::new(&crate::mnemonic::parse(COSIGNERS[i]).unwrap(), "", Network::Testnet).unwrap()
    }

    /// The three seeds as a registered 2-of-3 wallet named "vault", as Sparrow describes it.
    fn vault() -> Vec<(String, MultisigWallet)> {
        let account: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let keys: Vec<String> = (0..3)
            .map(|i| format!("[{}/48h/1h/0h/2h]{}/<0;1>/*", cosigner(i).fingerprint(), cosigner(i).xpub_at(&account).unwrap()))
            .collect();
        vec![("vault".to_string(), MultisigWallet::from_descriptor_text(&format!("wsh(sortedmulti(2,{}))", keys.join(","))).unwrap())]
    }

    /// Spend 100_000 sat from the vault's receive address 0: 60_000 to someone else and 39_000 to
    /// the vault's change address 0, or to `change` instead. As Sparrow does, the input and the
    /// change carry every cosigner's key path and the witness script.
    fn multisig_psbt(ms: &MultisigWallet, change: Option<(ScriptBuf, ScriptBuf)>) -> Psbt {
        let key_paths = |chain: u32, index: u32| -> BTreeMap<secp256k1::PublicKey, KeySource> {
            (0..3)
                .map(|i| {
                    let path: DerivationPath = format!("m/48'/1'/0'/2'/{chain}/{index}").parse().unwrap();
                    (cosigner(i).public_key_at(&path).unwrap(), (cosigner(i).fingerprint(), path))
                })
                .collect()
        };
        let (change_witness, change_spk) = change.unwrap_or_else(|| (ms.witness_script(1, 0).unwrap(), ms.script_pubkey(1, 0).unwrap()));
        let outside = Address::from_str("tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx").unwrap().assume_checked();
        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint { txid: Txid::from_byte_array([0xbb; 32]), vout: 0 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut { value: Amount::from_sat(60_000), script_pubkey: outside.script_pubkey() },
                TxOut { value: Amount::from_sat(39_000), script_pubkey: change_spk },
            ],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(TxOut { value: Amount::from_sat(100_000), script_pubkey: ms.script_pubkey(0, 0).unwrap() });
        psbt.inputs[0].witness_script = Some(ms.witness_script(0, 0).unwrap());
        psbt.inputs[0].bip32_derivation = key_paths(0, 0);
        psbt.outputs[1].witness_script = Some(change_witness);
        psbt.outputs[1].bip32_derivation = key_paths(1, 0);
        psbt
    }

    #[test]
    fn signs_its_part_of_a_registered_2_of_3_multisig() {
        let registered = vault();
        let ms = &registered[0].1;
        let mut psbt = multisig_psbt(ms, None);
        let a = cosigner(0);
        let r = review_with(&psbt, &a, &registered).unwrap();
        assert_eq!(r.own_inputs, 1);
        assert_eq!(r.inputs[0].multisig.as_deref(), Some("vault"));
        assert_eq!(r.outputs[0].kind, OutputKind::External);
        assert!(
            matches!(&r.outputs[1].kind, OutputKind::MultisigChange { wallet, path } if wallet == "vault" && path == "m/48'/1'/0'/2'/1/0"),
            "{:?}",
            r.outputs[1].kind
        );
        assert_eq!(r.sending, Amount::from_sat(60_000));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let summary = r.multisig.clone().unwrap();
        let position = ms.position(a.fingerprint()).unwrap() + 1;
        assert_eq!((summary.threshold, summary.cosigners, summary.position, summary.signed_by_others), (2, 3, position, 0));

        // A signs exactly its own key, over the P2WSH signature hash (the witness script is the script code)
        assert_eq!(sign_with(&mut psbt, &a, &registered).unwrap(), 1);
        let a_key = PublicKey::new(a.public_key_at(&"m/48'/1'/0'/2'/0/0".parse().unwrap()).unwrap());
        assert_eq!(psbt.inputs[0].partial_sigs.len(), 1);
        let sighash = SighashCache::new(&psbt.unsigned_tx)
            .p2wsh_signature_hash(0, &ms.witness_script(0, 0).unwrap(), Amount::from_sat(100_000), EcdsaSighashType::All)
            .unwrap();
        let signature = psbt.inputs[0].partial_sigs[&a_key];
        Secp256k1::verification_only()
            .verify_ecdsa(&Message::from_digest(sighash.to_byte_array()), &signature.signature, &a_key.inner)
            .unwrap();

        // then B: its review sees A's signature, and afterwards the input has the two it needs
        let b = cosigner(1);
        assert_eq!(review_with(&psbt, &b, &registered).unwrap().multisig.unwrap().signed_by_others, 1);
        assert_eq!(sign_with(&mut psbt, &b, &registered).unwrap(), 1);
        assert_eq!(psbt.inputs[0].partial_sigs.len(), 2);
    }

    #[test]
    fn multisig_is_refused_unless_registered_and_rebuilt_exactly() {
        let registered = vault();
        let ms = &registered[0].1;
        let a = cosigner(0);
        let e = review(&multisig_psbt(ms, None), &a).unwrap_err().to_string();
        assert!(e.contains("not registered") && e.contains("menu 10"), "{e}");
        // the PSBT's own copy of the witness script is not the real one: refused
        let mut tampered = multisig_psbt(ms, None);
        tampered.inputs[0].witness_script = Some(ms.witness_script(0, 1).unwrap());
        assert!(review_with(&tampered, &a, &registered).is_err());
        // a seed that is not one of the cosigners signs nothing
        let stranger = Wallet::new(&crate::mnemonic::parse("zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong").unwrap(), "", Network::Testnet).unwrap();
        assert_eq!(review_with(&multisig_psbt(ms, None), &stranger, &registered).unwrap().own_inputs, 0);
        assert!(sign_with(&mut multisig_psbt(ms, None), &stranger, &registered).is_err());
    }

    #[test]
    fn swapped_multisig_change_is_a_payment_to_someone_else() {
        // "change" that holds A's key at the change path, but with two keys of an attacker
        let registered = vault();
        let ms = &registered[0].1;
        let a = cosigner(0);
        let secp = Secp256k1::new();
        let mut keys: Vec<PublicKey> = [[7u8; 32], [8u8; 32]]
            .iter()
            .map(|k| PublicKey::new(secp256k1::SecretKey::from_slice(k).unwrap().public_key(&secp)))
            .collect();
        keys.push(PublicKey::new(a.public_key_at(&"m/48'/1'/0'/2'/1/0".parse().unwrap()).unwrap()));
        keys.sort_by(|x, y| x.inner.serialize().cmp(&y.inner.serialize()));
        let mut builder = bitcoin::script::Builder::new().push_int(2);
        for key in &keys {
            builder = builder.push_key(key);
        }
        let fake = builder.push_int(3).push_opcode(bitcoin::opcodes::all::OP_CHECKMULTISIG).into_script();
        let fake_spk = ScriptBuf::new_p2wsh(&fake.wscript_hash());
        let r = review_with(&multisig_psbt(ms, Some((fake, fake_spk))), &a, &registered).unwrap();
        assert_eq!(r.outputs[1].kind, OutputKind::External);
        assert_eq!(r.sending, Amount::from_sat(99_000), "both outputs leave the wallet");
        assert!(r.warnings.iter().any(|w| w.contains("output #1")), "{:?}", r.warnings);
    }

    /// A descriptor of the three cosigners with account path `account` (like "48h/1h/0h/1h"),
    /// wrapped in `wrap` ("wsh(", "sh(wsh(" or "sh(") around `multi` ("sortedmulti" or "multi").
    fn typed_wallet(wrap: &str, multi: &str, account: &str) -> Vec<(String, MultisigWallet)> {
        let path: DerivationPath = format!("m/{}", account.replace('h', "'")).parse().unwrap();
        let keys: Vec<String> =
            (0..3).map(|i| format!("[{}/{account}]{}/<0;1>/*", cosigner(i).fingerprint(), cosigner(i).xpub_at(&path).unwrap())).collect();
        let closing = ")".repeat(wrap.matches('(').count()); // "(2,...)" closes multi() itself
        let text = format!("{wrap}{multi}(2,{}){closing}", keys.join(","));
        vec![("safe".to_string(), MultisigWallet::from_descriptor_text(&text).unwrap())]
    }

    /// Spend 100_000 sat from `ms`'s receive address 0 (60_000 out, 39_000 to its change address 0),
    /// with what its kind of script needs: the scripts and, for legacy, the whole previous transaction.
    fn typed_psbt(ms: &MultisigWallet, account: &str) -> Psbt {
        let prev = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: vec![TxOut { value: Amount::from_sat(100_000), script_pubkey: ms.script_pubkey(0, 0).unwrap() }],
        };
        let outside = Address::from_str("tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx").unwrap().assume_checked();
        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint { txid: prev.compute_txid(), vout: 0 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut { value: Amount::from_sat(60_000), script_pubkey: outside.script_pubkey() },
                TxOut { value: Amount::from_sat(39_000), script_pubkey: ms.script_pubkey(1, 0).unwrap() },
            ],
        };
        let key_paths = |chain: u32| -> BTreeMap<secp256k1::PublicKey, KeySource> {
            (0..3)
                .map(|i| {
                    let path: DerivationPath = format!("m/{}/{chain}/0", account.replace('h', "'")).parse().unwrap();
                    (cosigner(i).public_key_at(&path).unwrap(), (cosigner(i).fingerprint(), path))
                })
                .collect()
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        if ms.script == MultisigScript::Sh {
            psbt.inputs[0].non_witness_utxo = Some(prev.clone());
        } else {
            psbt.inputs[0].witness_utxo = Some(prev.output[0].clone());
        }
        (psbt.inputs[0].redeem_script, psbt.inputs[0].witness_script) = ms.psbt_scripts(0, 0).unwrap();
        psbt.inputs[0].bip32_derivation = key_paths(0);
        (psbt.outputs[1].redeem_script, psbt.outputs[1].witness_script) = ms.psbt_scripts(1, 0).unwrap();
        psbt.outputs[1].bip32_derivation = key_paths(1);
        psbt
    }

    #[test]
    fn signs_nested_and_legacy_multisig_over_the_right_signature_hash() {
        for (wrap, multi, account) in [("sh(wsh(", "sortedmulti", "48h/1h/0h/1h"), ("sh(", "multi", "45h")] {
            let registered = typed_wallet(wrap, multi, account);
            let ms = &registered[0].1;
            let mut psbt = typed_psbt(ms, account);
            let a = cosigner(0);
            let r = review_with(&psbt, &a, &registered).unwrap();
            assert_eq!(r.own_inputs, 1, "{wrap}");
            assert!(matches!(&r.outputs[1].kind, OutputKind::MultisigChange { .. }), "{wrap}: {:?}", r.outputs[1].kind);
            assert!(r.warnings.is_empty(), "{wrap}: {:?}", r.warnings);
            assert_eq!(sign_with(&mut psbt, &a, &registered).unwrap(), 1);
            let path: DerivationPath = format!("m/{}/0/0", account.replace('h', "'")).parse().unwrap();
            let key = PublicKey::new(a.public_key_at(&path).unwrap());
            let script = ms.witness_script(0, 0).unwrap();
            let mut cache = SighashCache::new(&psbt.unsigned_tx);
            let digest: [u8; 32] = if ms.script == MultisigScript::Sh {
                cache.legacy_signature_hash(0, &script, EcdsaSighashType::All.to_u32()).unwrap().to_byte_array()
            } else {
                cache.p2wsh_signature_hash(0, &script, Amount::from_sat(100_000), EcdsaSighashType::All).unwrap().to_byte_array()
            };
            let signature = psbt.inputs[0].partial_sigs[&key];
            Secp256k1::verification_only().verify_ecdsa(&Message::from_digest(digest), &signature.signature, &key.inner).unwrap();
        }
        // a legacy input without its whole previous transaction: its amount cannot be checked, refused
        let registered = typed_wallet("sh(", "multi", "45h");
        let mut psbt = typed_psbt(&registered[0].1, "45h");
        psbt.inputs[0].witness_utxo = Some(psbt.inputs[0].non_witness_utxo.take().unwrap().output[0].clone());
        let e = review_with(&psbt, &cosigner(0), &registered).unwrap_err().to_string();
        assert!(e.contains("previous transaction"), "{e}");
        // a nested input whose PSBT lacks the redeem script cannot be signed: said plainly
        let registered = typed_wallet("sh(wsh(", "sortedmulti", "48h/1h/0h/1h");
        let mut psbt = typed_psbt(&registered[0].1, "48h/1h/0h/1h");
        psbt.inputs[0].redeem_script = None;
        let e = review_with(&psbt, &cosigner(0), &registered).unwrap_err().to_string();
        assert!(e.contains("without its script"), "{e}");
    }
}
