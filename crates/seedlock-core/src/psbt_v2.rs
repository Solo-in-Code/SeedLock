//! PSBTs as raw key-value maps, for version 0 and version 2 (BIP 370), the version that Sparrow uses for silent
//! payments (BIP 375 and 376).
//!
//! The `bitcoin` crate reads and signs version 0 only. A version 2 PSBT is therefore kept here as it came, every
//! field of it, and turned into its version-0 form ([`to_v0`]): the same inputs and outputs, with the transaction
//! put together from version 2's per-input and per-output fields. That form is reviewed and signed as any other,
//! and the signatures then go back into the version 2 PSBT ([`add_new_entries`]), which leaves as version 2.

use crate::{Error, Result};
use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::psbt::Psbt;
use bitcoin::transaction::Version;
use bitcoin::{absolute, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};
use bitcoin::hashes::Hash;

/// The magic bytes every PSBT starts with.
const MAGIC: &[u8] = b"psbt\xff";

// Version 2's own fields (BIP 370), which a version-0 PSBT does not have.
pub(crate) const GLOBAL_UNSIGNED_TX: u8 = 0x00;
pub(crate) const GLOBAL_TX_VERSION: u8 = 0x02;
pub(crate) const GLOBAL_FALLBACK_LOCKTIME: u8 = 0x03;
pub(crate) const GLOBAL_INPUT_COUNT: u8 = 0x04;
pub(crate) const GLOBAL_OUTPUT_COUNT: u8 = 0x05;
pub(crate) const GLOBAL_TX_MODIFIABLE: u8 = 0x06;
pub(crate) const GLOBAL_VERSION: u8 = 0xFB;
pub(crate) const IN_PREVIOUS_TXID: u8 = 0x0e;
pub(crate) const IN_OUTPUT_INDEX: u8 = 0x0f;
pub(crate) const IN_SEQUENCE: u8 = 0x10;
pub(crate) const IN_REQUIRED_TIME_LOCKTIME: u8 = 0x11;
pub(crate) const IN_REQUIRED_HEIGHT_LOCKTIME: u8 = 0x12;
pub(crate) const OUT_AMOUNT: u8 = 0x03;
pub(crate) const OUT_SCRIPT: u8 = 0x04;

// The field of an input that names its keys (as in version 0)
pub(crate) const IN_BIP32_DERIVATION: u8 = 0x06;

// Silent payments (BIP 375 and 376)
pub(crate) const GLOBAL_SP_ECDH_SHARE: u8 = 0x07;
pub(crate) const GLOBAL_SP_DLEQ: u8 = 0x08;
pub(crate) const IN_SP_ECDH_SHARE: u8 = 0x1d;
pub(crate) const IN_SP_DLEQ: u8 = 0x1e;
pub(crate) const IN_SP_SPEND_BIP32_DERIVATION: u8 = 0x1f;
pub(crate) const IN_SP_TWEAK: u8 = 0x20;
pub(crate) const OUT_SP_V0_INFO: u8 = 0x09;
pub(crate) const OUT_SP_V0_LABEL: u8 = 0x0a;

fn bad(why: &str) -> Error {
    Error::Psbt(format!("invalid PSBT: {why}"))
}

/// Reads a compact size integer at `at`.
fn read_compact(bytes: &[u8], at: &mut usize) -> Result<u64> {
    let first = *bytes.get(*at).ok_or_else(|| bad("it ends too early"))?;
    *at += 1;
    let width = match first {
        0xfd => 2,
        0xfe => 4,
        0xff => 8,
        n => return Ok(u64::from(n)),
    };
    let field = bytes.get(*at..*at + width).ok_or_else(|| bad("it ends too early"))?;
    *at += width;
    Ok(field.iter().rev().fold(0u64, |n, b| (n << 8) | u64::from(*b)))
}

fn write_compact(out: &mut Vec<u8>, n: u64) {
    match n {
        0..=0xfc => out.push(n as u8),
        0xfd..=0xffff => {
            out.push(0xfd);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(0xfe);
            out.extend_from_slice(&(n as u32).to_le_bytes());
        }
        _ => {
            out.push(0xff);
            out.extend_from_slice(&n.to_le_bytes());
        }
    }
}

/// One map of a PSBT (the global one, or an input's or an output's): its keys (the key type and its key data)
/// and values, in their order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Map(pub(crate) Vec<(Vec<u8>, Vec<u8>)>);

impl Map {
    /// The type of a key (a compact size at its start) and its key data.
    pub(crate) fn split_key(key: &[u8]) -> (u64, &[u8]) {
        let mut at = 0;
        match read_compact(key, &mut at) {
            Ok(t) => (t, &key[at..]),
            Err(_) => (u64::MAX, &[]),
        }
    }

    /// The value of the field of type `t` that has no key data.
    pub(crate) fn get(&self, t: u8) -> Option<&[u8]> {
        self.0.iter().find(|(k, _)| k.as_slice() == [t]).map(|(_, v)| v.as_slice())
    }

    /// The fields of type `t`: (key data, value).
    pub(crate) fn entries(&self, t: u8) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.0.iter().filter_map(move |(k, v)| {
            let (kt, data) = Self::split_key(k);
            (kt == u64::from(t)).then_some((data, v.as_slice()))
        })
    }

    /// Whether a field of type `t` is there.
    pub(crate) fn has(&self, t: u8) -> bool {
        self.entries(t).next().is_some()
    }

    /// Sets the field of type `t` with key data `data` (replacing its value if it is there).
    pub(crate) fn set(&mut self, t: u8, data: &[u8], value: Vec<u8>) {
        let mut key = vec![t];
        key.extend_from_slice(data);
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key, value)),
        }
    }

    /// The map without the fields of these types.
    fn without(&self, types: &[u8]) -> Map {
        Map(self.0.iter().filter(|(k, _)| !types.iter().any(|t| Self::split_key(k).0 == u64::from(*t))).cloned().collect())
    }

    fn read(bytes: &[u8], at: &mut usize) -> Result<Map> {
        let mut map = Map::default();
        loop {
            let key_len = read_compact(bytes, at)? as usize;
            if key_len == 0 {
                return Ok(map);
            }
            let key = bytes.get(*at..*at + key_len).ok_or_else(|| bad("it ends too early"))?.to_vec();
            *at += key_len;
            let value_len = read_compact(bytes, at)? as usize;
            let value = bytes.get(*at..*at + value_len).ok_or_else(|| bad("it ends too early"))?.to_vec();
            *at += value_len;
            if map.0.iter().any(|(k, _)| *k == key) {
                return Err(bad("a field is there twice"));
            }
            map.0.push((key, value));
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        for (key, value) in &self.0 {
            write_compact(out, key.len() as u64);
            out.extend_from_slice(key);
            write_compact(out, value.len() as u64);
            out.extend_from_slice(value);
        }
        out.push(0x00);
    }
}

/// A PSBT as its maps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawPsbt {
    pub(crate) global: Map,
    pub(crate) inputs: Vec<Map>,
    pub(crate) outputs: Vec<Map>,
}

/// A 4-byte little-endian number, the only valid length of such a field.
pub(crate) fn u32_field(value: &[u8], what: &str) -> Result<u32> {
    let bytes: [u8; 4] = value.try_into().map_err(|_| bad(&format!("{what} has the wrong length")))?;
    Ok(u32::from_le_bytes(bytes))
}

/// A compact size number that is a whole field.
fn compact_field(value: &[u8], what: &str) -> Result<u64> {
    let mut at = 0;
    let n = read_compact(value, &mut at)?;
    if at != value.len() {
        return Err(bad(&format!("{what} has the wrong length")));
    }
    Ok(n)
}

impl RawPsbt {
    /// Reads a PSBT of version 0 or 2.
    pub(crate) fn parse(bytes: &[u8]) -> Result<RawPsbt> {
        if !bytes.starts_with(MAGIC) {
            return Err(bad("it does not start as a PSBT does"));
        }
        let mut at = MAGIC.len();
        let global = Map::read(bytes, &mut at)?;
        let version = match global.get(GLOBAL_VERSION) {
            Some(v) => u32_field(v, "the PSBT version")?,
            None => 0,
        };
        let (n_in, n_out) = match version {
            0 => {
                let tx: Transaction = deserialize(global.get(GLOBAL_UNSIGNED_TX).ok_or_else(|| bad("the transaction is missing"))?)
                    .map_err(|_| bad("the transaction cannot be read"))?;
                (tx.input.len() as u64, tx.output.len() as u64)
            }
            2 => {
                let count = |t: u8, what: &str| -> Result<u64> {
                    compact_field(global.get(t).ok_or_else(|| bad(&format!("the number of {what} is missing")))?, what)
                };
                (count(GLOBAL_INPUT_COUNT, "inputs")?, count(GLOBAL_OUTPUT_COUNT, "outputs")?)
            }
            v => return Err(Error::Psbt(format!("PSBT version {v} is not supported (only 0 and 2)"))),
        };
        // every map takes at least one byte, which bounds the counts before anything is allocated
        if n_in.saturating_add(n_out) > (bytes.len() - at) as u64 {
            return Err(bad("it has fewer maps than its inputs and outputs need"));
        }
        let inputs = (0..n_in).map(|_| Map::read(bytes, &mut at)).collect::<Result<Vec<_>>>()?;
        let outputs = (0..n_out).map(|_| Map::read(bytes, &mut at)).collect::<Result<Vec<_>>>()?;
        if at != bytes.len() {
            return Err(bad("there is more after its last map"));
        }
        Ok(RawPsbt { global, inputs, outputs })
    }

    /// The PSBT's version (0 or 2).
    pub(crate) fn version(&self) -> u32 {
        self.global.get(GLOBAL_VERSION).and_then(|v| u32_field(v, "").ok()).unwrap_or(0)
    }

    pub(crate) fn serialize(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        self.global.write(&mut out);
        for map in self.inputs.iter().chain(&self.outputs) {
            map.write(&mut out);
        }
        out
    }

    /// Whether any field of silent payments (BIP 375, 376) is in it.
    pub(crate) fn has_silent_payment_fields(&self) -> bool {
        self.global.has(GLOBAL_SP_ECDH_SHARE)
            || self.global.has(GLOBAL_SP_DLEQ)
            || self.inputs.iter().any(|m| [IN_SP_ECDH_SHARE, IN_SP_DLEQ, IN_SP_SPEND_BIP32_DERIVATION, IN_SP_TWEAK].iter().any(|t| m.has(*t)))
            || self.outputs.iter().any(|m| m.has(OUT_SP_V0_INFO) || m.has(OUT_SP_V0_LABEL))
    }

    /// The outpoint an input of a version 2 PSBT spends.
    pub(crate) fn outpoint(&self, i: usize) -> Result<OutPoint> {
        let input = &self.inputs[i];
        let txid = input.get(IN_PREVIOUS_TXID).ok_or_else(|| bad(&format!("input #{i} does not say which transaction it spends")))?;
        let txid: [u8; 32] = txid.try_into().map_err(|_| bad(&format!("input #{i}'s txid has the wrong length")))?;
        let vout = input.get(IN_OUTPUT_INDEX).ok_or_else(|| bad(&format!("input #{i} does not say which output it spends")))?;
        Ok(OutPoint { txid: Txid::from_byte_array(txid), vout: u32_field(vout, "an output index")? })
    }

    /// The transaction of a version 2 PSBT. Every output must have its script: silent payment outputs get theirs
    /// from the signer, before this.
    pub(crate) fn unsigned_tx(&self) -> Result<Transaction> {
        let version = self.global.get(GLOBAL_TX_VERSION).ok_or_else(|| bad("the transaction version is missing"))?;
        let version = Version(u32_field(version, "the transaction version")? as i32);
        let mut input = Vec::with_capacity(self.inputs.len());
        for (i, map) in self.inputs.iter().enumerate() {
            let sequence = match map.get(IN_SEQUENCE) {
                Some(s) => Sequence(u32_field(s, "a sequence number")?),
                None => Sequence::MAX,
            };
            input.push(TxIn { previous_output: self.outpoint(i)?, script_sig: ScriptBuf::new(), sequence, witness: Witness::new() });
        }
        let mut output = Vec::with_capacity(self.outputs.len());
        for (j, map) in self.outputs.iter().enumerate() {
            let amount = map.get(OUT_AMOUNT).ok_or_else(|| bad(&format!("output #{j} has no amount")))?;
            let amount: [u8; 8] = amount.try_into().map_err(|_| bad(&format!("output #{j}'s amount has the wrong length")))?;
            let amount = i64::from_le_bytes(amount);
            if amount < 0 {
                return Err(bad(&format!("output #{j} has a negative amount")));
            }
            let script = map.get(OUT_SCRIPT).ok_or_else(|| bad(&format!("output #{j} has no script")))?;
            output.push(TxOut { value: Amount::from_sat(amount as u64), script_pubkey: ScriptBuf::from_bytes(script.to_vec()) });
        }
        Ok(Transaction { version, lock_time: self.lock_time()?, input, output })
    }

    /// BIP 370's lock time: the fallback lock time (or 0) when no input requires one; otherwise the latest of the
    /// heights the inputs require if they all can take a height (or the latest of their times if they all can
    /// take a time).
    fn lock_time(&self) -> Result<absolute::LockTime> {
        let mut any = false;
        let (mut heights_ok, mut times_ok) = (true, true);
        let (mut height, mut time) = (0u32, 0u32);
        for map in &self.inputs {
            let t = map.get(IN_REQUIRED_TIME_LOCKTIME).map(|v| u32_field(v, "a required time")).transpose()?;
            let h = map.get(IN_REQUIRED_HEIGHT_LOCKTIME).map(|v| u32_field(v, "a required height")).transpose()?;
            if t.is_none() && h.is_none() {
                continue;
            }
            any = true;
            match t {
                Some(t) => time = time.max(t),
                None => times_ok = false,
            }
            match h {
                Some(h) => height = height.max(h),
                None => heights_ok = false,
            }
        }
        let value = if !any {
            match self.global.get(GLOBAL_FALLBACK_LOCKTIME) {
                Some(v) => u32_field(v, "the fallback lock time")?,
                None => 0,
            }
        } else if heights_ok {
            height
        } else if times_ok {
            time
        } else {
            return Err(bad("its inputs require lock times that do not go together"));
        };
        Ok(absolute::LockTime::from_consensus(value))
    }
}

/// The version-0 form of a version 2 PSBT, which the `bitcoin` crate reads: the transaction put together, and
/// every other field kept (fields it does not know, such as those of silent payments, among its unknown ones).
pub(crate) fn to_v0(raw: &RawPsbt) -> Result<Psbt> {
    let tx = raw.unsigned_tx()?;
    let mut global = Map(vec![(vec![GLOBAL_UNSIGNED_TX], serialize(&tx))]);
    global.0.extend(
        raw.global
            .without(&[GLOBAL_UNSIGNED_TX, GLOBAL_TX_VERSION, GLOBAL_FALLBACK_LOCKTIME, GLOBAL_INPUT_COUNT, GLOBAL_OUTPUT_COUNT, GLOBAL_TX_MODIFIABLE, GLOBAL_VERSION])
            .0,
    );
    let v0 = RawPsbt {
        global,
        inputs: raw
            .inputs
            .iter()
            .map(|m| m.without(&[IN_PREVIOUS_TXID, IN_OUTPUT_INDEX, IN_SEQUENCE, IN_REQUIRED_TIME_LOCKTIME, IN_REQUIRED_HEIGHT_LOCKTIME]))
            .collect(),
        outputs: raw.outputs.iter().map(|m| m.without(&[OUT_AMOUNT, OUT_SCRIPT])).collect(),
    };
    Psbt::deserialize(&v0.serialize()).map_err(|e| Error::Psbt(format!("invalid PSBT: {e}")))
}

/// The version-0 form of a version 2 PSBT whose silent payment outputs may not have their scripts yet (Sparrow
/// leaves them to the signer): those get a stand-in script, `OP_1` and 32 zero bytes, which no key can spend.
/// For looking at the PSBT before the wallet works out the real ones; never for signing.
pub(crate) fn to_v0_with_stand_ins(raw: &RawPsbt) -> Result<Psbt> {
    let mut stand_in = raw.clone();
    for map in stand_in.outputs.iter_mut().filter(|m| !m.has(OUT_SCRIPT) && m.has(OUT_SP_V0_INFO)) {
        let mut script = vec![0x51, 0x20];
        script.extend_from_slice(&[0u8; 32]);
        map.set(OUT_SCRIPT, &[], script);
    }
    to_v0(&stand_in)
}

/// After a signature with SIGHASH_ALL (the only kind SeedLock makes), no input or output may be added or removed
/// any more: BIP 370's signer clears the "modifiable" flags of the inputs and the outputs.
pub(crate) fn clear_modifiable(raw: &mut RawPsbt) {
    if let Some(flags) = raw.global.get(GLOBAL_TX_MODIFIABLE).and_then(|f| f.first().copied()) {
        raw.global.set(GLOBAL_TX_MODIFIABLE, &[], vec![flags & !0b11]);
    }
}

/// Adds to a version 2 PSBT the fields that signing added to its version-0 form (signatures): every field of an
/// input of `signed` that the PSBT does not have yet.
pub(crate) fn add_new_entries(raw: &mut RawPsbt, signed: &Psbt) -> Result<()> {
    let v0 = RawPsbt::parse(&signed.serialize())?;
    if v0.inputs.len() != raw.inputs.len() {
        return Err(Error::Internal("the signed PSBT has another number of inputs".into()));
    }
    for (map, signed_map) in raw.inputs.iter_mut().zip(v0.inputs) {
        for (key, value) in signed_map.0 {
            if !map.0.iter().any(|(k, _)| *k == key) {
                map.0.push((key, value));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx() -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: absolute::LockTime::from_consensus(800_000),
            input: vec![TxIn {
                previous_output: OutPoint { txid: Txid::from_byte_array([7; 32]), vout: 3 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence(0xfffffffd),
                witness: Witness::new(),
            }],
            output: vec![TxOut { value: Amount::from_sat(5000), script_pubkey: ScriptBuf::from_bytes(vec![0x51, 0x20, 1, 2]) }],
        }
    }

    /// The version 2 form of a transaction, with an unknown field in each map.
    fn v2_of(tx: &Transaction) -> RawPsbt {
        let mut global = Map::default();
        global.set(GLOBAL_TX_VERSION, &[], 2u32.to_le_bytes().to_vec());
        global.set(GLOBAL_FALLBACK_LOCKTIME, &[], tx.lock_time.to_consensus_u32().to_le_bytes().to_vec());
        global.set(GLOBAL_INPUT_COUNT, &[], vec![tx.input.len() as u8]);
        global.set(GLOBAL_OUTPUT_COUNT, &[], vec![tx.output.len() as u8]);
        global.set(GLOBAL_TX_MODIFIABLE, &[], vec![3]);
        global.set(GLOBAL_VERSION, &[], 2u32.to_le_bytes().to_vec());
        global.set(GLOBAL_SP_ECDH_SHARE, &[2; 33], vec![3; 33]);
        let inputs = tx
            .input
            .iter()
            .map(|i| {
                let mut m = Map::default();
                m.set(IN_PREVIOUS_TXID, &[], i.previous_output.txid.to_byte_array().to_vec());
                m.set(IN_OUTPUT_INDEX, &[], i.previous_output.vout.to_le_bytes().to_vec());
                m.set(IN_SEQUENCE, &[], i.sequence.0.to_le_bytes().to_vec());
                m.set(IN_SP_TWEAK, &[], vec![9; 32]);
                m
            })
            .collect();
        let outputs = tx
            .output
            .iter()
            .map(|o| {
                let mut m = Map::default();
                m.set(OUT_AMOUNT, &[], (o.value.to_sat() as i64).to_le_bytes().to_vec());
                m.set(OUT_SCRIPT, &[], o.script_pubkey.to_bytes());
                m.set(OUT_SP_V0_LABEL, &[], 0u32.to_le_bytes().to_vec());
                m
            })
            .collect();
        RawPsbt { global, inputs, outputs }
    }

    #[test]
    fn a_version_2_psbt_reads_back_and_gives_its_transaction() {
        let tx = tx();
        let raw = v2_of(&tx);
        let bytes = raw.serialize();
        let again = RawPsbt::parse(&bytes).unwrap();
        assert_eq!(again, raw);
        assert_eq!(again.version(), 2);
        assert!(again.has_silent_payment_fields());
        assert_eq!(again.unsigned_tx().unwrap(), tx);
        // the version-0 form: the same transaction, the unknown fields kept, version 2's own ones gone
        let v0 = to_v0(&again).unwrap();
        assert_eq!(v0.unsigned_tx, tx);
        assert_eq!(v0.version, 0);
        let back = RawPsbt::parse(&v0.serialize()).unwrap();
        assert!(back.global.has(GLOBAL_SP_ECDH_SHARE) && !back.global.has(GLOBAL_TX_MODIFIABLE));
        assert!(back.inputs[0].has(IN_SP_TWEAK) && !back.inputs[0].has(IN_PREVIOUS_TXID));
        assert!(back.outputs[0].has(OUT_SP_V0_LABEL) && !back.outputs[0].has(OUT_AMOUNT));
    }

    #[test]
    fn lock_times_as_bip370_says() {
        let tx = tx();
        let mut raw = v2_of(&tx);
        assert_eq!(raw.lock_time().unwrap().to_consensus_u32(), 800_000);
        raw.inputs[0].set(IN_REQUIRED_HEIGHT_LOCKTIME, &[], 900_000u32.to_le_bytes().to_vec());
        assert_eq!(raw.lock_time().unwrap().to_consensus_u32(), 900_000);
        raw.inputs[0].set(IN_REQUIRED_TIME_LOCKTIME, &[], 1_700_000_000u32.to_le_bytes().to_vec());
        assert_eq!(raw.lock_time().unwrap().to_consensus_u32(), 900_000, "both possible: the height");
        let mut only_time = v2_of(&tx);
        only_time.inputs[0].set(IN_REQUIRED_TIME_LOCKTIME, &[], 1_700_000_000u32.to_le_bytes().to_vec());
        assert_eq!(only_time.lock_time().unwrap().to_consensus_u32(), 1_700_000_000);
    }

    #[test]
    fn broken_psbts_are_refused() {
        let raw = v2_of(&tx());
        let bytes = raw.serialize();
        assert!(RawPsbt::parse(&bytes[..bytes.len() - 1]).is_err(), "cut short");
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(RawPsbt::parse(&longer).is_err(), "more after the end");
        let mut twice = raw.clone();
        twice.inputs[0].0.push((vec![IN_SP_TWEAK], vec![1; 32]));
        assert!(RawPsbt::parse(&twice.serialize()).is_err(), "a field twice");
        let mut huge = raw.clone();
        huge.global.set(GLOBAL_INPUT_COUNT, &[], vec![0xfe, 0xff, 0xff, 0xff, 0x0f]);
        assert!(RawPsbt::parse(&huge.serialize()).is_err(), "more inputs than bytes");
        let mut v3 = raw;
        v3.global.set(GLOBAL_VERSION, &[], 3u32.to_le_bytes().to_vec());
        assert!(RawPsbt::parse(&v3.serialize()).is_err());
    }
}
