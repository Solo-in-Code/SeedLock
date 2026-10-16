//! QR codes to show on the screen: a backup or a wallet in one code, a signed transaction as an
//! animated series (the same codes the command-line program shows).

use crate::error::{failed, Result};
use seedlock_qr::{matrix, same_size_matrices, EcLevel, Matrix};

/// A QR code as a grid of modules, for the app to draw. The app adds the light border ("quiet zone")
/// of 4 modules on every side, and draws each module as a square of whole pixels.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct QrImage {
    /// Modules per side, without the border.
    pub width: i32,
    /// `width * width` values, row after row: 1 for a dark module, 0 for a light one.
    pub modules: Vec<u8>,
}

fn image(m: Matrix) -> QrImage {
    QrImage { width: m.width as i32, modules: m.dark.into_iter().map(u8::from).collect() }
}

/// One QR code of `text` (a backup, or a wallet's descriptor), with medium error correction as the
/// command-line program makes them.
#[uniffi::export]
pub fn qr_code(text: String) -> Result<QrImage> {
    match matrix(&text, EcLevel::M) {
        Ok(m) => Ok(image(m)),
        Err(e) => failed(e.to_string()),
    }
}

/// The QR codes of an animated series ([`crate::Transaction::qr_frames`]), all of the same size so
/// that the picture stays still while the codes change.
#[uniffi::export]
pub fn qr_animation(frames: Vec<String>) -> Result<Vec<QrImage>> {
    match same_size_matrices(&frames, EcLevel::L) {
        Ok(codes) => Ok(codes.into_iter().map(image).collect()),
        Err(e) => failed(e.to_string()),
    }
}
