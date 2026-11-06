//! Silent payments against the BIPs' own test vectors (tests/data, written by tools/silent_vectors.py from
//! bitcoin/bips): BIP 352's sending and receiving vectors, and BIP 374's DLEQ proofs.

use seedlock_core::bitcoin::hashes::{hash160, Hash};
use seedlock_core::bitcoin::secp256k1::{Parity, PublicKey, Secp256k1, SecretKey};
use seedlock_core::bitcoin::Network;
use seedlock_core::silent::{self, SilentKeys};
use std::collections::BTreeMap;
use std::path::Path;

fn data(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap()
}

fn hex(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn array32(bytes: &[u8]) -> [u8; 32] {
    bytes.try_into().unwrap()
}

/// One record of a data file: its first line, then its `word values...` lines.
type Record = (String, Vec<(String, Vec<String>)>);

/// The records of a data file: a first line naming it, then `word values...` lines, up to `end`.
fn records(text: &str) -> Vec<Record> {
    let mut out = Vec::new();
    let mut current: Option<Record> = None;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match (&mut current, word) {
            (None, _) => current = Some((line.to_string(), Vec::new())),
            (Some(_), "end") => out.push(current.take().unwrap()),
            (Some((_, fields)), _) => fields.push((word.to_string(), rest.split(' ').filter(|s| !s.is_empty()).map(String::from).collect())),
        }
    }
    out
}

/// The items of a serialized witness (count, then each item with its length).
fn witness_items(bytes: &[u8]) -> Vec<Vec<u8>> {
    fn compact(bytes: &[u8], at: &mut usize) -> usize {
        let first = bytes[*at];
        *at += 1;
        let width = match first {
            0xfd => 2,
            0xfe => 4,
            0xff => 8,
            n => return n as usize,
        };
        let mut n = 0usize;
        for i in 0..width {
            n |= (bytes[*at + i] as usize) << (8 * i);
        }
        *at += width;
        n
    }
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut at = 0;
    let count = compact(bytes, &mut at);
    (0..count)
        .map(|_| {
            let len = compact(bytes, &mut at);
            let item = bytes[at..at + len].to_vec();
            at += len;
            item
        })
        .collect()
}

/// The data pushes of a script (enough for the scriptSigs of the vectors).
fn pushes(script: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < script.len() {
        let op = script[at] as usize;
        at += 1;
        let len = match op {
            1..=75 => op,
            76 => {
                at += 1;
                script[at - 1] as usize
            }
            77 => {
                at += 2;
                script[at - 2] as usize | (script[at - 1] as usize) << 8
            }
            _ => continue,
        };
        out.push(script[at..at + len].to_vec());
        at += len;
    }
    out
}

/// BIP 341's NUMS point H, the internal key of taproot outputs that have no key path.
const NUMS_H: &str = "50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0";

/// The private key an input adds to the sum for silent payments (negated for a taproot output key with an odd
/// y), or None when the input does not count (BIP 352, "Inputs For Shared Secret Derivation").
fn input_key(prevout: &[u8], script_sig: &[u8], witness: &[Vec<u8>], key: SecretKey) -> Option<SecretKey> {
    let secp = Secp256k1::new();
    let public = PublicKey::from_secret_key(&secp, &key);
    match prevout {
        [0x51, 0x20, ..] if prevout.len() == 34 => {
            let mut stack = witness.to_vec();
            if stack.len() > 1 && stack.last()?.first() == Some(&0x50) {
                stack.pop(); // the annex
            }
            if stack.len() > 1 {
                let control = stack.last()?;
                if control.len() >= 33 && to_hex(&control[1..33]) == NUMS_H {
                    return None;
                }
            }
            Some(if public.x_only_public_key().1 == Parity::Odd { key.negate() } else { key })
        }
        [0x00, 0x14, ..] if prevout.len() == 22 => (witness.last()?.len() == 33).then_some(key),
        [0xa9, 0x14, .., 0x87] if prevout.len() == 23 => {
            let redeem = pushes(script_sig).pop()?;
            (redeem.len() == 22 && redeem[..2] == [0x00, 0x14] && witness.last()?.len() == 33).then_some(key)
        }
        [0x76, 0xa9, 0x14, .., 0x88, 0xac] if prevout.len() == 25 => {
            (hash160::Hash::hash(&public.serialize()).to_byte_array()[..] == prevout[3..23]).then_some(key)
        }
        _ => None,
    }
}

#[test]
fn bip352_sending() {
    let secp = Secp256k1::new();
    let mut cases = 0;
    for (name, fields) in records(&data("bip352.txt")).into_iter().filter(|(n, _)| n.starts_with("send ")) {
        let mut outpoints: Vec<[u8; 36]> = Vec::new();
        let mut keys = Vec::new();
        let mut recipients: Vec<(PublicKey, PublicKey)> = Vec::new();
        let mut expected: Vec<Vec<String>> = Vec::new();
        for (word, values) in &fields {
            match word.as_str() {
                "in" => {
                    let mut txid = hex(&values[0]);
                    txid.reverse(); // shown big-endian, serialized little-endian
                    let mut outpoint = [0u8; 36];
                    outpoint[..32].copy_from_slice(&txid);
                    outpoint[32..].copy_from_slice(&values[1].parse::<u32>().unwrap().to_le_bytes());
                    outpoints.push(outpoint);
                    let key = SecretKey::from_slice(&hex(&values[5])).unwrap();
                    if let Some(k) = input_key(&hex(&values[2]), &hex(&values[3]), &witness_items(&hex(&values[4])), key) {
                        keys.push(k);
                    }
                }
                "to" => {
                    let (_, scan, spend) = silent::decode_address(&values[0]).unwrap();
                    let count = values.get(1).map_or(1, |c| c.parse().unwrap());
                    recipients.extend(std::iter::repeat_n((scan, spend), count));
                }
                "out" => expected.push(values.clone()),
                _ => panic!("{name}: unknown line {word}"),
            }
        }
        // the outputs, grouped by scan key, in BIP 375's order (spend keys ascending)
        let computed: Option<Vec<String>> = (|| {
            let a = silent::sum_private_keys(&keys)?;
            let big_a = PublicKey::from_secret_key(&secp, &a);
            let smallest = *outpoints.iter().min()?;
            let input_hash = silent::input_hash(&smallest, &big_a).ok()?;
            let mut groups: BTreeMap<[u8; 33], Vec<PublicKey>> = BTreeMap::new();
            for (scan, spend) in &recipients {
                groups.entry(scan.serialize()).or_default().push(*spend);
            }
            let mut out = Vec::new();
            for (scan, mut spends) in groups {
                if spends.len() > 2323 {
                    return None; // K_max
                }
                spends.sort_by_key(|s| s.serialize());
                let share = silent::ecdh_share(&a, &PublicKey::from_slice(&scan).unwrap()).ok()?;
                let secret = silent::shared_secret(&input_hash, &share).ok()?;
                for (k, spend) in spends.iter().enumerate() {
                    out.push(to_hex(&silent::output_key(&secret, spend, k as u32).ok()?.serialize()));
                }
            }
            Some(out)
        })();
        let mut got = computed.unwrap_or_default();
        got.sort();
        let accepted = expected.iter().any(|set| {
            let mut set = set.clone();
            set.sort();
            set == got
        });
        assert!(accepted, "{name}: computed {got:?}, expected one of {expected:?}");
        cases += 1;
    }
    assert_eq!(cases, 29);
}

#[test]
fn bip352_receiving_addresses_and_spending_keys() {
    let secp = Secp256k1::new();
    let mut cases = 0;
    for (name, fields) in records(&data("bip352.txt")).into_iter().filter(|(n, _)| n.starts_with("receive ")) {
        let mut keys: Option<SilentKeys> = None;
        let mut labels = Vec::new();
        let mut expected_addresses = Vec::new();
        let mut found = Vec::new();
        for (word, values) in &fields {
            match word.as_str() {
                "keys" => {
                    let scan = SecretKey::from_slice(&hex(&values[0])).unwrap();
                    let spend = SecretKey::from_slice(&hex(&values[1])).unwrap();
                    keys = Some(SilentKeys::from_private_keys(scan, spend, Network::Bitcoin));
                }
                "label" => labels.push(values[0].parse::<u32>().unwrap()),
                "address" => expected_addresses.push(values[0].clone()),
                "found" => found.push((values[0].clone(), array32(&hex(&values[1])))),
                _ => panic!("{name}: unknown line {word}"),
            }
        }
        let keys = keys.unwrap();
        // the address, and each labelled address
        let mut addresses = vec![keys.address()];
        for m in &labels {
            addresses.push(keys.labeled_address(*m).unwrap());
        }
        addresses.sort();
        expected_addresses.sort();
        assert_eq!(addresses, expected_addresses, "{name}");
        // each output found: the key that spends it (BIP 376) has the output's key
        for (output, tweak) in found {
            let d = keys.spending_key(&tweak).unwrap();
            assert_eq!(to_hex(&d.x_only_public_key(&secp).0.serialize()), output, "{name}");
        }
        cases += 1;
    }
    assert_eq!(cases, 30);
}

fn csv(name: &str) -> Vec<Vec<String>> {
    data(name).lines().skip(1).map(|l| l.split(',').map(String::from).collect()).collect()
}

/// A point, or None for one that is not (such as `INFINITY`).
fn point(text: &str) -> Option<PublicKey> {
    if text.len() != 66 || !text.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    PublicKey::from_slice(&hex(text)).ok()
}

fn message(text: &str) -> Option<[u8; 32]> {
    (!text.is_empty()).then(|| array32(&hex(text)))
}

#[test]
fn bip374_proofs() {
    let rows = csv("bip374_generate_proof.csv");
    assert_eq!(rows.len(), 14);
    for row in rows {
        // index, point_G, scalar_a, point_B, auxrand_r, message, result_proof, comment
        let proof = (|| {
            let g = point(&row[1])?;
            let a = SecretKey::from_slice(&hex(&row[2])).ok()?;
            let b = point(&row[3])?;
            silent::dleq_prove_with(&a, &b, &array32(&hex(&row[4])), &g, message(&row[5]).as_ref()).ok()
        })();
        match row[6].as_str() {
            "INVALID" => assert!(proof.is_none(), "{}", row[7]),
            expected => assert_eq!(to_hex(&proof.expect(&row[7])), expected, "{}", row[7]),
        }
    }
    let rows = csv("bip374_verify_proof.csv");
    assert_eq!(rows.len(), 17);
    for row in rows {
        // index, point_G, point_A, point_B, point_C, proof, message, result_success, comment
        let (g, a, b, c) = (point(&row[1]).unwrap(), point(&row[2]).unwrap(), point(&row[3]).unwrap(), point(&row[4]).unwrap());
        let proof: [u8; 64] = hex(&row[5]).try_into().unwrap();
        let ok = silent::dleq_verify_with(&a, &b, &c, &proof, &g, message(&row[6]).as_ref());
        assert_eq!(ok, row[7] == "TRUE", "{}", row[8]);
    }
}
