//! Reading a transaction, a backup or a multisig wallet with the phone's camera. The app hands every
//! grey camera picture to [`QrScanner::add_picture`] and shows what comes back: where the codes are, how
//! many parts of an animated code have arrived, and what to tell the person. The command-line program's
//! scanner works the same way (`scan.rs`), with the same messages.

use crate::error::{failed, Result};
use crate::multisig::MultisigWallet;
use crate::transaction::Transaction;
use seedlock_core::backup;
use seedlock_core::multisig::{DescriptorScanner, DescriptorUpdate};
use seedlock_core::psbt::{PsbtScanner, ScanUpdate};
use seedlock_qr::{find_qr_codes, qr_in_view, Frame};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the camera is looking for.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanTarget {
    /// A transaction to sign, shown by Sparrow (an animated `ur:crypto-psbt` code, BBQr, or one code).
    Transaction,
    /// A SeedLock backup's QR code (the command-line program's menu 8 makes it).
    Backup,
    /// A multisig wallet's descriptor, shown by Sparrow (the wallet's Settings, Script Policy, the QR code
    /// button): a `ur:crypto-output` code, often animated, BBQr, or the descriptor as text in one code.
    MultisigWallet,
}

/// What happened with one camera picture.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanStep {
    /// The corners of every QR code read in this picture, in the picture's pixels: x and y of the four
    /// corners of each code, so 8 numbers per code. Empty when none was read.
    pub corners: Vec<i32>,
    /// Parts of an animated code received so far, and how many it has (0 and 0: none yet).
    pub have: i32,
    pub total: i32,
    /// What to tell the person now, if anything new.
    pub message: Option<String>,
    /// True once the whole transaction (or the backup, or the multisig wallet) has been read: take it with
    /// [`QrScanner::transaction`], [`QrScanner::backup_text`] or [`QrScanner::multisig_wallet`], and switch
    /// the camera off. Also true when Sparrow's code of a multisig wallet was read, but that wallet cannot be
    /// used: [`QrScanner::multisig_wallet`] then says why.
    pub done: bool,
}

const UNREAD_HINT: &str = "A QR code is in view but cannot be read yet: hold the phone steady and closer, so that the code \
                           fills more of the picture. Less dense codes read more easily: in Sparrow's QR window, click \
                           Less Dense (or set the density to Low).";

const TRANSACTION_TIP: &str = "No QR code was read yet. Tips: make the QR code on the other screen as big as possible \
                               (full screen), hold the phone 20 to 30 cm from it, keep it steady and avoid glare from \
                               lamps or windows.";

const BACKUP_TIP: &str = "No QR code was read yet. Tips: hold the backup's QR code (printed, or on a screen at full \
                          brightness) 20 to 30 cm from the phone, flat and steady, with good light and no glare.";

/// Pictures larger than this (per side) are refused.
const MAX_SIDE: i32 = 4096;

struct State {
    target: ScanTarget,
    collector: PsbtScanner,
    descriptor: DescriptorScanner,
    transaction: Option<Arc<Transaction>>,
    backup: Option<String>,
    /// The multisig wallet read, or why Sparrow's code that was read cannot be used.
    multisig: Option<std::result::Result<Arc<MultisigWallet>, String>>,
    /// When a code was last read, and whether the tip for "nothing read for a while" was given since.
    last_code: Instant,
    told_tip: bool,
    /// When it was last looked whether an unreadable code is in view, and since when one is.
    hint_checked: Instant,
    in_view_since: Option<Instant>,
    progress: (i32, i32),
}

/// Collects what the camera reads until the whole transaction (or backup) is known. One scanner per scan.
#[derive(uniffi::Object)]
pub struct QrScanner {
    state: Mutex<State>,
}

#[uniffi::export]
impl QrScanner {
    #[uniffi::constructor]
    pub fn new(target: ScanTarget) -> Arc<QrScanner> {
        let now = Instant::now();
        Arc::new(QrScanner {
            state: Mutex::new(State {
                target,
                collector: PsbtScanner::new(),
                descriptor: DescriptorScanner::new(),
                transaction: None,
                backup: None,
                multisig: None,
                last_code: now,
                told_tip: false,
                hint_checked: now,
                in_view_since: None,
                progress: (0, 0),
            }),
        })
    }

    /// One grey camera picture: `width` x `height` bytes, one per pixel, row after row (on Android, the
    /// Y plane of a YUV picture with its row padding removed). Takes up to about a third of a second:
    /// call it away from the screen's thread, and drop the pictures that arrive meanwhile.
    pub fn add_picture(&self, width: i32, height: i32, luma: Vec<u8>) -> Result<ScanStep> {
        let fits = width > 0 && height > 0 && width <= MAX_SIDE && height <= MAX_SIDE && luma.len() == (width as usize) * (height as usize);
        if !fits {
            return failed(format!("a camera picture of {width} x {height} pixels must have {} bytes, not {}", i64::from(width) * i64::from(height), luma.len()));
        }
        let frame = Frame { width: width as usize, height: height as usize, luma };
        let found = find_qr_codes(&frame);
        let mut s = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut step = ScanStep { have: s.progress.0, total: s.progress.1, ..ScanStep::default() };
        if s.transaction.is_some() || s.backup.is_some() || s.multisig.is_some() {
            step.done = true;
            return Ok(step);
        }
        if found.is_empty() {
            if s.hint_checked.elapsed() >= Duration::from_secs(1) {
                s.hint_checked = Instant::now();
                if qr_in_view(&frame) {
                    let since = *s.in_view_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(3) {
                        step.message = Some(UNREAD_HINT.to_string());
                    }
                } else {
                    s.in_view_since = None;
                }
            }
            if s.last_code.elapsed() > Duration::from_secs(12) && !s.told_tip {
                s.told_tip = true;
                let tip = if s.target == ScanTarget::Backup { BACKUP_TIP } else { TRANSACTION_TIP };
                step.message = Some(tip.to_string());
            }
            return Ok(step);
        }
        s.in_view_since = None;
        s.last_code = Instant::now();
        s.told_tip = false;
        step.corners = found.iter().flat_map(|f| f.quad.iter().flat_map(|&(x, y)| [x, y])).collect();
        for code in found {
            match s.target {
                ScanTarget::Transaction => match s.collector.add(&code.text) {
                    ScanUpdate::Done(psbt) => {
                        s.transaction = Some(Arc::new(Transaction::new(psbt)));
                        s.progress = (1, 1);
                        step.have = 1;
                        step.total = 1;
                        step.done = true;
                        step.message = Some("Received the whole transaction.".to_string());
                        return Ok(step);
                    }
                    ScanUpdate::Progress { have, total } => {
                        s.progress = (have as i32, total as i32);
                        step.have = have as i32;
                        step.total = total as i32;
                        step.message = Some(format!("{have} of {total} parts received. Keep the camera on the code."));
                    }
                    ScanUpdate::Ignored(why) => step.message = Some(format!("A QR code was seen, but {why}.")),
                },
                ScanTarget::Backup => {
                    let text = code.text.trim();
                    let upper = text.to_uppercase();
                    if upper.starts_with(backup::TEXT_PREFIX) {
                        // only taken when the checksum inside fits: a misread code is never taken
                        if backup::from_text(text).is_ok() {
                            s.backup = Some(text.to_string());
                            step.done = true;
                            step.message = Some("Read the backup's QR code.".to_string());
                            return Ok(step);
                        }
                        step.message = Some("A SeedLock QR code was seen, but it did not read correctly. Hold it steady.".to_string());
                    } else if upper.starts_with("UR:CRYPTO-PSBT/") || upper.starts_with("UR:PSBT/") {
                        step.message = Some("That is a transaction's QR code, not a backup. Show the backup's QR code.".to_string());
                    } else if upper.starts_with("UR:") {
                        step.message = Some("That is a wallet's QR code, not a backup. Show the backup's QR code.".to_string());
                    } else {
                        step.message = Some("A QR code was seen, but it is not a SeedLock backup.".to_string());
                    }
                }
                ScanTarget::MultisigWallet => match s.descriptor.add(&code.text) {
                    DescriptorUpdate::Done(wallet) => {
                        s.multisig = Some(Ok(Arc::new(MultisigWallet::new(wallet))));
                        s.progress = (1, 1);
                        step.have = 1;
                        step.total = 1;
                        step.done = true;
                        step.message = Some("Read the wallet's QR code.".to_string());
                        return Ok(step);
                    }
                    DescriptorUpdate::Progress { have, total } => {
                        s.progress = (have as i32, total as i32);
                        step.have = have as i32;
                        step.total = total as i32;
                        step.message = Some(format!("{have} of {total} parts received. Keep the camera on the code."));
                    }
                    DescriptorUpdate::Ignored(why) => step.message = Some(format!("A QR code was seen, but {why}.")),
                    DescriptorUpdate::Refused(why) => {
                        let text = format!("Sparrow's QR code was read, but that wallet cannot be added: {why}");
                        s.multisig = Some(Err(text.clone()));
                        step.done = true;
                        step.message = Some(text);
                        return Ok(step);
                    }
                },
            }
        }
        Ok(step)
    }

    /// The transaction, once [`ScanStep::done`] said it is complete.
    pub fn transaction(&self) -> Option<Arc<Transaction>> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).transaction.clone()
    }

    /// The backup's text (`SEEDLOCK1:...`), once [`ScanStep::done`] said it was read.
    pub fn backup_text(&self) -> Option<String> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).backup.clone()
    }

    /// The multisig wallet, once [`ScanStep::done`] said Sparrow's code was read; the error says why
    /// that wallet cannot be used (a Taproot wallet, for example), or that none was read yet.
    pub fn multisig_wallet(&self) -> Result<Arc<MultisigWallet>> {
        match &self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).multisig {
            Some(Ok(wallet)) => Ok(wallet.clone()),
            Some(Err(why)) => failed(why.clone()),
            None => failed("no multisig wallet was read yet"),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::qr::{qr_animation, qr_code, QrImage};
    use seedlock_core::bitcoin::{absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness};

    /// A QR code as the phone's camera sees it: drawn 6 px per module (fewer for a large code, down to 4) on
    /// a grey picture, slightly soft and noisy.
    pub(crate) fn camera_picture(code: &QrImage, seed: u64) -> (i32, i32, Vec<u8>) {
        let n = code.width as usize;
        let (w, h) = (640usize, 480usize);
        let px = (h / (n + 8)).clamp(4, 6);
        let side = (n + 8) * px;
        assert!(side <= h, "the code fits in the picture");
        let (x0, y0) = ((w - side) / 2, (h - side) / 2);
        let mut luma = vec![170u8; w * h];
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = (x / px, y / px);
                let dark = mx >= 4 && my >= 4 && mx < n + 4 && my < n + 4 && code.modules[(my - 4) * n + mx - 4] == 1;
                luma[(y0 + y) * w + x0 + x] = if dark { 35 } else { 230 };
            }
        }
        let src = luma.clone();
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let sum: u32 = (0..3).flat_map(|dy| (0..3).map(move |dx| (dx, dy))).map(|(dx, dy)| u32::from(src[(y + dy - 1) * w + x + dx - 1])).sum();
                luma[y * w + x] = (sum / 9) as u8;
            }
        }
        let mut state = seed | 1;
        for v in luma.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *v = (i32::from(*v) + ((state >> 33) % 11) as i32 - 5).clamp(0, 255) as u8;
        }
        (w as i32, h as i32, luma)
    }

    pub(crate) fn sample_transaction() -> Arc<Transaction> {
        let tx = seedlock_core::bitcoin::Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: (0..6).map(|i| TxOut { value: Amount::from_sat(1000 + i), script_pubkey: ScriptBuf::from(vec![0x51; 34]) }).collect(),
        };
        Arc::new(Transaction::new(seedlock_core::bitcoin::Psbt::from_unsigned_tx(tx).unwrap()))
    }

    #[test]
    fn collects_an_animated_transaction_shown_on_a_screen() {
        let tx = sample_transaction();
        let frames = tx.qr_frames().unwrap();
        assert!(frames.len() >= 4 && frames.iter().all(|f| f.starts_with("UR:CRYPTO-PSBT/")), "{frames:?}");
        let codes = qr_animation(frames.clone()).unwrap();
        let scanner = QrScanner::new(ScanTarget::Transaction);
        let mut done = None;
        // start in the middle of the loop, as a camera does
        for (i, code) in codes.iter().cycle().skip(2).take(codes.len() * 2).enumerate() {
            let (w, h, luma) = camera_picture(code, i as u64);
            let step = scanner.add_picture(w, h, luma).unwrap();
            assert_eq!(step.corners.len(), 8, "one code, four corners");
            assert!(step.corners.chunks(2).all(|c| (0..w).contains(&c[0]) && (0..h).contains(&c[1])));
            if step.done {
                done = Some(i);
                break;
            }
            assert!(step.total > 0 && step.have <= step.total && step.message.as_deref().unwrap_or("").contains("parts received"));
        }
        assert!(done.is_some(), "the whole transaction arrived");
        let got = scanner.transaction().unwrap();
        assert_eq!(got.to_bytes(), tx.to_bytes());
        assert!(scanner.backup_text().is_none());
        // after that, every picture just says done
        let (w, h, luma) = camera_picture(&codes[0], 99);
        assert!(scanner.add_picture(w, h, luma).unwrap().done);
    }

    #[test]
    fn reads_a_backup_and_passes_over_other_codes() {
        let quick = backup::KdfParams { m_cost_kib: 64, t_cost: 1, parallelism: 1 };
        let text = backup::to_text(&backup::encrypt(&[7u8; 16], "a test password", quick).unwrap());
        let scanner = QrScanner::new(ScanTarget::Backup);
        let tx_code = qr_animation(sample_transaction().qr_frames().unwrap()).unwrap().remove(0);
        let (w, h, luma) = camera_picture(&tx_code, 1);
        let step = scanner.add_picture(w, h, luma).unwrap();
        assert!(!step.done && step.message.unwrap().contains("transaction's QR code, not a backup"));
        let (w, h, luma) = camera_picture(&qr_code("HTTPS://EXAMPLE.COM/SOMETHING".into()).unwrap(), 2);
        assert!(scanner.add_picture(w, h, luma).unwrap().message.unwrap().contains("not a SeedLock backup"));
        // a damaged backup code is never taken
        let mut damaged: Vec<char> = text.chars().collect();
        damaged[30] = if damaged[30] == 'A' { 'B' } else { 'A' };
        let (w, h, luma) = camera_picture(&qr_code(damaged.into_iter().collect()).unwrap(), 3);
        let step = scanner.add_picture(w, h, luma).unwrap();
        assert!(!step.done && step.message.unwrap().contains("did not read correctly"));
        let (w, h, luma) = camera_picture(&qr_code(text.clone()).unwrap(), 4);
        assert!(scanner.add_picture(w, h, luma).unwrap().done);
        assert_eq!(scanner.backup_text().unwrap(), text);
        assert!(scanner.transaction().is_none());
        assert_eq!(crate::seed::backup_name_in(text.clone()), None, "the command-line program's codes carry no name yet");

        // a backup's QR code made by this app carries its name, and is read as the backup all the same
        let named = crate::seed::backup_qr_text(text.clone(), "Poupança da família".into()).unwrap();
        let scanner = QrScanner::new(ScanTarget::Backup);
        let (w, h, luma) = camera_picture(&qr_code(named.clone()).unwrap(), 5);
        assert!(scanner.add_picture(w, h, luma).unwrap().done);
        let read = scanner.backup_text().unwrap();
        assert_eq!(read, named);
        assert_eq!(crate::seed::backup_text(read.clone()).unwrap(), text, "the same backup");
        assert_eq!(crate::seed::backup_name_in(read).as_deref(), Some("Poupança da família"));
    }

    #[test]
    fn a_picture_of_the_wrong_size_is_refused() {
        let scanner = QrScanner::new(ScanTarget::Transaction);
        assert!(scanner.add_picture(640, 480, vec![0; 100]).unwrap_err().to_string().contains("640 x 480"));
        assert!(scanner.add_picture(-1, 480, Vec::new()).is_err());
        assert!(scanner.add_picture(5000, 1, vec![0; 5000]).is_err());
        // an empty picture: nothing read, nothing to say yet
        assert_eq!(scanner.add_picture(64, 48, vec![128; 64 * 48]).unwrap(), ScanStep::default());
    }

    /// A multisig wallet as the UR crypto-output that Sparrow shows (its SettingsController and hummingbird):
    /// sortedmulti, every key with "is-private: false", its network, origin with depth, parent fingerprint
    /// and its keystore's label.
    fn sparrow_crypto_output(ms: &MultisigWallet) -> Vec<u8> {
        use seedlock_core::bitcoin::bip32::ChildNumber;
        use seedlock_core::cbor::{self, Value};
        let u = Value::Uint;
        let fp = |f: seedlock_core::bitcoin::bip32::Fingerprint| u(u64::from(u32::from_be_bytes(f.to_bytes())));
        let keys = ms
            .inner
            .cosigners
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let steps: &[ChildNumber] = c.path.as_ref();
                let components = steps
                    .iter()
                    .flat_map(|s| match *s {
                        ChildNumber::Hardened { index } => [u(u64::from(index)), Value::Bool(true)],
                        ChildNumber::Normal { index } => [u(u64::from(index)), Value::Bool(false)],
                    })
                    .collect();
                let origin = Value::Map(vec![(u(1), Value::Array(components)), (u(2), fp(c.fingerprint)), (u(3), u(steps.len() as u64))]);
                Value::tag(
                    303,
                    Value::Map(vec![
                        (u(2), Value::Bool(false)),
                        (u(3), Value::Bytes(c.xpub.public_key.serialize().to_vec())),
                        (u(4), Value::Bytes(c.xpub.chain_code.to_bytes().to_vec())),
                        (u(5), Value::tag(305, Value::Map(vec![(u(1), u(0)), (u(2), u(1))]))),
                        (u(6), Value::tag(304, origin)),
                        (u(8), fp(c.xpub.parent_fingerprint)),
                        (u(9), Value::Text(format!("Keystore {}", i + 1))),
                    ]),
                )
            })
            .collect();
        let multi = Value::tag(407, Value::Map(vec![(u(1), u(ms.inner.threshold as u64)), (u(2), Value::Array(keys))]));
        cbor::encode(&Value::tag(401, multi))
    }

    #[test]
    fn reads_a_multisig_wallet_from_sparrows_qr_codes() {
        use crate::multisig::tests::descriptor;
        let text = descriptor("wsh(", "sortedmulti", "48h/1h/0h/2h");
        let ms = MultisigWallet::from_text(text.clone()).unwrap();

        // as Sparrow shows it: an animated UR crypto-output, taken up wherever the camera comes in
        let mut encoder = ur::Encoder::new(&sparrow_crypto_output(&ms), 100, "crypto-output").unwrap();
        let parts: Vec<String> = (0..2 * encoder.fragment_count()).map(|_| encoder.next_part().unwrap().to_uppercase()).collect();
        let codes = qr_animation(parts).unwrap();
        assert!(codes.len() >= 6, "an animated code of {} parts", codes.len());
        let scanner = QrScanner::new(ScanTarget::MultisigWallet);
        assert!(scanner.multisig_wallet().unwrap_err().to_string().contains("no multisig wallet was read yet"));
        let mut done = false;
        for (i, code) in codes.iter().cycle().skip(3).take(codes.len() * 2).enumerate() {
            let (w, h, luma) = camera_picture(code, 50 + i as u64);
            let step = scanner.add_picture(w, h, luma).unwrap();
            if step.done {
                assert_eq!(step.message.as_deref(), Some("Read the wallet's QR code."));
                done = true;
                break;
            }
            assert!(step.message.unwrap_or_default().contains("parts received. Keep the camera on the code."));
        }
        assert!(done, "the whole wallet arrived");
        assert!(scanner.multisig_wallet().unwrap().same_as(ms.clone()));
        assert!(scanner.transaction().is_none() && scanner.backup_text().is_none());

        // the descriptor as text in one code, after a transaction's code that is passed over
        let scanner = QrScanner::new(ScanTarget::MultisigWallet);
        let tx_code = qr_animation(sample_transaction().qr_frames().unwrap()).unwrap().remove(0);
        let (w, h, luma) = camera_picture(&tx_code, 1);
        let step = scanner.add_picture(w, h, luma).unwrap();
        assert!(!step.done && step.message.unwrap().contains("A QR code was seen, but that is a transaction's QR code"));
        let (w, h, luma) = camera_picture(&qr_code(text.clone()).unwrap(), 2);
        assert!(scanner.add_picture(w, h, luma).unwrap().done);
        assert!(scanner.multisig_wallet().unwrap().same_as(ms.clone()));

        // a wallet that cannot be added: the scan ends, saying why
        let (body, _) = text.rsplit_once('#').unwrap();
        let body = body.replace("wsh(sortedmulti(", "tr(sortedmulti_a(");
        let taproot = format!("{body}#{}", seedlock_core::wallet::descriptor_checksum(&body).unwrap());
        let scanner = QrScanner::new(ScanTarget::MultisigWallet);
        let (w, h, luma) = camera_picture(&qr_code(taproot).unwrap(), 3);
        let step = scanner.add_picture(w, h, luma).unwrap();
        assert!(step.done);
        let why = scanner.multisig_wallet().unwrap_err().to_string();
        assert!(why.starts_with("Sparrow's QR code was read, but that wallet cannot be added: this is a Taproot wallet"), "{why}");
        assert_eq!(step.message.unwrap(), why);

        // and a wallet's code where a backup is expected
        let scanner = QrScanner::new(ScanTarget::Backup);
        let (w, h, luma) = camera_picture(&codes[0], 4);
        assert!(scanner.add_picture(w, h, luma).unwrap().message.unwrap().contains("That is a wallet's QR code, not a backup"));
    }
}
