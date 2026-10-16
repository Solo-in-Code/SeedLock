//! Seeds and backups: opening a backup, making a new seed, taking the words of one, and encrypting a
//! seed into a backup; also the helpers for typing words and dice rolls, and for naming backups.

use crate::error::{failed, secret_text, Result};
use crate::wallet::{Net, Wallet};
use seedlock_core::bitcoin::Network;
use seedlock_core::wallet::Wallet as CoreWallet;
use seedlock_core::{backup, entropy, mnemonic, Mnemonic};
use std::sync::Arc;
use zeroize::Zeroizing;

/// How much memory Argon2id uses to stretch a backup's password, which decides how hard the password
/// is to guess. The setting is stored inside the backup, so either kind opens everywhere.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupStrength {
    /// 64 MiB: for old phones with little memory.
    Light,
    /// 256 MiB: the default, the same as the command-line program's.
    Standard,
}

impl BackupStrength {
    fn params(self) -> backup::KdfParams {
        match self {
            BackupStrength::Light => backup::KdfParams::LIGHT,
            BackupStrength::Standard => backup::KdfParams::STANDARD,
        }
    }
}

/// What can be read from a backup without its password.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    /// The backup format's version.
    pub version: i32,
    /// The memory (MiB) that opening it takes; opening takes a few seconds on a phone.
    pub memory_mib: i32,
}

/// A newly encrypted backup.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct NewBackup {
    /// What goes into the backup's file (`<name>.seedlock`): two comment lines, then the backup line.
    /// The same as the command-line program writes, so the file opens there too.
    pub file_contents: String,
    /// The backup line alone (`SEEDLOCK1:...`), for its QR code.
    pub text: String,
}

/// A seed (what its words stand for), held while the app needs it. The memory is wiped when the app
/// lets go of the object (`close()`), or at the latest when it is garbage collected.
#[derive(uniffi::Object)]
pub struct Seed {
    entropy: Zeroizing<Vec<u8>>,
}

/// Never shows what is inside (an error message or a log must not reveal a seed).
impl std::fmt::Debug for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Seed { .. }")
    }
}

fn entropy_len(words: i32) -> Result<usize> {
    match usize::try_from(words).ok().and_then(entropy::entropy_len_for_words) {
        Some(len) => Ok(len),
        None => failed("a seed has 12, 15, 18, 21 or 24 words"),
    }
}

impl Seed {
    fn mnemonic(&self) -> Result<Mnemonic> {
        Ok(mnemonic::from_entropy(&self.entropy)?)
    }

    fn of(m: &Mnemonic) -> Arc<Seed> {
        Arc::new(Seed { entropy: mnemonic::entropy(m) })
    }
}

#[uniffi::export]
impl Seed {
    /// A new seed from the phone's own randomness: 12 or 24 words (15, 18 and 21 work too).
    #[uniffi::constructor]
    pub fn generate(words: i32) -> Result<Arc<Seed>> {
        Ok(Arc::new(Seed { entropy: entropy::generate(entropy_len(words)?)? }))
    }

    /// A new seed from dice rolls (the digits 1 to 6; spaces and commas are ignored). The rolls are mixed
    /// with the phone's randomness, unless `dice_only`: then the same rolls always give the same seed,
    /// exactly as on SeedSigner. [`dice_rolls_needed`] says how many rolls it takes.
    #[uniffi::constructor]
    pub fn from_dice(rolls: String, words: i32, dice_only: bool) -> Result<Arc<Seed>> {
        let rolls = Zeroizing::new(rolls);
        Ok(Arc::new(Seed { entropy: entropy::from_dice(&rolls, entropy_len(words)?, !dice_only)? }))
    }

    /// The seed whose words were typed (12 to 24 words, separated by spaces). A word may be shortened
    /// to its first four letters. The error says which word is wrong.
    #[uniffi::constructor]
    pub fn from_words(words: String) -> Result<Arc<Seed>> {
        let words = Zeroizing::new(words);
        Ok(Seed::of(&mnemonic::parse(&words)?))
    }

    /// Open a backup (its `SEEDLOCK1:...` line, or a whole backup file) with its password, given as its
    /// UTF-8 bytes. A wrong password gives `SeedLockException.WrongPassword`. Takes a few seconds and
    /// up to 256 MiB of memory (Argon2id): call it away from the screen's thread.
    #[uniffi::constructor]
    pub fn unlock(backup_text: String, password: Vec<u8>) -> Result<Arc<Seed>> {
        let password = secret_text(password)?;
        Ok(Seed::of(&backup::decrypt_mnemonic(&backup_text, &password)?))
    }

    /// The wallet's fingerprint (8 hex digits) without a BIP39 passphrase: a short name that tells
    /// wallets apart, and that Sparrow shows too.
    pub fn fingerprint(&self) -> Result<String> {
        Ok(CoreWallet::new(&self.mnemonic()?, "", Network::Bitcoin)?.fingerprint().to_string())
    }

    /// How many words the seed has.
    pub fn word_count(&self) -> i32 {
        (self.entropy.len() * 3 / 4) as i32
    }

    /// The words, in order. Only to show them, with nobody looking.
    pub fn words(&self) -> Result<Vec<String>> {
        Ok(self.mnemonic()?.words().map(str::to_string).collect())
    }

    /// Whether these typed words are this seed (to compare a backup with the paper it was written on).
    /// Words that are not a seed at all give an error saying what is wrong.
    pub fn matches_words(&self, words: String) -> Result<bool> {
        let words = Zeroizing::new(words);
        let typed = mnemonic::parse(&words)?;
        Ok(*mnemonic::entropy(&typed) == *self.entropy)
    }

    /// Whether `other` is the same seed (for example the backup that was just saved, opened again).
    pub fn same_as(&self, other: Arc<Seed>) -> bool {
        *self.entropy == *other.entropy
    }

    /// Encrypt the seed into a backup with this password (its UTF-8 bytes; at least
    /// [`min_password_length`] characters). Takes a few seconds: away from the screen's thread.
    pub fn encrypt(&self, password: Vec<u8>, strength: BackupStrength) -> Result<NewBackup> {
        let password = secret_text(password)?;
        let container = backup::encrypt(&self.entropy, &password, strength.params())?;
        Ok(NewBackup { file_contents: backup::to_file_contents(&container), text: backup::to_text(&container) })
    }

    /// The wallet of this seed, with a BIP39 passphrase (its UTF-8 bytes; empty for none) and for a network.
    /// With a passphrase it is a different wallet from the one without it.
    pub fn wallet(&self, passphrase: Vec<u8>, network: Net) -> Result<Arc<Wallet>> {
        let passphrase = secret_text(passphrase)?;
        let wallet = CoreWallet::new(&self.mnemonic()?, &passphrase, network.network())?;
        Ok(Arc::new(Wallet::new(wallet, !passphrase.is_empty())))
    }
}

/// What a backup's text says without its password: its version and the memory opening it takes. An
/// error says when the text is not a backup, or is damaged (its checksum does not fit).
#[uniffi::export]
pub fn backup_info(text: String) -> Result<BackupInfo> {
    let info = backup::inspect_text(&text)?;
    Ok(BackupInfo { version: i32::from(info.version), memory_mib: info.kdf.memory_mib() as i32 })
}

/// The backup line (`SEEDLOCK1:...`) of a backup's text or file, written the standard way: to show it
/// as a QR code, and to tell whether two backups are the same one.
#[uniffi::export]
pub fn backup_text(text: String) -> Result<String> {
    Ok(backup::to_text(&backup::from_text(&text)?))
}

/// What to write into a `.seedlock` file for this backup (the same as the command-line program writes).
#[uniffi::export]
pub fn backup_file_contents(text: String) -> Result<String> {
    Ok(backup::to_file_contents(&backup::from_text(&text)?))
}

/// The text of a backup's QR code: its backup line and, when `name` is not empty, its name on a second
/// line (`# name: ...`, see docs/FORMAT.md), so that the phone or computer that scans it can suggest the
/// name. `text` is a backup's line or file. Every SeedLock skips lines that start with `#` when it reads
/// a backup, so such a code reads as the same backup everywhere, also in the versions before names.
#[uniffi::export]
pub fn backup_qr_text(text: String, name: String) -> Result<String> {
    Ok(backup::qr_text(&text, &name)?)
}

/// The name a backup's text carries (a backup's QR code made by [`backup_qr_text`]), if it has one; none
/// for a code without one, such as those of the versions before names were added.
#[uniffi::export]
pub fn backup_name_in(text: String) -> Option<String> {
    backup::name_in(&text)
}

/// The name a backup is saved under, cleaned up from what was typed (spaces around it, quotes and a
/// typed ".seedlock" are removed), or an error that says what is wrong with it. The same rules as the
/// command-line program's, so that the files can be copied to a computer, a USB stick or a cloud folder.
#[uniffi::export]
pub fn check_backup_name(typed: String) -> Result<String> {
    check_file_name(&typed, "seedlock", "backup")
}

/// The command-line program's rules for the name of a file of kind `ext` (without the dot), which
/// holds a `what` ("backup"): cleaned up from what was typed, or an error that says what is wrong.
pub(crate) fn check_file_name(typed: &str, ext: &str, what: &str) -> Result<String> {
    let mut name = typed.trim().trim_matches('"').trim_matches('\'').trim().to_string();
    if name.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        name.truncate(name.len() - ext.len() - 1);
        name = name.trim().to_string();
    }
    if name.is_empty() {
        return failed(format!("Please type a name for the {what}."));
    }
    if let Some(bad) = name.chars().find(|c| c.is_control() || "/\\:*?\"<>|".contains(*c)) {
        return failed(format!("A {what} name cannot contain the character '{bad}'. Please choose another name."));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return failed(format!("A {what} name cannot start or end with a dot. Please choose another name."));
    }
    if name.chars().count() > 100 {
        return failed("That name is too long (100 characters at most). Please choose a shorter name.");
    }
    Ok(name)
}

/// The BIP39 words that start with what was typed so far (in lower case), to offer while a word is typed.
#[uniffi::export]
pub fn word_suggestions(prefix: String) -> Vec<String> {
    let prefix = prefix.trim().to_lowercase();
    if prefix.is_empty() {
        return Vec::new();
    }
    mnemonic::LANGUAGE.words_by_prefix(&prefix).iter().map(|w| w.to_string()).collect()
}

/// Every word that can complete a seed whose last word is still missing (11, 14, 17, 20 or 23 words
/// typed): the last word carries a checksum, so only some words fit.
#[uniffi::export]
pub fn final_words(leading_words: String) -> Result<Vec<String>> {
    let leading = Zeroizing::new(leading_words);
    Ok(mnemonic::final_word_candidates(&leading)?.into_iter().map(str::to_string).collect())
}

/// How many dice rolls a new seed of `words` words needs: 50 for 12 words, 100 for 24 (99 in
/// dice-only mode, as SeedSigner counts them).
#[uniffi::export]
pub fn dice_rolls_needed(words: i32, dice_only: bool) -> Result<i32> {
    Ok(entropy::min_dice_rolls(entropy_len(words)?, dice_only) as i32)
}

/// How many dice rolls this text holds (digits 1 to 6; spaces and commas are ignored), or an error that
/// says which character is not a roll.
#[uniffi::export]
pub fn count_dice_rolls(text: String) -> Result<i32> {
    let text = Zeroizing::new(text);
    Ok(entropy::validate_rolls(&text)? as i32)
}

/// A strong random password for a backup: 8 words joined by dashes (88 bits), to keep in a password manager.
#[uniffi::export]
pub fn generate_password() -> Result<String> {
    Ok(entropy::generate_password(8)?.to_string())
}

/// The shortest password a backup accepts, in characters.
#[uniffi::export]
pub fn min_password_length() -> i32 {
    backup::MIN_PASSWORD_CHARS as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SeedLockError;

    const WORDS: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn quick_backup(seed: &Seed, password: &str) -> String {
        let quick = backup::KdfParams { m_cost_kib: 64, t_cost: 1, parallelism: 1 };
        backup::to_file_contents(&backup::encrypt(&seed.entropy, password, quick).unwrap())
    }

    #[test]
    fn a_backup_opens_with_its_password_only() {
        let seed = Seed::from_words(WORDS.into()).unwrap();
        assert_eq!(seed.fingerprint().unwrap(), "73c5da0a"); // the BIP39 test vector's well-known fingerprint
        assert_eq!(seed.word_count(), 12);
        let file = quick_backup(&seed, "a good long password");
        let opened = Seed::unlock(file.clone(), b"a good long password".to_vec()).unwrap();
        assert!(opened.same_as(seed.clone()));
        assert_eq!(opened.words().unwrap().join(" "), WORDS);
        match Seed::unlock(file, b"not the password".to_vec()) {
            Err(SeedLockError::WrongPassword(text)) => assert!(text.contains("wrong password"), "{text}"),
            other => panic!("expected a wrong password, got {:?}", other.map(|_| ())),
        }
        assert!(matches!(Seed::unlock("SEEDLOCK1:ABC".into(), b"x".to_vec()), Err(SeedLockError::Failed(_))));
        assert!(matches!(Seed::unlock(WORDS.into(), vec![0xff, 0xfe]), Err(SeedLockError::Failed(_))), "bytes that are not text");
    }

    #[test]
    fn new_seeds_and_their_backups() {
        let a = Seed::generate(24).unwrap();
        let b = Seed::generate(24).unwrap();
        assert_eq!(a.word_count(), 24);
        assert!(!a.same_as(b), "two new seeds differ");
        assert!(Seed::generate(13).is_err());
        let backup = a.encrypt(b"correct horse battery".to_vec(), BackupStrength::Light).unwrap();
        assert!(backup.file_contents.starts_with("# SeedLock encrypted seed backup"));
        assert!(backup.file_contents.contains(&backup.text) && backup.text.starts_with("SEEDLOCK1:"));
        assert_eq!(backup_info(backup.text.clone()).unwrap(), BackupInfo { version: 1, memory_mib: 64 });
        assert_eq!(backup_text(backup.file_contents.clone()).unwrap(), backup.text);
        assert_eq!(backup_file_contents(backup.text.to_lowercase()).unwrap(), backup.file_contents);
        let reopened = Seed::unlock(backup.file_contents, b"correct horse battery".to_vec()).unwrap();
        assert!(reopened.same_as(a.clone()));
        let short = a.encrypt(b"short".to_vec(), BackupStrength::Light).map(|_| ()).unwrap_err();
        assert!(short.to_string().contains(&min_password_length().to_string()), "{short}");
    }

    #[test]
    fn dice_rolls_make_the_same_seed_as_seedsigner_in_dice_only_mode() {
        assert_eq!(dice_rolls_needed(12, false).unwrap(), 50);
        assert_eq!(dice_rolls_needed(24, true).unwrap(), 99);
        let rolls = "1234561234 5612345612,3456123456 1234561234 5612345612";
        assert_eq!(count_dice_rolls(rolls.into()).unwrap(), 50);
        assert!(count_dice_rolls("12x".into()).unwrap_err().to_string().contains("'x'"));
        let once = Seed::from_dice(rolls.into(), 12, true).unwrap();
        let again = Seed::from_dice(rolls.into(), 12, true).unwrap();
        assert!(once.same_as(again), "dice only: the same rolls give the same seed");
        // SeedSigner's method, worked out in Python: the first 16 bytes of sha256 of the 50 digits
        let seedsigner = "unveil nice picture region tragic fault cream strike tourist control recipe tourist";
        assert!(once.same_as(Seed::from_words(seedsigner.into()).unwrap()), "the seed SeedSigner makes from these rolls");
        let mixed = Seed::from_dice(rolls.into(), 12, false).unwrap();
        assert!(!mixed.same_as(once), "mixed with the phone's randomness");
        assert!(Seed::from_dice("123".into(), 12, true).unwrap_err().to_string().contains("50"));
    }

    #[test]
    fn typed_words_are_checked_and_compared() {
        let seed = Seed::from_words(WORDS.into()).unwrap();
        assert!(seed.matches_words(WORDS.to_uppercase()).unwrap());
        assert!(seed.matches_words(WORDS.replace("abandon", "aban")).unwrap(), "four letters are enough");
        let other = "legal winner thank year wave sausage worth useful legal winner thank yellow";
        assert!(!seed.matches_words(other.into()).unwrap());
        let wrong = Seed::from_words(WORDS.replace("about", "abaut")).map(|_| ()).unwrap_err().to_string();
        assert!(wrong.contains("#12"), "{wrong}");
        assert_eq!(final_words(WORDS.rsplit_once(' ').unwrap().0.into()).unwrap().len(), 128);
        assert!(word_suggestions("zo".into()).contains(&"zone".to_string()));
        assert!(word_suggestions(" ".into()).is_empty());
    }

    #[test]
    fn a_backup_qr_code_carries_its_name_and_still_reads_as_the_backup() {
        let seed = Seed::from_words(WORDS.into()).unwrap();
        let file = quick_backup(&seed, "a good long password");
        let line = backup_text(file.clone()).unwrap();
        let code = backup_qr_text(file.clone(), "  Poupança 2026\t".into()).unwrap();
        assert_eq!(code, format!("{line}\n# name: Poupança 2026"));
        // read as before: the name line is a comment, so earlier versions take the code as it is
        assert_eq!(backup::from_text(&code).unwrap(), backup::from_text(&line).unwrap());
        assert!(code.trim().to_uppercase().starts_with(backup::TEXT_PREFIX), "what earlier scanners check first");
        assert_eq!(backup_name_in(code).as_deref(), Some("Poupança 2026"));
        // no name: the backup line alone, exactly as before
        assert_eq!(backup_qr_text(file.clone(), " \n ".into()).unwrap(), line);
        assert_eq!(backup_name_in(line.clone()), None);
        assert_eq!(backup_name_in(file.clone()), None, "a file's own comment lines are no name");
        // one line, and not too long
        assert_eq!(backup_name_in(backup_qr_text(file.clone(), "two\nlines".into()).unwrap()).as_deref(), Some("twolines"));
        assert_eq!(backup_name_in(backup_qr_text(file, "x".repeat(150)).unwrap()).unwrap().chars().count(), backup::MAX_NAME_CHARS);
        assert_eq!(backup_name_in("SEEDLOCK1:ABC\n#NAME:  Cold  ".into()).as_deref(), Some("Cold"));
        assert!(backup_qr_text("SEEDLOCK1:ABC".into(), "x".into()).is_err(), "only a backup gets a QR code");
    }

    #[test]
    fn backup_names_follow_the_command_line_rules() {
        assert_eq!(check_backup_name("  \"my wallet.seedlock\" ".into()).unwrap(), "my wallet");
        for bad in ["", "a/b", "dot.", ".hidden", &"x".repeat(101)] {
            assert!(check_backup_name(bad.into()).is_err(), "{bad:?}");
        }
        let password = generate_password().unwrap();
        assert_eq!(password.split('-').count(), 8);
    }
}
