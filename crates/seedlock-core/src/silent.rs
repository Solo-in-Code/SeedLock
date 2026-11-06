//! Silent payments (BIP 352): one static address that payers use again and again, while each payment
//! lands on a taproot output of its own that nobody else can link to the address or to the other payments.
//!
//! What SeedLock does with them, as Sparrow's offline signer:
//!
//! * [`SilentKeys`]: the wallet's scan and spend keys (at BIP 352's paths), its address (`sp1q...`, and
//!   `tsp1q...` on the test networks) and its `sp()` descriptor for Sparrow (BIP 392). The descriptor holds the
//!   *scan private key*: with it, Sparrow finds the payments to the address; it cannot spend them.
//! * a sender's computations ([`input_hash`], [`shared_secret`], [`output_key`]): what a payment to a silent
//!   payment address (someone else's, or the wallet's own for its change) must look like on chain, worked out
//!   from the private keys of the inputs that pay it;
//! * BIP 374's DLEQ proofs ([`dleq_prove`], [`dleq_verify`]): that an ECDH share was made with the inputs' own
//!   keys, so that Sparrow can check the outputs that SeedLock worked out.
//!
//! The PSBT side of it (BIP 370, 375 and 376) is in [`crate::psbt`].

use crate::wallet::{descriptor_checksum, path_to_string, Wallet};
use crate::{Error, Result};
use bitcoin::bech32::primitives::decode::CheckedHrpstring;
use bitcoin::bech32::primitives::hrp::Hrp;
use bitcoin::bech32::primitives::iter::{ByteIterExt, Fe32IterExt};
use bitcoin::bech32::{Bech32m, Fe32};
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint};
use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::{All, Parity, PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};
use bitcoin::Network;

/// BIP 352's purpose number: the keys are at `m/352'/coin'/account'/...`.
pub const PURPOSE: u32 = 352;

/// The label of change: a wallet pays its change to its own address with label 0 (BIP 352).
pub const CHANGE_LABEL: u32 = 0;

/// The generator point G of secp256k1, compressed.
const G: [u8; 33] = [
    0x02, 0x79, 0xBE, 0x66, 0x7E, 0xF9, 0xDC, 0xBB, 0xAC, 0x55, 0xA0, 0x62, 0x95, 0xCE, 0x87, 0x0B, 0x07, 0x02, 0x9B,
    0xFC, 0xDB, 0x2D, 0xCE, 0x28, 0xD9, 0x59, 0xF2, 0x81, 0x5B, 0x16, 0xF8, 0x17, 0x98,
];

/// The order n of secp256k1's group.
const N: [u8; 32] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xBA, 0xAE, 0xDC,
    0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36, 0x41, 0x41,
];

/// `SHA256(SHA256(tag) || SHA256(tag) || data)`, BIP 340's tagged hash.
pub(crate) fn tagged_hash(tag: &str, parts: &[&[u8]]) -> [u8; 32] {
    let tag = sha256::Hash::hash(tag.as_bytes());
    let mut engine = sha256::Hash::engine();
    engine.input(tag.as_ref());
    engine.input(tag.as_ref());
    for part in parts {
        engine.input(part);
    }
    sha256::Hash::from_engine(engine).to_byte_array()
}

/// A number from a hash, which BIP 352 and 374 require to be below the group order (failing otherwise is
/// so unlikely that it never happens in practice).
fn scalar(bytes: [u8; 32], what: &str) -> Result<Scalar> {
    match Scalar::from_be_bytes(bytes) {
        Ok(s) if s != Scalar::ZERO => Ok(s),
        _ => Err(Error::Wallet(format!("silent payments: {what} is not a valid number (BIP 352)"))),
    }
}

/// The generator point G.
fn generator() -> PublicKey {
    PublicKey::from_slice(&G).expect("G is a valid point")
}

/// `x - n`, wrapping around 2^256.
fn minus_n(x: [u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let mut d = i16::from(x[i]) - i16::from(N[i]) - borrow;
        borrow = 0;
        if d < 0 {
            d += 256;
            borrow = 1;
        }
        out[i] = d as u8;
    }
    out
}

/// `bytes mod n`, for a 256-bit number (one subtraction is enough, as n is close to 2^256).
fn reduce(bytes: [u8; 32]) -> [u8; 32] {
    if bytes < N {
        bytes
    } else {
        minus_n(bytes)
    }
}

/// `(x + y) mod n`, for numbers below n.
fn add_mod_n(x: [u8; 32], y: [u8; 32]) -> [u8; 32] {
    let mut sum = [0u8; 32];
    let mut carry = 0u16;
    for i in (0..32).rev() {
        let s = u16::from(x[i]) + u16::from(y[i]) + carry;
        sum[i] = s as u8;
        carry = s >> 8;
    }
    // past 2^256 (carry) or at or past n: subtract n once (wrapping, which takes the carry away)
    if carry == 1 || sum >= N {
        minus_n(sum)
    } else {
        sum
    }
}

/// The sum of private keys modulo n, as BIP 352 adds up the keys of a transaction's inputs (a sum along the
/// way may be zero; only a zero total is no key, and then there can be no silent payment).
pub fn sum_private_keys(keys: &[SecretKey]) -> Option<SecretKey> {
    let mut total = [0u8; 32];
    for key in keys {
        total = add_mod_n(total, key.secret_bytes());
    }
    let sum = SecretKey::from_slice(&total).ok();
    total.iter_mut().for_each(|b| *b = 0);
    sum
}

// ---------------------------------------------------------------------------
// Points, where infinity (None) can come out of a sum
// ---------------------------------------------------------------------------

/// `k·P`, or None (infinity) for k = 0. `k` must be below n.
fn mul(secp: &Secp256k1<All>, p: &PublicKey, k: &[u8; 32]) -> Option<PublicKey> {
    if *k == [0u8; 32] {
        return None;
    }
    let k = Scalar::from_be_bytes(*k).ok()?;
    p.mul_tweak(secp, &k).ok()
}

/// `P + Q`, where None is infinity.
fn add(p: Option<PublicKey>, q: Option<PublicKey>) -> Option<PublicKey> {
    match (p, q) {
        (None, x) | (x, None) => x,
        (Some(p), Some(q)) => p.combine(&q).ok(),
    }
}

/// `-P`, where None is infinity.
fn neg(secp: &Secp256k1<All>, p: Option<PublicKey>) -> Option<PublicKey> {
    p.map(|p| p.negate(secp))
}

// ---------------------------------------------------------------------------
// Bech32m with a version, longer than segwit addresses (BIP 352: up to 1023 characters)
// ---------------------------------------------------------------------------

/// `hrp1q...`: version 0 (`q`) and the payload, in Bech32m.
fn encode_v0(hrp: &str, payload: &[u8]) -> String {
    let hrp = Hrp::parse_unchecked(hrp);
    payload.iter().copied().bytes_to_fes().with_checksum::<Bech32m>(&hrp).with_witness_version(Fe32::Q).chars().collect()
}

/// The human-readable part and version-0 payload of a Bech32m string.
fn decode_v0(text: &str) -> Result<(String, Vec<u8>)> {
    let bad = |why: &str| Error::Wallet(format!("not a silent payment code: {why}"));
    let mut checked = CheckedHrpstring::new::<Bech32m>(text).map_err(|e| bad(&e.to_string()))?;
    if checked.remove_witness_version() != Some(Fe32::Q) {
        return Err(bad("not version 0"));
    }
    Ok((checked.hrp().to_lowercase(), checked.byte_iter().collect()))
}

/// A silent payment address (version 0): `sp1q...` on mainnet, `tsp1q...` on the test networks (`sprt1q...` on
/// regtest, as Bitcoin Core and Sparrow write it), holding the scan key and the (possibly labelled) spend key.
pub fn encode_address(scan: &PublicKey, spend: &PublicKey, network: Network) -> String {
    let mut payload = Vec::with_capacity(66);
    payload.extend_from_slice(&scan.serialize());
    payload.extend_from_slice(&spend.serialize());
    let hrp = match network {
        Network::Bitcoin => "sp",
        Network::Regtest => "sprt",
        _ => "tsp",
    };
    encode_v0(hrp, &payload)
}

/// The keys in a silent payment address: (mainnet, scan key, spend key).
pub fn decode_address(address: &str) -> Result<(bool, PublicKey, PublicKey)> {
    let (hrp, payload) = decode_v0(address)?;
    let mainnet = match hrp.as_str() {
        "sp" => true,
        "tsp" | "sprt" => false,
        _ => return Err(Error::Wallet("not a silent payment address (sp1... or tsp1...)".into())),
    };
    if payload.len() != 66 {
        return Err(Error::Wallet("a silent payment address of version 0 holds exactly two keys".into()));
    }
    let key = |b: &[u8]| PublicKey::from_slice(b).map_err(|_| Error::Wallet("a silent payment address holds an invalid key".into()));
    Ok((mainnet, key(&payload[..33])?, key(&payload[33..])?))
}

// ---------------------------------------------------------------------------
// The wallet's keys
// ---------------------------------------------------------------------------

/// A wallet's silent payment keys, derived as BIP 352 recommends: the scan key at
/// `m/352'/coin'/account'/1'/0`, the spend key at `m/352'/coin'/account'/0'/0` (both hardened down to the
/// account, so that handing out the scan key reveals nothing of the other keys).
pub struct SilentKeys {
    network: Network,
    fingerprint: Fingerprint,
    account_path: DerivationPath,
    scan: SecretKey,
    spend: SecretKey,
    scan_public: PublicKey,
    spend_public: PublicKey,
    secp: Secp256k1<All>,
}

impl Drop for SilentKeys {
    fn drop(&mut self) {
        self.scan.non_secure_erase();
        self.spend.non_secure_erase();
    }
}

impl SilentKeys {
    /// Keys given as they are, without a wallet (for the BIPs' test vectors): no fingerprint, and the path of
    /// account 0 of the network.
    pub fn from_private_keys(scan: SecretKey, spend: SecretKey, network: Network) -> Self {
        let secp = Secp256k1::new();
        let coin = if network == Network::Bitcoin { 0 } else { 1 };
        let account_path = DerivationPath::from(vec![
            ChildNumber::Hardened { index: PURPOSE },
            ChildNumber::Hardened { index: coin },
            ChildNumber::Hardened { index: 0 },
        ]);
        SilentKeys {
            network,
            fingerprint: Fingerprint::default(),
            scan_public: PublicKey::from_secret_key(&secp, &scan),
            spend_public: PublicKey::from_secret_key(&secp, &spend),
            account_path,
            scan,
            spend,
            secp,
        }
    }

    /// The keys of `account` (SeedLock and Sparrow use account 0).
    pub fn new(wallet: &Wallet, account: u32) -> Result<Self> {
        let hardened = |i: u32| ChildNumber::from_hardened_idx(i).map_err(|e| Error::Wallet(format!("bad path index: {e}")));
        let account_path = DerivationPath::from(vec![hardened(PURPOSE)?, hardened(wallet.coin_type())?, hardened(account)?]);
        let zero = ChildNumber::from_normal_idx(0).map_err(|e| Error::Wallet(e.to_string()))?;
        let spend = wallet.private_key_at(&account_path.extend([hardened(0)?, zero]))?;
        let scan = wallet.private_key_at(&account_path.extend([hardened(1)?, zero]))?;
        let secp = Secp256k1::new();
        Ok(SilentKeys {
            network: wallet.network(),
            fingerprint: wallet.fingerprint(),
            scan_public: PublicKey::from_secret_key(&secp, &scan),
            spend_public: PublicKey::from_secret_key(&secp, &spend),
            account_path,
            scan,
            spend,
            secp,
        })
    }

    /// The network the keys are for.
    pub fn network(&self) -> Network {
        self.network
    }

    /// The master key's fingerprint.
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// `m/352'/coin'/account'`
    pub fn account_path(&self) -> &DerivationPath {
        &self.account_path
    }

    /// `m/352'/coin'/account'/0'/0`, the spend key's path (as Sparrow writes it into a PSBT, BIP 376).
    pub fn spend_path(&self) -> DerivationPath {
        let steps = [ChildNumber::Hardened { index: 0 }, ChildNumber::Normal { index: 0 }];
        self.account_path.extend(steps)
    }

    /// The scan public key `B_scan`.
    pub fn scan_public(&self) -> PublicKey {
        self.scan_public
    }

    /// The spend public key `B_spend`.
    pub fn spend_public(&self) -> PublicKey {
        self.spend_public
    }

    /// `hash_BIP0352/Label(ser256(b_scan) || ser32(m))`, the tweak of label `m`.
    pub fn label_tweak(&self, m: u32) -> Result<Scalar> {
        scalar(tagged_hash("BIP0352/Label", &[&self.scan.secret_bytes(), &m.to_be_bytes()]), "a label's tweak")
    }

    /// `B_m = B_spend + label_tweak(m)·G`, the spend key of the address with label `m`.
    pub fn labeled_spend(&self, m: u32) -> Result<PublicKey> {
        self.spend_public
            .add_exp_tweak(&self.secp, &self.label_tweak(m)?)
            .map_err(|_| Error::Wallet("silent payments: a label gives an invalid key".into()))
    }

    /// The wallet's silent payment address, `sp1q...` (`tsp1q...` on the test networks): the one to give to payers.
    pub fn address(&self) -> String {
        encode_address(&self.scan_public, &self.spend_public, self.network)
    }

    /// The address with label `m` (label 0 is the change address).
    pub fn labeled_address(&self, m: u32) -> Result<String> {
        Ok(encode_address(&self.scan_public, &self.labeled_spend(m)?, self.network))
    }

    /// The `spscan` key of BIP 392: the scan private key and the spend public key, in Bech32m (`spscan1q...`,
    /// `tspscan1q...` on the test networks). Sparrow takes it in the "spscan" field of a watch-only keystore.
    /// Like [`SilentKeys::descriptor`], it finds the payments to the address, but cannot spend them.
    pub fn spscan(&self) -> String {
        let mut payload = Vec::with_capacity(65);
        payload.extend_from_slice(&self.scan.secret_bytes());
        payload.extend_from_slice(&self.spend_public.serialize());
        let text = encode_v0(if self.network == Network::Bitcoin { "spscan" } else { "tspscan" }, &payload);
        payload.iter_mut().for_each(|b| *b = 0);
        text
    }

    /// The watch-only wallet for Sparrow (BIP 392): `sp([fingerprint/352h/coinh/0h]spscan1q...)#checksum`.
    /// Sparrow scans the blockchain with it for the payments to the address and builds transactions that spend
    /// them, which SeedLock then signs; it holds the scan private key, so whoever has it can see those payments,
    /// but not spend them.
    pub fn descriptor(&self) -> Result<String> {
        let body = format!("sp([{}/{}]{})", self.fingerprint, path_to_string(&self.account_path), self.spscan());
        let checksum = descriptor_checksum(&body)?;
        Ok(format!("{body}#{checksum}"))
    }

    /// The key that spends an output paid to this wallet (BIP 376): `d = (b_spend + tweak) mod n`, negated when
    /// `d·G` has an odd y coordinate, as BIP 340 signatures need. The tweak comes from the PSBT
    /// (`PSBT_IN_SP_TWEAK`), already including the label's tweak for the change.
    pub fn spending_key(&self, tweak: &[u8; 32]) -> Result<SecretKey> {
        let tweak = Scalar::from_be_bytes(*tweak).map_err(|_| Error::Psbt("a silent payment tweak is out of range".into()))?;
        let d = self.spend.add_tweak(&tweak).map_err(|_| Error::Psbt("a silent payment tweak gives no valid key".into()))?;
        Ok(if d.x_only_public_key(&self.secp).1 == Parity::Odd { d.negate() } else { d })
    }
}

// ---------------------------------------------------------------------------
// A sender's computations (BIP 352, "Creating outputs")
// ---------------------------------------------------------------------------

/// `input_hash = hash_BIP0352/Inputs(outpoint_L || ser_P(A))`: `outpoint_L` is the smallest of the transaction's
/// outpoints (36 bytes each, as serialized in a transaction), `A` the sum of the public keys of its inputs that
/// count for silent payments.
pub fn input_hash(smallest_outpoint: &[u8; 36], input_keys: &PublicKey) -> Result<Scalar> {
    scalar(tagged_hash("BIP0352/Inputs", &[smallest_outpoint, &input_keys.serialize()]), "the inputs' hash")
}

/// `ecdh_shared_secret = input_hash·C`, where `C = a·B_scan` is the ECDH share for a scan key (`a` being the sum
/// of the inputs' private keys).
pub fn shared_secret(input_hash: &Scalar, ecdh_share: &PublicKey) -> Result<PublicKey> {
    let secp = Secp256k1::new();
    ecdh_share.mul_tweak(&secp, input_hash).map_err(|_| Error::Wallet("silent payments: invalid shared secret".into()))
}

/// The output key of the `k`-th output (0, 1, ...) for a scan key: `P = B_m + t_k·G`, with
/// `t_k = hash_BIP0352/SharedSecret(ser_P(ecdh_shared_secret) || ser32(k))` and `B_m` the spend key of the address
/// paid (labelled or not). The output's script is `OP_1 <P>` (a taproot output with this key, not tweaked again).
pub fn output_key(shared_secret: &PublicKey, spend: &PublicKey, k: u32) -> Result<XOnlyPublicKey> {
    let secp = Secp256k1::new();
    let t_k = scalar(tagged_hash("BIP0352/SharedSecret", &[&shared_secret.serialize(), &k.to_be_bytes()]), "t_k")?;
    let p = spend.add_exp_tweak(&secp, &t_k).map_err(|_| Error::Wallet("silent payments: invalid output key".into()))?;
    Ok(p.x_only_public_key().0)
}

/// `C = a·B`: the ECDH share of private key (or sum of private keys) `a` with scan key `B`.
pub fn ecdh_share(a: &SecretKey, scan: &PublicKey) -> Result<PublicKey> {
    let secp = Secp256k1::new();
    scan.mul_tweak(&secp, &Scalar::from(*a)).map_err(|_| Error::Wallet("silent payments: invalid ECDH share".into()))
}

// ---------------------------------------------------------------------------
// DLEQ proofs (BIP 374)
// ---------------------------------------------------------------------------

/// The challenge `e` of a proof: `hash_BIP0374/challenge(A || B || C || G || R1 || R2 || m) mod n`.
fn challenge(points: [&PublicKey; 6], message: Option<&[u8; 32]>) -> [u8; 32] {
    let serialized: Vec<[u8; 33]> = points.iter().map(|p| p.serialize()).collect();
    let mut parts: Vec<&[u8]> = serialized.iter().map(|p| &p[..]).collect();
    if let Some(m) = message {
        parts.push(m);
    }
    reduce(tagged_hash("BIP0374/challenge", &parts))
}

/// BIP 374's proof that `C = a·B` was made with the same `a` as `A = a·G`, without revealing `a`: what goes into a
/// PSBT with each ECDH share (BIP 375). `aux` is 32 fresh random bytes.
pub fn dleq_prove(a: &SecretKey, b: &PublicKey, aux: &[u8; 32]) -> Result<[u8; 64]> {
    dleq_prove_with(a, b, aux, &generator(), None)
}

/// Checks a proof made by [`dleq_prove`]: that `A = a·G` and `C = a·B` share the same `a`.
pub fn dleq_verify(a: &PublicKey, b: &PublicKey, c: &PublicKey, proof: &[u8; 64]) -> bool {
    dleq_verify_with(a, b, c, proof, &generator(), None)
}

/// BIP 374's GenerateProof(a, B, r, G, m), for any generator `g` and an optional message (the BIP's test vectors
/// use both).
pub fn dleq_prove_with(a: &SecretKey, b: &PublicKey, aux: &[u8; 32], g: &PublicKey, message: Option<&[u8; 32]>) -> Result<[u8; 64]> {
    let secp = Secp256k1::new();
    let fail = || Error::Wallet("silent payments: the proof could not be made (BIP 374)".into());
    let a_bytes = a.secret_bytes();
    let big_a = mul(&secp, g, &a_bytes).ok_or_else(fail)?;
    let big_c = mul(&secp, b, &a_bytes).ok_or_else(fail)?;
    let aux_hash = tagged_hash("BIP0374/aux", &[aux]);
    let mut t = [0u8; 32];
    for i in 0..32 {
        t[i] = a_bytes[i] ^ aux_hash[i];
    }
    let (sa, sc) = (big_a.serialize(), big_c.serialize());
    let mut nonce_parts: Vec<&[u8]> = vec![&t, &sa, &sc];
    if let Some(m) = message {
        nonce_parts.push(m);
    }
    let k = reduce(tagged_hash("BIP0374/nonce", &nonce_parts));
    t.iter_mut().for_each(|x| *x = 0);
    let r1 = mul(&secp, g, &k).ok_or_else(fail)?;
    let r2 = mul(&secp, b, &k).ok_or_else(fail)?;
    let e = challenge([&big_a, b, &big_c, g, &r1, &r2], message);
    // s = k + e·a (mod n)
    let k_key = SecretKey::from_slice(&k).map_err(|_| fail())?;
    let s = if e == [0u8; 32] {
        k_key
    } else {
        let ea = a.mul_tweak(&Scalar::from_be_bytes(e).map_err(|_| fail())?).map_err(|_| fail())?;
        k_key.add_tweak(&Scalar::from(ea)).map_err(|_| fail())?
    };
    let mut proof = [0u8; 64];
    proof[..32].copy_from_slice(&e);
    proof[32..].copy_from_slice(&s.secret_bytes());
    if !dleq_verify_with(&big_a, b, &big_c, &proof, g, message) {
        return Err(fail());
    }
    Ok(proof)
}

/// BIP 374's VerifyProof(A, B, C, proof, G, m).
pub fn dleq_verify_with(a: &PublicKey, b: &PublicKey, c: &PublicKey, proof: &[u8; 64], g: &PublicKey, message: Option<&[u8; 32]>) -> bool {
    let secp = Secp256k1::new();
    let mut e = [0u8; 32];
    let mut s = [0u8; 32];
    e.copy_from_slice(&proof[..32]);
    s.copy_from_slice(&proof[32..]);
    if e >= N || s >= N {
        return false;
    }
    // R1 = s·G - e·A, R2 = s·B - e·C; neither may be infinity
    let Some(r1) = add(mul(&secp, g, &s), neg(&secp, mul(&secp, a, &e))) else { return false };
    let Some(r2) = add(mul(&secp, b, &s), neg(&secp, mul(&secp, c, &e))) else { return false };
    challenge([a, b, c, g, &r1, &r2], message) == e
}

#[cfg(test)]
mod tests {
    use super::*;
    use bip39::Mnemonic;

    fn keys(network: Network) -> SilentKeys {
        let m = Mnemonic::parse("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about").unwrap();
        SilentKeys::new(&Wallet::new(&m, "", network).unwrap(), 0).unwrap()
    }

    #[test]
    fn the_generator_and_the_group_order() {
        let secp = Secp256k1::new();
        let one = SecretKey::from_slice(&{
            let mut b = [0u8; 32];
            b[31] = 1;
            b
        })
        .unwrap();
        assert_eq!(PublicKey::from_secret_key(&secp, &one), generator());
        // n mod n = 0, (n + 5) mod n = 5, n - 1 stays
        assert_eq!(reduce(N), [0u8; 32]);
        let mut n_minus_1 = N;
        n_minus_1[31] -= 1;
        assert_eq!(reduce(n_minus_1), n_minus_1);
        let mut above = N;
        above[31] += 5;
        let mut five = [0u8; 32];
        five[31] = 5;
        assert_eq!(reduce(above), five);
        // (n - 1) + 6 = 5 (mod n); (n - 1) + (n - 1) = n - 2, past 2^256
        assert_eq!(add_mod_n(n_minus_1, {
            let mut six = [0u8; 32];
            six[31] = 6;
            six
        }), five);
        let mut n_minus_2 = N;
        n_minus_2[31] -= 2;
        assert_eq!(add_mod_n(n_minus_1, n_minus_1), n_minus_2);
    }

    #[test]
    fn sums_of_keys_may_pass_through_zero() {
        let one = SecretKey::from_slice(&{
            let mut b = [0u8; 32];
            b[31] = 1;
            b
        })
        .unwrap();
        let minus_one = one.negate();
        assert!(sum_private_keys(&[one, minus_one]).is_none());
        assert_eq!(sum_private_keys(&[one, minus_one, one]), Some(one));
        assert!(sum_private_keys(&[]).is_none());
    }

    #[test]
    fn address_and_descriptor_of_the_test_seed() {
        let k = keys(Network::Testnet);
        assert_eq!(k.account_path().to_string(), "352'/1'/0'");
        assert_eq!(k.spend_path().to_string(), "352'/1'/0'/0'/0");
        let address = k.address();
        assert!(address.starts_with("tsp1q") && address.len() == 117, "{address}");
        let (mainnet, scan, spend) = decode_address(&address).unwrap();
        assert!(!mainnet);
        assert_eq!((scan, spend), (k.scan_public(), k.spend_public()));
        let descriptor = k.descriptor().unwrap();
        assert!(descriptor.starts_with("sp([73c5da0a/352h/1h/0h]tspscan1q"), "{descriptor}");
        let (body, checksum) = descriptor.split_once('#').unwrap();
        assert_eq!(descriptor_checksum(body).unwrap(), checksum);
        // the spscan key: the scan private key and the spend public key
        let spscan = &body[body.find(']').unwrap() + 1..body.len() - 1];
        let (hrp, payload) = decode_v0(spscan).unwrap();
        assert_eq!(hrp, "tspscan");
        assert_eq!(&payload[..32], &k.scan.secret_bytes());
        assert_eq!(&payload[32..], &k.spend_public().serialize());

        let main = keys(Network::Bitcoin);
        assert!(main.address().starts_with("sp1q") && main.address().len() == 116);
        assert!(main.descriptor().unwrap().starts_with("sp([73c5da0a/352h/0h/0h]spscan1q"));
        assert_ne!(main.scan_public(), k.scan_public());
    }

    #[test]
    fn labels_change_the_spend_key_only() {
        let k = keys(Network::Bitcoin);
        let change = k.labeled_address(CHANGE_LABEL).unwrap();
        let (_, scan, spend) = decode_address(&change).unwrap();
        assert_eq!(scan, k.scan_public());
        assert_ne!(spend, k.spend_public());
        assert_eq!(spend, k.labeled_spend(0).unwrap());
        assert_ne!(k.labeled_spend(1).unwrap(), k.labeled_spend(0).unwrap());
    }

    #[test]
    fn spending_key_matches_the_output_key() {
        // an output paid to the wallet: P = B_spend + t·G; the spending key d (BIP 376) has x(d·G) = x(P)
        let k = keys(Network::Testnet);
        let tweak = tagged_hash("test", &[b"some tweak"]);
        let p = k.spend_public().add_exp_tweak(&k.secp, &Scalar::from_be_bytes(tweak).unwrap()).unwrap();
        let d = k.spending_key(&tweak).unwrap();
        let (x, parity) = d.x_only_public_key(&k.secp);
        assert_eq!(x, p.x_only_public_key().0);
        assert_eq!(parity, Parity::Even);
    }

    #[test]
    fn dleq_proofs_check_out_and_refuse_a_wrong_share() {
        let secp = Secp256k1::new();
        let a = SecretKey::from_slice(&tagged_hash("a", &[])).unwrap();
        let b = PublicKey::from_secret_key(&secp, &SecretKey::from_slice(&tagged_hash("b", &[])).unwrap());
        let big_a = PublicKey::from_secret_key(&secp, &a);
        let c = ecdh_share(&a, &b).unwrap();
        let proof = dleq_prove(&a, &b, &[7u8; 32]).unwrap();
        assert!(dleq_verify(&big_a, &b, &c, &proof));
        // another share, another key, a changed proof: refused
        let other = ecdh_share(&SecretKey::from_slice(&tagged_hash("c", &[])).unwrap(), &b).unwrap();
        assert!(!dleq_verify(&big_a, &b, &other, &proof));
        assert!(!dleq_verify(&b, &b, &c, &proof));
        let mut changed = proof;
        changed[63] ^= 1;
        assert!(!dleq_verify(&big_a, &b, &c, &changed));
        // e and s at or above n
        let mut high = proof;
        high[..32].copy_from_slice(&N);
        assert!(!dleq_verify(&big_a, &b, &c, &high));
    }
}
