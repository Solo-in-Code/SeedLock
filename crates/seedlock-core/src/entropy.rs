//! Sources of randomness: the operating system, dice rolls, and a
//! word-based password generator.

use crate::{Error, Result};
use bitcoin::hashes::{sha256, Hash};
use zeroize::Zeroizing;

/// Fill `buf` from the operating system's CSPRNG.
pub fn random_bytes(buf: &mut [u8]) -> Result<()> {
    getrandom::getrandom(buf).map_err(|e| Error::Rng(e.to_string()))
}

/// Entropy length in bytes for a mnemonic of `words` words (12 -> 16, ..., 24 -> 32).
pub fn entropy_len_for_words(words: usize) -> Option<usize> {
    match words {
        12 => Some(16),
        15 => Some(20),
        18 => Some(24),
        21 => Some(28),
        24 => Some(32),
        _ => None,
    }
}

/// Fresh entropy straight from the operating system.
pub fn generate(entropy_len: usize) -> Result<Zeroizing<Vec<u8>>> {
    check_len(entropy_len)?;
    let mut v = Zeroizing::new(vec![0u8; entropy_len]);
    random_bytes(&mut v)?;
    Ok(v)
}

/// Number of dice rolls needed for a given entropy length.
///
/// Enough rolls to cover all the entropy: 50 rolls (129 bits) for a 12-word seed,
/// 62, 75 and 87 for 15, 18 and 21 words, and 100 rolls (258 bits) for 24 words.
///
/// `dice_only` is SeedSigner's mode (see [`from_dice`]): there the number for
/// 24 words is SeedSigner's own 99 rolls (255.9 bits), so the same rolls give
/// exactly the same seed.
pub fn min_dice_rolls(entropy_len: usize, dice_only: bool) -> usize {
    match entropy_len {
        16 => 50,
        20 => 62,
        24 => 75,
        28 => 87,
        _ if dice_only => 99,
        _ => 100,
    }
}

/// Check a batch of dice rolls (for example one line typed by a person) and
/// return how many rolls it contains. Whitespace and commas are ignored; every
/// other character must be a digit from 1 to 6.
pub fn validate_rolls(rolls: &str) -> Result<usize> {
    let mut count = 0;
    for c in rolls.chars().filter(|c| !c.is_whitespace() && *c != ',') {
        count += 1;
        if !('1'..='6').contains(&c) {
            return Err(Error::Dice(format!(
                "roll #{count} of this line is '{c}', but only the digits 1 to 6 are valid"
            )));
        }
    }
    Ok(count)
}

/// Turn dice rolls into entropy.
///
/// * `mix_os_entropy = false` ("dice only"): `SHA256(rolls)`, truncated. This is
///   the exact method SeedSigner uses, so the same rolls give the same seed.
/// * `mix_os_entropy = true` (default in the apps): `SHA256(rolls || 32 random
///   bytes)`. The result is safe as long as *either* the dice *or* the
///   computer's RNG is good, but it can no longer be reproduced from dice alone.
///
/// `rolls` may contain whitespace and commas, which are ignored. Every other
/// character must be `1`..`6`.
pub fn from_dice(rolls: &str, entropy_len: usize, mix_os_entropy: bool) -> Result<Zeroizing<Vec<u8>>> {
    check_len(entropy_len)?;
    let cleaned: Zeroizing<String> = Zeroizing::new(
        rolls
            .chars()
            .filter(|c| !c.is_whitespace() && *c != ',')
            .collect(),
    );
    validate_rolls(&cleaned)?;
    let need = min_dice_rolls(entropy_len, !mix_os_entropy);
    if cleaned.len() < need {
        return Err(Error::Dice(format!(
            "{} rolls entered, but at least {need} are needed",
            cleaned.len()
        )));
    }

    let mut data = Zeroizing::new(cleaned.as_bytes().to_vec());
    if mix_os_entropy {
        let mut extra = Zeroizing::new([0u8; 32]);
        random_bytes(&mut *extra)?;
        data.extend_from_slice(&*extra);
    }
    let digest = sha256::Hash::hash(&data);
    Ok(Zeroizing::new(digest.to_byte_array()[..entropy_len].to_vec()))
}

/// Bits of entropy contributed by each word of [`generate_password`].
pub const PASSWORD_BITS_PER_WORD: usize = 11;

/// A random, easy-to-type password made of `words` BIP39 words joined by `-`,
/// e.g. `bridge-october-amazing-...`. Eight words give 88 bits of entropy.
///
/// Meant to be stored in a password manager. It is *not* a seed phrase.
pub fn generate_password(words: usize) -> Result<Zeroizing<String>> {
    let list = crate::mnemonic::LANGUAGE.word_list();
    let mut raw = Zeroizing::new(vec![0u8; words * 2]);
    random_bytes(&mut raw)?;
    // The word list has exactly 2048 = 2^11 entries, so masking is unbiased.
    let picked: Vec<&str> = raw
        .chunks_exact(2)
        .map(|c| list[(u16::from_le_bytes([c[0], c[1]]) & 0x07FF) as usize])
        .collect();
    Ok(Zeroizing::new(picked.join("-")))
}

fn check_len(n: usize) -> Result<()> {
    if matches!(n, 16 | 20 | 24 | 28 | 32) {
        Ok(())
    } else {
        Err(Error::Internal(format!("invalid entropy length {n}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_entropy_has_requested_length_and_varies() {
        let a = generate(32).unwrap();
        let b = generate(32).unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(*a, *b);
        assert!(generate(17).is_err());
    }

    #[test]
    fn dice_only_matches_seedsigner_method() {
        // SeedSigner: sha256(roll_string), first 16 bytes for 12 words.
        let rolls = "1".repeat(50);
        let got = from_dice(&rolls, 16, false).unwrap();
        let expected = sha256::Hash::hash(rolls.as_bytes()).to_byte_array();
        assert_eq!(&*got, &expected[..16]);
    }

    #[test]
    fn dice_validation() {
        assert!(matches!(from_dice("123", 16, false), Err(Error::Dice(_))));
        let bad = format!("{}7", "1".repeat(60));
        assert!(matches!(from_dice(&bad, 16, false), Err(Error::Dice(_))));
        // whitespace and commas are ignored
        let spaced = "1 2, 3 4 5 6 ".repeat(9);
        assert!(from_dice(&spaced, 16, false).is_ok());
    }

    #[test]
    fn number_of_rolls_needed() {
        assert_eq!(min_dice_rolls(16, false), 50); // 12 words
        assert_eq!(min_dice_rolls(32, false), 100); // 24 words
        assert_eq!(min_dice_rolls(16, true), 50);
        assert_eq!(min_dice_rolls(32, true), 99); // SeedSigner's number, dice-only mode
        // 99 rolls are enough in dice-only mode (SeedSigner) but not in the default mode
        assert!(from_dice(&"3".repeat(99), 32, false).is_ok());
        assert!(from_dice(&"3".repeat(99), 32, true).is_err());
        assert!(from_dice(&"3".repeat(100), 32, true).is_ok());
    }

    #[test]
    fn rolls_are_counted_and_validated_line_by_line() {
        assert_eq!(validate_rolls("").unwrap(), 0);
        assert_eq!(validate_rolls("3 5 1, 6 2\t2").unwrap(), 6);
        assert_eq!(validate_rolls("123456123456").unwrap(), 12);
        match validate_rolls("12 7 3") {
            Err(Error::Dice(m)) => assert!(m.contains("#3") && m.contains('7'), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(validate_rolls("abc").is_err());
        assert!(validate_rolls("0").is_err());
    }

    #[test]
    fn mixing_makes_dice_output_unique() {
        let rolls = "3".repeat(100);
        let a = from_dice(&rolls, 32, true).unwrap();
        let b = from_dice(&rolls, 32, true).unwrap();
        assert_ne!(*a, *b);
    }

    #[test]
    fn password_generator_shape() {
        let p = generate_password(8).unwrap();
        assert_eq!(p.split('-').count(), 8);
        for w in p.split('-') {
            assert!(crate::mnemonic::LANGUAGE.find_word(w).is_some());
        }
    }
}
