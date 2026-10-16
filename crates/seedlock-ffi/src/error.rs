//! The error the app sees: a message written for the person, and whether it was a wrong password, so
//! that the app can simply ask again.

use zeroize::{Zeroize, Zeroizing};

/// In Kotlin: `SeedLockException.WrongPassword` and `SeedLockException.Failed`, whose `message` is the
/// text to show.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum SeedLockError {
    /// The password does not open the backup (or the backup was changed).
    #[error("{0}")]
    WrongPassword(String),
    /// Anything else, said for the person.
    #[error("{0}")]
    Failed(String),
}

impl From<seedlock_core::Error> for SeedLockError {
    fn from(e: seedlock_core::Error) -> Self {
        let text = e.to_string();
        match e {
            seedlock_core::Error::WrongPassword => SeedLockError::WrongPassword(text),
            _ => SeedLockError::Failed(text),
        }
    }
}

pub type Result<T> = std::result::Result<T, SeedLockError>;

pub(crate) fn failed<T>(text: impl Into<String>) -> Result<T> {
    Err(SeedLockError::Failed(text.into()))
}

/// A password or passphrase as the app passes it: its UTF-8 bytes (the app wipes its own copy right
/// after the call). Becomes text that is wiped when dropped, without being copied first.
pub(crate) fn secret_text(bytes: Vec<u8>) -> Result<Zeroizing<String>> {
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(e) => {
            e.into_bytes().zeroize();
            failed("the password is not valid text")
        }
    }
}
