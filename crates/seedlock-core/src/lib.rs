//! # seedlock-core
//!
//! Offline Bitcoin seed tooling that needs no special hardware:
//! password-encrypted mnemonic backups, BIP32/44/49/84/86 derivation and
//! PSBT review/signing.
//!
//! The crate does no I/O and never touches the network, so it can be embedded
//! in a command-line tool today and in Android / iOS apps later.
#![forbid(unsafe_code)]

pub mod backup;
pub mod bbqr;
pub mod cbor;
pub mod entropy;
pub mod error;
pub mod mnemonic;
pub mod multisig;
pub mod psbt;
pub mod wallet;

pub use error::{Error, Result};

// Re-exports so front-ends (CLI, mobile bindings) need not depend on these crates directly.
pub use bip39::Mnemonic;
pub use bitcoin::Network;
/// The underlying `bitcoin` crate, re-exported so callers can build and inspect
/// transactions with exactly the version this crate uses.
pub use bitcoin;
