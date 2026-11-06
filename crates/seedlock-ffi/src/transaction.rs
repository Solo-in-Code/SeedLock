//! A transaction to sign (a PSBT), read from a file or scanned with the camera, and the signed one.

use crate::error::{failed, Result};
use crate::multisig::RegisteredMultisig;
use seedlock_core::psbt::{self, Request};
use std::sync::Arc;

/// The largest .psbt file accepted (the same limit as the command-line program's).
const MAX_PSBT_BYTES: usize = 16 * 1024 * 1024;

/// What a transaction's key paths say about its network (they decide which wallet signs it).
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkHint {
    /// Real bitcoin.
    Mainnet,
    /// A test network: testnet, signet or regtest (a transaction cannot tell them apart; they use
    /// the same keys).
    TestNetwork,
    /// The transaction does not say: ask the person.
    Unknown,
    /// Some key paths say mainnet and others a test network: something is wrong with the transaction.
    Conflicting,
}

/// A transaction (PSBT), of version 0 (what most wallets send) or 2 (what Sparrow sends with silent payments).
#[derive(uniffi::Object, Debug)]
pub struct Transaction {
    pub(crate) request: Request,
}

impl Transaction {
    pub(crate) fn new(request: Request) -> Transaction {
        Transaction { request }
    }
}

#[uniffi::export]
impl Transaction {
    /// From the contents of a .psbt file (binary, or base64 or hex text), as Sparrow saves it.
    #[uniffi::constructor]
    pub fn from_bytes(data: Vec<u8>) -> Result<Arc<Transaction>> {
        if data.len() > MAX_PSBT_BYTES {
            return failed("that file is too large to be a transaction (PSBT)");
        }
        Ok(Arc::new(Transaction::new(Request::decode(&data)?)))
    }

    /// What its key paths say about the network: mainnet, a test network, nothing, or both.
    pub fn network_hint(&self) -> NetworkHint {
        match self.request.network_hint() {
            psbt::NetworkHint::Mainnet => NetworkHint::Mainnet,
            psbt::NetworkHint::TestNetwork => NetworkHint::TestNetwork,
            psbt::NetworkHint::Unknown => NetworkHint::Unknown,
            psbt::NetworkHint::Conflicting => NetworkHint::Conflicting,
        }
    }

    /// [`Transaction::network_hint`], and when the key paths do not say (a legacy multisig wallet's paths,
    /// m/45'/..., name no network), the network of the multisig wallet on this phone that one of its inputs
    /// belongs to, as the command-line program finds it.
    pub fn network_hint_with(&self, multisig: Vec<RegisteredMultisig>) -> NetworkHint {
        let hint = self.network_hint();
        if hint != NetworkHint::Unknown {
            return hint;
        }
        let owner = self.request.psbt().inputs.iter().flat_map(|i| i.bip32_derivation.values()).find_map(|(fingerprint, path)| {
            multisig.iter().find(|w| w.wallet.inner.locate(*fingerprint, path).is_some()).map(|w| w.wallet.inner.is_mainnet())
        });
        match owner {
            Some(true) => NetworkHint::Mainnet,
            Some(false) => NetworkHint::TestNetwork,
            None => NetworkHint::Unknown,
        }
    }

    /// The master fingerprints of the wallets that the transaction says sign its inputs, each once, in the
    /// order the transaction lists them: for a multisig wallet's payment, its cosigners; for silent payments
    /// received, the wallet whose spend key they need. (With a BIP39 passphrase a seed is another wallet, with
    /// another fingerprint.)
    pub fn input_fingerprints(&self) -> Vec<String> {
        self.request.input_fingerprints().iter().map(ToString::to_string).collect()
    }

    pub fn input_count(&self) -> i32 {
        self.request.psbt().inputs.len() as i32
    }

    pub fn output_count(&self) -> i32 {
        self.request.psbt().unsigned_tx.output.len() as i32
    }

    /// Whether it spends or pays silent payments.
    pub fn has_silent_payments(&self) -> bool {
        self.request.has_silent_payments()
    }

    /// The transaction's id (64 hex digits), for example for a file name. (Before a payment to a silent payment
    /// address is signed, its outputs are not worked out yet: the id comes only then.)
    pub fn txid(&self) -> String {
        self.request.psbt().unsigned_tx.compute_txid().to_string()
    }

    /// The binary PSBT, for a .psbt file, in the version it came in.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.request.serialize()
    }

    /// The PSBT as base64 text, as Sparrow and Bitcoin Core accept it pasted.
    pub fn to_base64(&self) -> String {
        self.request.to_base64()
    }

    /// The frames of the animated QR code that gives the transaction to Sparrow (`UR:CRYPTO-PSBT/...`,
    /// in capitals for compact QR codes). Show them in a loop, one every 300 ms, with [`crate::qr_animation`].
    pub fn qr_frames(&self) -> Result<Vec<String>> {
        Ok(self.request.ur_frames()?.into_iter().map(|f| f.to_uppercase()).collect())
    }
}
