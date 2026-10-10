//! Error type shared by every module of `seedlock-core`.

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong. Messages are written for end users.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // ----- mnemonic -------------------------------------------------------
    #[error("a mnemonic has 12, 15, 18, 21 or 24 words, but {0} were entered")]
    BadWordCount(usize),

    #[error("word #{position} (\"{word}\") is not a valid BIP39 word{hint}")]
    UnknownWord {
        position: usize,
        word: String,
        hint: String,
    },

    #[error("the mnemonic checksum does not match: a word is wrong or the words are in the wrong order")]
    BadChecksum,

    // ----- encrypted backup ----------------------------------------------
    #[error("the password must be at least {0} characters long")]
    PasswordTooShort(usize),

    #[error("this is not a SeedLock backup")]
    NotABackup,

    #[error("the backup text is damaged or was copied incorrectly (checksum mismatch)")]
    BackupChecksum,

    #[error("this backup uses format version {0}, which this version of SeedLock cannot read")]
    UnsupportedVersion(u8),

    #[error("this backup uses an unknown key-derivation function (id {0})")]
    UnsupportedKdf(u8),

    #[error("the backup's key-derivation settings are outside the supported range")]
    KdfParams,

    #[error("wrong password, or the backup has been modified")]
    WrongPassword,

    // ----- entropy --------------------------------------------------------
    #[error("dice rolls: {0}")]
    Dice(String),

    #[error("the operating system's random number generator failed: {0}")]
    Rng(String),

    // ----- wallet / psbt ---------------------------------------------------
    #[error("{0}")]
    Wallet(String),

    #[error("{0}")]
    Psbt(String),

    #[error("{0}")]
    Multisig(String),

    /// A bug or an unexpected failure inside a cryptographic library.
    #[error("internal error: {0}")]
    Internal(String),
}
