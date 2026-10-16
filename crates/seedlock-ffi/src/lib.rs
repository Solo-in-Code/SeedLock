//! # seedlock-ffi
//!
//! SeedLock for phones: the parts of `seedlock-core` that the Android app needs, made callable from
//! Kotlin with UniFFI (and later from Swift, for an iPhone app). The Kotlin code that calls these
//! functions is generated from this crate by `uniffi-bindgen` (see `android/README.md`).
//!
//! What the app can do through it, mirroring the command-line program's menu:
//!
//! * open a backup with its password ([`Seed::unlock`]) and check it, make a new seed
//!   ([`Seed::generate`], [`Seed::from_dice`]) or take one's words ([`Seed::from_words`]), and encrypt it
//!   into a backup ([`Seed::encrypt`]);
//! * lay a wallet out for Sparrow ([`Wallet::sparrow`]);
//! * read a transaction from the camera ([`QrScanner`]) or from a file ([`Transaction::from_bytes`]),
//!   review it ([`Wallet::review`]), sign it ([`Wallet::sign`]) and show the signed one as an animated
//!   QR code ([`Transaction::qr_frames`], [`qr_animation`]);
//! * multisig: lay this seed's key out for a multisig wallet in Sparrow ([`Wallet::cosigner_key`]),
//!   read a multisig wallet from Sparrow's QR code ([`QrScanner`]) or its exported file
//!   ([`MultisigWallet::from_text`]) and keep it as the command-line program does
//!   ([`MultisigWallet::file_contents`]), and check and sign its transactions ([`RegisteredMultisig`]).
//!
//! Secrets: the seed stays in Rust, in memory that is wiped when the app lets go of it (UniFFI objects
//! are freed when the app calls `close()` / `destroy()` on them, or when they are garbage collected).
//! Passwords and passphrases come in as bytes that the app wipes on its side right after the call, and
//! this side wipes its own copy. The words of a seed only leave Rust when the app shows them.
//!
//! The crate's own code is safe Rust (the compiler refuses anything else); only the glue that UniFFI's
//! macros generate deals with raw memory.
#![forbid(unsafe_code)]

uniffi::setup_scaffolding!();

mod error;
mod multisig;
mod qr;
mod scanner;
mod seed;
mod transaction;
mod wallet;

pub use error::SeedLockError;
pub use multisig::{check_wallet_name, multisig_kind_name, CosignerKey, CosignerLine, MultisigKind, MultisigWallet, RegisteredMultisig};
pub use qr::{qr_animation, qr_code, QrImage};
pub use scanner::{QrScanner, ScanStep, ScanTarget};
pub use seed::{
    backup_file_contents, backup_info, backup_name_in, backup_qr_text, backup_text, check_backup_name, count_dice_rolls,
    dice_rolls_needed, final_words, generate_password, min_password_length, word_suggestions, BackupInfo, BackupStrength,
    NewBackup, Seed,
};
pub use transaction::{NetworkHint, Transaction};
pub use wallet::{AddressLine, AddressType, InputLine, Net, OtherAddressType, OutputLine, Signed, SparrowWallet, TxReview, Wallet};

/// The version of SeedLock this library belongs to (the same number as the command-line program's).
#[uniffi::export]
pub fn seedlock_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
