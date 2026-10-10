//! BIP39 mnemonic helpers.
//!
//! The parser is deliberately forgiving to make typing a seed easier, in the
//! same spirit as SeedSigner's on-screen keyboard:
//!
//! * case and extra whitespace are ignored;
//! * any unambiguous prefix of 3+ letters is expanded (`aban` -> `abandon`);
//! * unknown words come with "did you mean" suggestions.
//!
//! Only the English word list is supported (like SeedSigner and most wallets).

use crate::{Error, Result};
use bip39::{Language, Mnemonic};
use zeroize::{Zeroize, Zeroizing};

/// The word list used everywhere in SeedLock.
pub const LANGUAGE: Language = Language::English;

/// Valid mnemonic lengths, in words.
pub const WORD_COUNTS: [usize; 5] = [12, 15, 18, 21, 24];

/// Parse a mnemonic typed or pasted by a person.
///
/// Returns a precise error (which word, and suggestions) when something is off.
pub fn parse(input: &str) -> Result<Mnemonic> {
    let typed: Zeroizing<Vec<String>> = Zeroizing::new(
        input
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect(),
    );
    if !WORD_COUNTS.contains(&typed.len()) {
        return Err(Error::BadWordCount(typed.len()));
    }
    let joined = Zeroizing::new(complete_words(&typed)?.join(" "));
    Mnemonic::parse_in_normalized(LANGUAGE, &joined).map_err(|e| match e {
        bip39::Error::InvalidChecksum => Error::BadChecksum,
        bip39::Error::BadWordCount(n) => Error::BadWordCount(n),
        other => Error::Internal(other.to_string()),
    })
}

/// Build a mnemonic from raw entropy (16, 20, 24, 28 or 32 bytes).
pub fn from_entropy(entropy: &[u8]) -> Result<Mnemonic> {
    Mnemonic::from_entropy_in(LANGUAGE, entropy).map_err(|e| Error::Internal(e.to_string()))
}

/// The raw entropy behind a mnemonic (the checksum bits are not included).
pub fn entropy(m: &Mnemonic) -> Zeroizing<Vec<u8>> {
    let (mut buf, len) = m.to_entropy_array();
    let out = Zeroizing::new(buf[..len].to_vec());
    buf.zeroize();
    out
}

/// The mnemonic as a single space-separated string.
pub fn phrase(m: &Mnemonic) -> Zeroizing<String> {
    Zeroizing::new(m.words().collect::<Vec<_>>().join(" "))
}

/// All valid final words for a mnemonic whose last word is still missing.
///
/// `leading` must contain 11, 14, 17, 20 or 23 words. For 11 words there are
/// exactly 128 valid choices (the last word carries the checksum bits).
pub fn final_word_candidates(leading: &str) -> Result<Vec<&'static str>> {
    let typed: Zeroizing<Vec<String>> = Zeroizing::new(
        leading
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect(),
    );
    if !matches!(typed.len(), 11 | 14 | 17 | 20 | 23) {
        return Err(Error::BadWordCount(typed.len()));
    }
    let base = Zeroizing::new(complete_words(&typed)?.join(" "));
    let mut out = Vec::new();
    for candidate in LANGUAGE.word_list().iter() {
        let attempt = Zeroizing::new(format!("{} {}", &*base, candidate));
        if Mnemonic::parse_in_normalized(LANGUAGE, &attempt).is_ok() {
            out.push(*candidate);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------

fn complete_words(typed: &[String]) -> Result<Vec<&'static str>> {
    typed
        .iter()
        .enumerate()
        .map(|(i, w)| {
            complete_word(w).map_err(|hint| Error::UnknownWord {
                position: i + 1,
                word: w.clone(),
                hint,
            })
        })
        .collect()
}

/// Exact word, or unambiguous prefix (3+ letters), or an error hint.
fn complete_word(w: &str) -> std::result::Result<&'static str, String> {
    if let Some(idx) = LANGUAGE.find_word(w) {
        return Ok(LANGUAGE.word_list()[idx as usize]);
    }
    if w.len() >= 3 {
        let matches = LANGUAGE.words_by_prefix(w);
        if matches.len() == 1 {
            return Ok(matches[0]);
        }
        if matches.len() > 1 {
            let list: Vec<&str> = matches.iter().take(6).copied().collect();
            return Err(format!(" (too short to be unique, could be: {})", list.join(", ")));
        }
    }
    Err(typo_hint(w))
}

fn typo_hint(word: &str) -> String {
    let len = word.chars().count();
    let mut scored: Vec<(usize, &'static str)> = LANGUAGE
        .word_list()
        .iter()
        .map(|w| (edit_distance(word, w), *w))
        .filter(|(d, _)| *d <= 2 && *d < len)
        .collect();
    scored.sort();
    scored.truncate(4);
    if scored.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = scored.iter().map(|(_, w)| *w).collect();
        format!(" (did you mean: {}?)", names.join(", "))
    }
}

/// Plain Levenshtein distance; the inputs are tiny.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABANDON: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn parses_standard_vector() {
        let m = parse(ABANDON).unwrap();
        assert_eq!(&*phrase(&m), ABANDON);
        assert_eq!(&*entropy(&m), &[0u8; 16][..]);
    }

    #[test]
    fn ignores_case_and_whitespace() {
        let messy = "  ABANDON abandon\tabandon abandon\nabandon abandon abandon abandon abandon abandon abandon About ";
        assert_eq!(&*phrase(&parse(messy).unwrap()), ABANDON);
    }

    #[test]
    fn expands_unique_prefixes() {
        let short = "aban aban aban aban aban aban aban aban aban aban aban abou";
        assert_eq!(&*phrase(&parse(short).unwrap()), ABANDON);
    }

    #[test]
    fn reports_bad_word_with_hint() {
        let bad = ABANDON.replacen("abandon", "abandn", 1);
        match parse(&bad) {
            Err(Error::UnknownWord { position, hint, .. }) => {
                assert_eq!(position, 1);
                assert!(hint.contains("abandon"), "hint was {hint:?}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_count_and_checksum() {
        assert!(matches!(parse("abandon abandon"), Err(Error::BadWordCount(2))));
        let wrong = ABANDON.replace("about", "abandon");
        assert!(matches!(parse(&wrong), Err(Error::BadChecksum)));
    }

    #[test]
    fn entropy_roundtrip_all_sizes() {
        for len in [16usize, 20, 24, 28, 32] {
            let raw: Vec<u8> = (0..len as u8).collect();
            let m = from_entropy(&raw).unwrap();
            assert_eq!(&*entropy(&m), &raw[..]);
            assert_eq!(m.word_count(), len * 3 / 4);
            assert_eq!(&*entropy(&parse(&phrase(&m)).unwrap()), &raw[..]);
        }
    }

    #[test]
    fn final_word_calculator() {
        let leading = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        let c = final_word_candidates(leading).unwrap();
        assert_eq!(c.len(), 128);
        assert!(c.contains(&"about"));
        assert!(matches!(
            final_word_candidates("abandon abandon"),
            Err(Error::BadWordCount(2))
        ));
    }
}
