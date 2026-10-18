//! QR codes in a window of their own, the way Sparrow shows them, so that their size does not
//! depend on the terminal (its window size, its font, or how it draws characters). The
//! pictures are sent through a pipe to **ffplay**, which comes with ffmpeg and already shows the
//! camera preview. Nothing is written to disk.

use crate::children::Running;
use crate::qrimg;
use crate::scan::{collect_messages, ffplay_program};
use qrcode::QrCode;
use std::ffi::OsStr;
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Pictures per second sent to the window; a QR frame is sent several times to stay up long enough.
const RATE: u64 = 10;

/// The size the picture aims for, in pixels (the window can be made bigger by dragging its corner).
const TARGET_SIDE: usize = 640;

/// A window that shows QR codes one after the other, in a loop. It closes when this is dropped,
/// at once when SeedLock stops at once (children.rs), and by itself if SeedLock stops for any other reason
/// (ffplay ends when its input ends).
pub struct QrWindow {
    child: Option<Running>,
    stop: Arc<AtomicBool>,
    feeder: Option<JoinHandle<()>>,
}

impl QrWindow {
    /// Show `codes` (all the same size) one after the other, each for `interval_ms`, in a loop.
    /// `Err` says why no window could be opened here.
    pub fn open(title: &str, codes: &[QrCode], interval_ms: u64) -> Result<QrWindow, String> {
        let need_screen = std::env::var_os("SEEDLOCK_FFPLAY").is_none();
        QrWindow::open_with(&ffplay_program(), need_screen, title, codes, interval_ms)
    }

    fn open_with(ffplay: &OsStr, need_screen: bool, title: &str, codes: &[QrCode], interval_ms: u64) -> Result<QrWindow, String> {
        if need_screen && cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err("no graphical screen was found (the DISPLAY variable is not set)".to_string());
        }
        let first = codes.first().ok_or("there is no QR code to show")?;
        let module_px = (TARGET_SIDE / (first.width() + 8)).clamp(4, 16);
        let pictures: Vec<(usize, Vec<u8>)> = codes.iter().map(|c| qrimg::luma(c, module_px)).collect();
        let side = pictures[0].0;
        if pictures.iter().any(|(s, _)| *s != side) {
            return Err("the QR codes are not all the same size".to_string());
        }
        let pictures: Vec<Vec<u8>> = pictures.into_iter().map(|(_, p)| p).collect();
        let size = format!("{side}x{side}");
        let rate = RATE.to_string();
        let mut child = Command::new(ffplay)
            .args(["-hide_banner", "-loglevel", "error", "-nostats", "-autoexit", "-f", "rawvideo", "-pixel_format", "gray"])
            .args(["-video_size", size.as_str(), "-framerate", rate.as_str(), "-window_title", title, "-i", "pipe:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == ErrorKind::NotFound {
                    "ffplay (it comes with ffmpeg) was not found".to_string()
                } else {
                    format!("cannot start ffplay: {e}")
                }
            })?;
        let mut input = child.stdin.take().ok_or("ffplay gave no input")?;
        let log = collect_messages(child.stderr.take().ok_or("ffplay gave no messages")?);
        let child = Running::keep(child);
        let stop = Arc::new(AtomicBool::new(false));
        let repeats = (interval_ms * RATE / 1000).max(1);
        let feeder = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                'show: loop {
                    for picture in &pictures {
                        for _ in 0..repeats {
                            if stop.load(Ordering::SeqCst) || input.write_all(picture).is_err() {
                                break 'show; // finished, or the window was closed
                            }
                            thread::sleep(Duration::from_millis(1000 / RATE));
                        }
                    }
                }
            })
        };
        // A window that cannot be opened makes ffplay stop within a moment: say why.
        thread::sleep(Duration::from_millis(700));
        if child.ended() {
            stop.store(true, Ordering::SeqCst);
            let _ = feeder.join();
            thread::sleep(Duration::from_millis(100)); // let the error text arrive
            let text = log.lock().map(|t| t.trim().to_string()).unwrap_or_default();
            return Err(if text.is_empty() { "ffplay stopped at once".to_string() } else { text });
        }
        Ok(QrWindow { child: Some(child), stop, feeder: Some(feeder) })
    }
}

impl Drop for QrWindow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        drop(self.child.take()); // ffplay is ended and waited for
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ui;
    use qrcode::EcLevel;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in for ffplay: a script in a folder of its own ({DIR} in `body` is that folder).
    fn script(name: &str, body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("seedlock-qrwindow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ffplay");
        std::fs::write(&path, body.replace("{DIR}", &dir.display().to_string())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }

    #[test]
    fn the_window_gets_every_code_in_turn_each_for_the_whole_interval() {
        let texts: Vec<String> = (1..=3).map(|i| format!("UR:CRYPTO-PSBT/{i}-3/LPADAXCFAXJNCYJZ{i}")).collect();
        let codes = ui::same_size_codes(&texts, EcLevel::L).unwrap();
        let (dir, ffplay) = script("feed", "#!/bin/sh\necho \"$@\" > '{DIR}/args'\nexec cat > '{DIR}/received'\n");
        let window = QrWindow::open_with(ffplay.as_os_str(), false, "SeedLock test window", &codes, 300).unwrap();
        thread::sleep(Duration::from_millis(2300));
        drop(window);
        let module_px = (TARGET_SIDE / (codes[0].width() + 8)).clamp(4, 16);
        let side = qrimg::luma(&codes[0], module_px).0;
        let expected: Vec<Vec<u8>> = codes.iter().map(|c| qrimg::luma(c, module_px).1).collect();
        let args = std::fs::read_to_string(dir.join("args")).unwrap();
        let size = format!("{side}x{side}");
        for wanted in ["rawvideo", "gray", size.as_str(), "-framerate 10", "-autoexit", "pipe:0", "SeedLock test window"] {
            assert!(args.contains(wanted), "{wanted} missing in: {args}");
        }
        let received = std::fs::read(dir.join("received")).unwrap();
        assert_eq!(received.len() % (side * side), 0, "whole pictures only");
        let order: Vec<usize> = received
            .chunks(side * side)
            .map(|p| expected.iter().position(|e| e.as_slice() == p).expect("every picture is one of the codes"))
            .collect();
        assert!(order.len() >= 15, "only {} pictures in about 3 seconds", order.len());
        // each code three times in a row (300 ms at 10 pictures a second), in order, in a loop
        for (i, &code) in order.iter().enumerate() {
            assert_eq!(code, (i / 3) % 3, "picture {i}: {order:?}");
        }
        // ... and each picture is a readable QR code
        let picture = &expected[1];
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| picture[y * side + x]);
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].decode().unwrap().1, texts[1]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_window_that_cannot_be_opened_says_why() {
        let codes = ui::same_size_codes(&["UR:CRYPTO-PSBT/1-3/LPADAXCFAXJNCYJZ".to_string()], EcLevel::L).unwrap();
        let (dir, failing) = script("fail", "#!/bin/sh\necho 'Could not initialize SDL - No available video device' >&2\nexit 1\n");
        let why = QrWindow::open_with(failing.as_os_str(), false, "t", &codes, 300).err().expect("ffplay failed");
        assert!(why.contains("No available video device"), "{why}");
        let why = QrWindow::open_with(OsStr::new("/nonexistent/ffplay"), false, "t", &codes, 300).err().expect("no ffplay");
        assert!(why.contains("ffplay (it comes with ffmpeg) was not found"), "{why}");
        if cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            let why = QrWindow::open_with(failing.as_os_str(), true, "t", &codes, 300).err().expect("no screen");
            assert!(why.contains("graphical screen"), "{why}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn kept_programs_end_when_dropped_and_all_at_once_before_seedlock_stops() {
        // a registry of its own, so that ending them all does not touch the other tests' windows
        static TEST: crate::children::Registry = crate::children::Registry::new();
        let (dir, sleeper) = script("kept", "#!/bin/sh\nexec sleep 30\n");
        let start = || Command::new(&sleeper).stdin(Stdio::null()).spawn().unwrap();
        let one = TEST.keep(start());
        assert!(!one.ended());
        drop(one); // ended and waited for
        let (a, b) = (TEST.keep(start()), TEST.keep(start()));
        assert!(!a.ended() && !b.ended());
        TEST.end_all(); // what offline.rs does with SeedLock's own, just before SeedLock stops
        thread::sleep(Duration::from_millis(300));
        assert!(a.ended() && b.ended(), "all ended at once, not when their input ends");
        std::fs::remove_dir_all(&dir).ok();
    }
}
