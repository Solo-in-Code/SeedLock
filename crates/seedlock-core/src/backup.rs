//! Password-encrypted mnemonic backups.
//!
//! A backup is a small, self-describing blob that is safe to keep in the cloud
//! or on a USB stick: without the password it is useless, and any change to it
//! is detected. The byte-level specification lives in `docs/FORMAT.md`.
//!
//! ```text
//! container (100 bytes)
//!   magic "SLK" | version=1 | kdf=1 (Argon2id v1.3) | m_cost_kib u32le | t u8 | p u8
//!   | salt[16] | nonce[24] | ciphertext[33 + 16-byte tag]
//! text form
//!   "SEEDLOCK1:" + BASE32( container || SHA256(container)[..4] )
//! ```
//!
//! * key = Argon2id(NFKD(password), salt, m, t, p) -> 32 bytes
//! * cipher = XChaCha20-Poly1305, the whole header is authenticated (AAD)
//! * plaintext = `len | entropy | zero padding`, always 33 bytes, so a 12-word
//!   backup is indistinguishable in size from a 24-word one

use crate::{Error, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use bip39::Mnemonic;
use bitcoin::hashes::{sha256, Hash};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use data_encoding::BASE32_NOPAD;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

/// Prefix of the text representation of a backup.
pub const TEXT_PREFIX: &str = "SEEDLOCK1:";
/// Binary format version written by this crate.
pub const FORMAT_VERSION: u8 = 1;
/// Shortest password (in characters) accepted when *creating* a backup.
pub const MIN_PASSWORD_CHARS: usize = 8;

const MAGIC: &[u8; 3] = b"SLK";
const KDF_ARGON2ID_V13: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const PAYLOAD_LEN: usize = 33;
const HEADER_LEN: usize = 3 + 1 + 1 + 4 + 1 + 1 + SALT_LEN + NONCE_LEN; // 51
const CONTAINER_LEN: usize = HEADER_LEN + PAYLOAD_LEN + TAG_LEN; // 100
const CHECKSUM_LEN: usize = 4;

/// Argon2id cost parameters. They are stored inside every backup, so backups
/// made with different settings can always be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_cost_kib: u32,
    /// Number of passes.
    pub t_cost: u8,
    /// Lanes / parallelism.
    pub parallelism: u8,
}

impl KdfParams {
    /// 64 MiB: for old phones and very small machines.
    pub const LIGHT: KdfParams = KdfParams { m_cost_kib: 64 * 1024, t_cost: 3, parallelism: 4 };
    /// 256 MiB: the default. Takes about a second on a modern computer.
    pub const STANDARD: KdfParams = KdfParams { m_cost_kib: 256 * 1024, t_cost: 3, parallelism: 4 };
    /// 1 GiB: for desktop machines with plenty of RAM.
    pub const STRONG: KdfParams = KdfParams { m_cost_kib: 1024 * 1024, t_cost: 3, parallelism: 4 };

    const MAX_M_COST_KIB: u32 = 2 * 1024 * 1024; // 2 GiB; protects against hostile files
    const MAX_T_COST: u8 = 32;
    const MAX_PARALLELISM: u8 = 16;

    /// Reject values that Argon2 cannot use or that would be an abuse (e.g. a
    /// tampered file asking for 100 GiB of memory).
    pub fn validate(&self) -> Result<()> {
        let ok = (1..=Self::MAX_PARALLELISM).contains(&self.parallelism)
            && (1..=Self::MAX_T_COST).contains(&self.t_cost)
            && self.m_cost_kib >= 8 * u32::from(self.parallelism)
            && self.m_cost_kib <= Self::MAX_M_COST_KIB;
        if ok {
            Ok(())
        } else {
            Err(Error::KdfParams)
        }
    }

    /// Memory in MiB (rounded down).
    pub fn memory_mib(&self) -> u32 {
        self.m_cost_kib / 1024
    }
}

/// What can be read from a backup without knowing the password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    /// Format version.
    pub version: u8,
    /// Key-derivation settings that will be needed to open it.
    pub kdf: KdfParams,
}

// ---------------------------------------------------------------------------
// High-level API
// ---------------------------------------------------------------------------

/// Encrypt a mnemonic and return the text form (`SEEDLOCK1:...`).
pub fn encrypt_mnemonic(mnemonic: &Mnemonic, password: &str, params: KdfParams) -> Result<String> {
    let entropy = crate::mnemonic::entropy(mnemonic);
    let container = encrypt(&entropy, password, params)?;
    Ok(to_text(&container))
}

/// Decrypt the text form of a backup (or the full contents of a backup file).
pub fn decrypt_mnemonic(text: &str, password: &str) -> Result<Mnemonic> {
    let container = from_text(text)?;
    let entropy = decrypt(&container, password)?;
    crate::mnemonic::from_entropy(&entropy)
}

/// Read the version and KDF settings of a backup without a password.
pub fn inspect_text(text: &str) -> Result<BackupInfo> {
    inspect(&from_text(text)?)
}

// ---------------------------------------------------------------------------
// Binary container
// ---------------------------------------------------------------------------

/// Encrypt raw entropy (16-32 bytes) into a binary container with a fresh
/// random salt and nonce.
pub fn encrypt(entropy: &[u8], password: &str, params: KdfParams) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    crate::entropy::random_bytes(&mut salt)?;
    crate::entropy::random_bytes(&mut nonce)?;
    seal(entropy, password, params, salt, nonce)
}

/// Deterministic core of [`encrypt`]; separate so tests can use known salt/nonce.
pub(crate) fn seal(
    entropy: &[u8],
    password: &str,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(Error::PasswordTooShort(MIN_PASSWORD_CHARS));
    }
    if !matches!(entropy.len(), 16 | 20 | 24 | 28 | 32) {
        return Err(Error::Internal(format!("invalid entropy length {}", entropy.len())));
    }
    params.validate()?;

    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.push(FORMAT_VERSION);
    header.push(KDF_ARGON2ID_V13);
    header.extend_from_slice(&params.m_cost_kib.to_le_bytes());
    header.push(params.t_cost);
    header.push(params.parallelism);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    debug_assert_eq!(header.len(), HEADER_LEN);

    let mut payload = Zeroizing::new([0u8; PAYLOAD_LEN]);
    payload[0] = entropy.len() as u8;
    payload[1..1 + entropy.len()].copy_from_slice(entropy);

    let key = derive_key(password, &salt, params)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| Error::Internal("bad key length".into()))?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload { msg: &payload[..], aad: &header },
        )
        .map_err(|_| Error::Internal("encryption failed".into()))?;

    let mut out = header;
    out.extend_from_slice(&ciphertext);
    debug_assert_eq!(out.len(), CONTAINER_LEN);
    Ok(out)
}

/// Decrypt a binary container and return the raw entropy.
pub fn decrypt(container: &[u8], password: &str) -> Result<Zeroizing<Vec<u8>>> {
    let (info, salt, nonce) = parse_header(container)?;
    let key = derive_key(password, &salt, info.kdf)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| Error::Internal("bad key length".into()))?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload { msg: &container[HEADER_LEN..], aad: &container[..HEADER_LEN] },
            )
            .map_err(|_| Error::WrongPassword)?,
    );

    // The AEAD already proved authenticity; these checks guard the payload layout.
    if plaintext.len() != PAYLOAD_LEN {
        return Err(Error::NotABackup);
    }
    let n = plaintext[0] as usize;
    if !matches!(n, 16 | 20 | 24 | 28 | 32) || plaintext[1 + n..].iter().any(|b| *b != 0) {
        return Err(Error::NotABackup);
    }
    Ok(Zeroizing::new(plaintext[1..1 + n].to_vec()))
}

/// Parse and validate the header of a container.
pub fn inspect(container: &[u8]) -> Result<BackupInfo> {
    parse_header(container).map(|(info, _, _)| info)
}

fn parse_header(container: &[u8]) -> Result<(BackupInfo, [u8; SALT_LEN], [u8; NONCE_LEN])> {
    if container.len() < 4 || &container[..3] != MAGIC {
        return Err(Error::NotABackup);
    }
    let version = container[3];
    if version != FORMAT_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    if container.len() != CONTAINER_LEN {
        return Err(Error::NotABackup);
    }
    let kdf_id = container[4];
    if kdf_id != KDF_ARGON2ID_V13 {
        return Err(Error::UnsupportedKdf(kdf_id));
    }
    let kdf = KdfParams {
        m_cost_kib: u32::from_le_bytes([container[5], container[6], container[7], container[8]]),
        t_cost: container[9],
        parallelism: container[10],
    };
    kdf.validate()?;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&container[11..11 + SALT_LEN]);
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&container[11 + SALT_LEN..HEADER_LEN]);
    Ok((BackupInfo { version, kdf }, salt, nonce))
}

/// Argon2id(NFKD(password)) -> 32-byte key.
///
/// The password is Unicode-normalised (NFKD, as BIP39 does for passphrases) so
/// that the same password typed on a Mac, a Windows PC or a phone keyboard
/// always produces the same key.
fn derive_key(password: &str, salt: &[u8], kdf: KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    kdf.validate()?;
    let normalized: Zeroizing<String> = Zeroizing::new(password.nfkd().collect());
    let params = Params::new(kdf.m_cost_kib, u32::from(kdf.t_cost), u32::from(kdf.parallelism), Some(32))
        .map_err(|e| Error::Internal(format!("argon2 parameters: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(normalized.as_bytes(), salt, &mut *key)
        .map_err(|e| Error::Internal(format!("argon2: {e}")))?;
    Ok(key)
}

// ---------------------------------------------------------------------------
// Text form
// ---------------------------------------------------------------------------

/// `SEEDLOCK1:` + Base32(container || 4-byte checksum). Upper-case Base32 lets
/// QR codes use the compact alphanumeric mode.
pub fn to_text(container: &[u8]) -> String {
    let digest = sha256::Hash::hash(container).to_byte_array();
    let mut data = container.to_vec();
    data.extend_from_slice(&digest[..CHECKSUM_LEN]);
    format!("{TEXT_PREFIX}{}", BASE32_NOPAD.encode(&data))
}

/// Parse the text form. Accepts a whole backup file: `#` comment lines and
/// blank lines are skipped; spaces, dashes, line breaks and lower case inside
/// the data are tolerated so a hand-copied backup still works.
pub fn from_text(text: &str) -> Result<Vec<u8>> {
    let joined: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("");
    let cleaned: String = joined
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    let body = cleaned.strip_prefix(TEXT_PREFIX).ok_or(Error::NotABackup)?;
    let data = BASE32_NOPAD
        .decode(body.as_bytes())
        .map_err(|_| Error::BackupChecksum)?;
    if data.len() <= CHECKSUM_LEN {
        return Err(Error::BackupChecksum);
    }
    let (container, check) = data.split_at(data.len() - CHECKSUM_LEN);
    let digest = sha256::Hash::hash(container).to_byte_array();
    if digest[..CHECKSUM_LEN] != *check {
        return Err(Error::BackupChecksum);
    }
    Ok(container.to_vec())
}

/// The contents to write into a `.seedlock` file: two comment lines that
/// explain what the file is, then the backup text.
pub fn to_file_contents(container: &[u8]) -> String {
    format!(
        "# SeedLock encrypted seed backup (format v{FORMAT_VERSION})\n\
         # Argon2id + XChaCha20-Poly1305. Useless without its password.\n\
         {}\n",
        to_text(container)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap parameters so the tests run quickly.
    const FAST: KdfParams = KdfParams { m_cost_kib: 256, t_cost: 1, parallelism: 1 };
    const PW: &str = "correct-horse-battery-staple";

    #[test]
    fn roundtrip_for_every_entropy_size() {
        for len in [16usize, 20, 24, 28, 32] {
            let entropy: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(7)).collect();
            let container = encrypt(&entropy, PW, FAST).unwrap();
            assert_eq!(container.len(), CONTAINER_LEN, "size must not depend on word count");
            assert_eq!(&*decrypt(&container, PW).unwrap(), &entropy[..]);
        }
    }

    /// Known-answer test. The expected text was produced by an independent
    /// implementation (Python: argon2-cffi + PyNaCl), see `docs/FORMAT.md`.
    /// If this ever fails, the on-disk format has changed and old backups
    /// would no longer open: never "fix" it by editing the constant.
    #[test]
    fn known_answer_matches_independent_implementation() {
        const EXPECTED: &str = "SEEDLOCK1:KNGEWAIBAAAQAAABAEIBCEQTCQKRMFYYDENBWHA5DYPSAIJCEMSCKJRHFAUSUKZMFUXC6MBRGIZTINJWG74EM5MYIBOQAXY5PNR2F3S4BVA3F2QPGIKPZGVH2UVQ57VYCGYH6BB7JYQLXMSL5Q2OQKODKWGCWERO6WKGI5Y";
        let entropy: Vec<u8> = (0u8..16).collect();
        let salt: [u8; SALT_LEN] = core::array::from_fn(|i| 0x10 + i as u8);
        let nonce: [u8; NONCE_LEN] = core::array::from_fn(|i| 0x20 + i as u8);
        let container = seal(&entropy, PW, FAST, salt, nonce).unwrap();
        assert_eq!(to_text(&container), EXPECTED);
        assert_eq!(&*decrypt(&from_text(EXPECTED).unwrap(), PW).unwrap(), &entropy[..]);
    }

    #[test]
    fn wrong_password_is_rejected() {
        let c = encrypt(&[9u8; 16], PW, FAST).unwrap();
        assert!(matches!(decrypt(&c, "not-the-password"), Err(Error::WrongPassword)));
    }

    #[test]
    fn same_input_gives_different_ciphertext() {
        let a = encrypt(&[9u8; 16], PW, FAST).unwrap();
        let b = encrypt(&[9u8; 16], PW, FAST).unwrap();
        assert_ne!(a, b, "salt and nonce must be random");
    }

    #[test]
    fn any_single_bit_flip_is_detected() {
        let c = encrypt(&[7u8; 24], PW, FAST).unwrap();
        for i in 0..c.len() {
            let mut bad = c.clone();
            bad[i] ^= 0x01;
            assert!(decrypt(&bad, PW).is_err(), "bit flip in byte {i} went unnoticed");
        }
    }

    #[test]
    fn truncated_or_extended_containers_are_rejected() {
        let c = encrypt(&[7u8; 16], PW, FAST).unwrap();
        assert!(decrypt(&c[..c.len() - 1], PW).is_err());
        let mut longer = c.clone();
        longer.push(0);
        assert!(decrypt(&longer, PW).is_err());
        assert!(matches!(decrypt(b"nope", PW), Err(Error::NotABackup)));
    }

    #[test]
    fn password_is_unicode_normalised() {
        let composed = "caf\u{e9}-password"; // é as one code point
        let decomposed = "cafe\u{301}-password"; // e + combining accent
        let c = encrypt(&[1u8; 16], composed, FAST).unwrap();
        assert!(decrypt(&c, decomposed).is_ok());
    }

    #[test]
    fn short_passwords_are_refused() {
        assert!(matches!(
            encrypt(&[1u8; 16], "short", FAST),
            Err(Error::PasswordTooShort(_))
        ));
    }

    #[test]
    fn kdf_bounds_are_enforced() {
        let huge = KdfParams { m_cost_kib: u32::MAX, t_cost: 1, parallelism: 1 };
        assert!(huge.validate().is_err());
        let zero_t = KdfParams { m_cost_kib: 1024, t_cost: 0, parallelism: 1 };
        assert!(zero_t.validate().is_err());
        assert!(KdfParams::STANDARD.validate().is_ok());
        assert!(KdfParams::LIGHT.validate().is_ok());
        assert!(KdfParams::STRONG.validate().is_ok());
    }

    #[test]
    fn text_form_roundtrip_and_tolerance() {
        let c = encrypt(&[5u8; 32], PW, FAST).unwrap();
        let text = to_text(&c);
        assert!(text.starts_with(TEXT_PREFIX));
        assert_eq!(from_text(&text).unwrap(), c);

        // lower case, dashes, spaces and wrapped lines
        let body = &text[TEXT_PREFIX.len()..];
        let messy = format!(
            "# a comment\n\n{}{}\n {}\n",
            TEXT_PREFIX.to_lowercase(),
            body[..40].to_lowercase(),
            body[40..].chars().collect::<Vec<_>>().chunks(5).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
        );
        assert_eq!(from_text(&messy).unwrap(), c);

        // file contents helper produces something from_text accepts
        assert_eq!(from_text(&to_file_contents(&c)).unwrap(), c);
    }

    #[test]
    fn typos_in_text_form_are_caught_before_key_derivation() {
        let c = encrypt(&[5u8; 16], PW, FAST).unwrap();
        let text = to_text(&c);
        let mut chars: Vec<char> = text.chars().collect();
        let i = TEXT_PREFIX.len() + 20;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert!(matches!(from_text(&typo), Err(Error::BackupChecksum)));
        assert!(matches!(from_text("hello world"), Err(Error::NotABackup)));
        assert!(matches!(from_text(""), Err(Error::NotABackup)));
    }

    #[test]
    fn inspect_reports_settings_without_password() {
        let c = encrypt(&[5u8; 16], PW, FAST).unwrap();
        let info = inspect(&c).unwrap();
        assert_eq!(info.version, FORMAT_VERSION);
        assert_eq!(info.kdf, FAST);
        assert!(matches!(inspect(&[]), Err(Error::NotABackup)));
    }

    #[test]
    fn mnemonic_level_roundtrip() {
        let m = crate::mnemonic::parse(
            "legal winner thank year wave sausage worth useful legal winner thank yellow",
        )
        .unwrap();
        let text = encrypt_mnemonic(&m, PW, FAST).unwrap();
        let back = decrypt_mnemonic(&text, PW).unwrap();
        assert_eq!(&*crate::mnemonic::phrase(&back), &*crate::mnemonic::phrase(&m));
    }
}
