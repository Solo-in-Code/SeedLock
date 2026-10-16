//! Scanning a transaction from the QR code that another wallet (Sparrow, ...) shows on
//! its screen, using this computer's camera.
//!
//! The camera is read through the free program **ffmpeg**, started as a separate process
//! that sends grey pictures through a pipe. That keeps native camera libraries (hard to
//! build and to audit) out of SeedLock, and it works the same on Linux, macOS and
//! Windows. SeedLock finds the QR codes in the pictures ([`find_qr_codes`]), collects
//! the pieces (`PsbtScanner`) and returns the transaction.
//!
//! So that a person can aim the camera, the pictures are also shown in a window: they are
//! passed on to **ffplay** (it comes with ffmpeg), with a frame drawn around every QR code
//! that was read and a bar along the bottom that fills as the transaction arrives.
//! SeedLock itself never touches the network, and nothing from the camera is stored.

use crate::ui::{fail, Res};
use seedlock_core::backup;
use seedlock_core::bitcoin::Psbt;
use seedlock_core::psbt::{PsbtScanner, ScanUpdate};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, ErrorKind, IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long to keep looking before giving up.
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(180);

const PREVIEW_TITLE: &str = "SeedLock camera - point it at the QR code";

const FFMPEG_HELP: &str = "\
Scanning with the camera needs the free program \"ffmpeg\", and it was not found on this computer.
Install it once (do this while the computer still has internet, or copy the installer over on a USB stick):
  Arch Linux    : sudo pacman -S ffmpeg
  Debian/Ubuntu : sudo apt install ffmpeg      (other Linux: the package is called ffmpeg)
  macOS         : brew install ffmpeg
  Windows       : put ffmpeg.exe and ffplay.exe in the same folder as seedlock.exe. They are in the \"bin\" folder
                  of ffmpeg-release-essentials.zip (https://ffmpeg.org/download.html, the builds from gyan.dev).
                  On a computer that is online, winget install ffmpeg works too.
If ffmpeg is installed but not found, set the environment variable SEEDLOCK_FFMPEG to its full path.
You can also choose \"From a file\" instead of the camera.";

const CAMERA_TIPS: &str = "\
Things to check:
  - Is a camera connected, and not in use by another program (a video call, for example)?
  - Linux: \"Permission denied\" means your user may not use the camera. Add yourself to the \"video\" group
    (sudo usermod -aG video $USER), then log out and in again. \"v4l2-ctl --list-devices\" (package v4l-utils)
    shows which /dev/videoN is your camera: choose it with --camera N.
  - macOS: allow your terminal app to use the camera (System Settings > Privacy & Security > Camera).
  - Windows: allow desktop apps to use the camera (Settings > Privacy & security > Camera).
  - With more than one camera, pick another one with --camera 1 (or 2, ...).
  - Menu 9, \"Test the camera\", checks that pictures arrive and whether a QR code can be read.";

const BACKUP_TIP: &str = "\
No QR code was read yet. Tips: hold the backup's QR code (printed, or on a phone at full brightness) about 20 to 30 cm
from the camera, flat and steady, with good light and no glare.";

const NO_CODE_TIP: &str = "\
No QR code was read yet. Tips: make the QR code on the other screen as big as possible (full screen),
hold the camera about an arm's length from it, keep it steady and avoid glare from lamps or windows.";

// ---------------------------------------------------------------------------
// Finding QR codes in a picture: the seedlock-qr crate, which the phone app uses too
// ---------------------------------------------------------------------------

pub use seedlock_qr::{find_qr_codes, qr_in_view, Frame, Quad};

/// Says, after a few seconds, that a QR code is in view but cannot be read, and what helps.
struct UnreadHint {
    since: Option<Instant>,
    checked: Instant,
}

const UNREAD_HINT: &str = "A QR code is in view but cannot be read yet: hold the camera steady and closer, so that \
                           the code fills more of the picture. Less dense codes read more easily: in Sparrow's QR window, \
                           click Less Dense (or set the density to Low).";

impl UnreadHint {
    fn new() -> Self {
        UnreadHint { since: None, checked: Instant::now() }
    }

    /// Something was read: start counting again.
    fn reset(&mut self) {
        self.since = None;
    }

    /// Nothing was read in this picture: once a second, look whether a code is in view all the same.
    fn nothing_read(&mut self, frame: &Frame, status: &mut Status) {
        if self.checked.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.checked = Instant::now();
        if !qr_in_view(frame) {
            self.since = None;
            return;
        }
        let since = *self.since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(3) {
            status.show(UNREAD_HINT);
        }
    }
}

// ---------------------------------------------------------------------------
// What the preview window shows on top of the camera picture
// ---------------------------------------------------------------------------

/// A square of `value` around a point (clipped to the picture).
fn plot(luma: &mut [u8], width: usize, height: usize, x: i32, y: i32, radius: i32, value: u8) {
    for py in (y - radius).max(0)..=(y + radius).min(height as i32 - 1) {
        for px in (x - radius).max(0)..=(x + radius).min(width as i32 - 1) {
            luma[py as usize * width + px as usize] = value;
        }
    }
}

fn draw_line(luma: &mut [u8], width: usize, height: usize, a: (i32, i32), b: (i32, i32), radius: i32, value: u8) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    for s in 0..=steps {
        plot(luma, width, height, a.0 + (b.0 - a.0) * s / steps, a.1 + (b.1 - a.1) * s / steps, radius, value);
    }
}

/// The corners moved a little away from the centre, so that the outline lies in the empty
/// border around the code and never covers one of its squares.
fn grown(quad: &Quad) -> Quad {
    let cx = quad.iter().map(|p| p.0 as f32).sum::<f32>() / 4.0;
    let cy = quad.iter().map(|p| p.1 as f32).sum::<f32>() / 4.0;
    let out = |p: (i32, i32)| ((cx + (p.0 as f32 - cx) * 1.07).round() as i32, (cy + (p.1 as f32 - cy) * 1.07).round() as i32);
    [out(quad[0]), out(quad[1]), out(quad[2]), out(quad[3])]
}

/// Draw on a picture what SeedLock found: an outline around every QR code that was read
/// (black under white, so it shows on any background) and a bar along the bottom edge
/// that fills as the transaction arrives.
pub fn draw_overlay(luma: &mut [u8], width: usize, height: usize, quads: &[Quad], progress: Option<(usize, usize)>) {
    for quad in quads {
        let q = grown(quad);
        for (radius, value) in [(4, 0u8), (2, 255u8)] {
            for i in 0..4 {
                draw_line(luma, width, height, q[i], q[(i + 1) % 4], radius, value);
            }
        }
    }
    let bar = (height / 30).max(6);
    if let Some((have, total)) = progress {
        if total > 0 && height > bar + 4 && width > 8 {
            for y in height - bar..height {
                luma[y * width..(y + 1) * width].fill(0);
            }
            let filled = (width - 4) * have.min(total) / total;
            for y in height - bar + 2..height - 2 {
                luma[y * width + 2..y * width + 2 + filled].fill(255);
            }
        }
    }
}

#[derive(Default)]
struct Overlay {
    quads: Vec<Quad>,
    seen: Option<Instant>,
    progress: Option<(usize, usize)>,
}

/// State shared between the thread that reads the camera and the scanner.
struct Shared {
    overlay: Mutex<Overlay>,
    preview: Mutex<Option<SyncSender<Vec<u8>>>>,
}

/// Send a copy of the picture, with the overlay drawn on it, to the preview window.
fn to_preview(shared: &Shared, picture: &[u8], width: usize, height: usize) {
    let Ok(mut slot) = shared.preview.lock() else { return };
    let Some(window) = slot.as_ref() else { return };
    let mut copy = picture.to_vec();
    if let Ok(overlay) = shared.overlay.lock() {
        let recent = overlay.seen.is_some_and(|t| t.elapsed() < Duration::from_millis(800));
        draw_overlay(&mut copy, width, height, if recent { &overlay.quads } else { &[] }, overlay.progress);
    }
    // busy: this picture is skipped; closed: there is no window any more
    if matches!(window.try_send(copy), Err(TrySendError::Disconnected(_))) {
        *slot = None;
    }
}

// ---------------------------------------------------------------------------
// Collecting the pieces
// ---------------------------------------------------------------------------

/// Where camera pictures come from.
pub trait FrameSource {
    /// The next picture; `Ok(None)` if none arrived within `wait`.
    fn next_frame(&mut self, wait: Duration) -> Res<Option<Frame>>;
    /// What was found in the pictures so far (for the preview window): where the QR codes
    /// are and how much of the transaction has arrived.
    fn found(&mut self, _quads: &[Quad], _progress: Option<(usize, usize)>) {}
}

/// One line of progress that is rewritten in place at a terminal.
struct Status {
    last: String,
    in_place: bool,
    open_line: bool,
}

impl Status {
    fn new() -> Self {
        Status { last: String::new(), in_place: std::io::stderr().is_terminal(), open_line: false }
    }

    fn show(&mut self, line: &str) {
        if line == self.last {
            return;
        }
        self.last = line.to_string();
        if self.in_place {
            eprint!("\r\x1b[K{line}");
            let _ = std::io::stderr().flush();
            self.open_line = true;
        } else {
            eprintln!("{line}");
        }
    }

    fn note(&mut self, text: &str) {
        self.finish_line();
        eprintln!("{text}");
        self.last.clear();
    }

    fn finish_line(&mut self) {
        if self.open_line {
            eprintln!();
            self.open_line = false;
        }
    }
}

fn bar(have: usize, total: usize) -> String {
    let filled = if total == 0 { 0 } else { (have * 20 / total).min(20) };
    format!("[{}{}]", "#".repeat(filled), "-".repeat(20 - filled))
}

/// Read pictures until a whole transaction has been collected.
pub fn scan_psbt(source: &mut dyn FrameSource, give_up_after: Duration) -> Res<Psbt> {
    let started = Instant::now();
    let mut scanner = PsbtScanner::new();
    let mut status = Status::new();
    let mut last_code = Instant::now();
    let mut told_tip = false;
    let mut hint = UnreadHint::new();
    status.show("Camera is on. Looking for the QR code ...");
    loop {
        if started.elapsed() > give_up_after {
            status.finish_line();
            return fail(format!(
                "Nothing was scanned within {} minutes, so SeedLock stopped.\n{NO_CODE_TIP}",
                give_up_after.as_secs() / 60
            ));
        }
        let Some(frame) = source.next_frame(Duration::from_millis(500))? else { continue };
        let found = find_qr_codes(&frame);
        if found.is_empty() {
            hint.nothing_read(&frame, &mut status);
            if last_code.elapsed() > Duration::from_secs(12) && !told_tip {
                status.note(NO_CODE_TIP);
                told_tip = true;
            }
            continue;
        }
        hint.reset();
        last_code = Instant::now();
        told_tip = false;
        let quads: Vec<Quad> = found.iter().map(|f| f.quad).collect();
        source.found(&quads, None);
        for code in found {
            match scanner.add(&code.text) {
                ScanUpdate::Done(psbt) => {
                    source.found(&quads, Some((1, 1)));
                    status.show("Received the whole transaction.");
                    status.finish_line();
                    return Ok(psbt);
                }
                ScanUpdate::Progress { have, total } => {
                    source.found(&quads, Some((have, total)));
                    status.show(&format!("{} {have} of {total} parts received. Keep the camera on the code.", bar(have, total)));
                }
                ScanUpdate::Ignored(why) => status.show(&format!("A QR code was seen, but {why}.")),
            }
        }
    }
}

/// Read a SeedLock backup from its QR code (the one menu 8 makes) and return its SEEDLOCK1:...
/// text, exactly as if it had been pasted. A code is only taken if the checksum inside the text is
/// right; other QR codes are ignored, with a word about what they are.
pub fn scan_backup_text(source: &mut dyn FrameSource, give_up_after: Duration) -> Res<String> {
    let started = Instant::now();
    let mut status = Status::new();
    let mut last_code = Instant::now();
    let mut told_tip = false;
    let mut hint = UnreadHint::new();
    status.show("Camera is on. Looking for the backup's QR code ...");
    loop {
        if started.elapsed() > give_up_after {
            status.finish_line();
            return fail(format!(
                "No backup was read within {} minutes, so SeedLock stopped.\n{BACKUP_TIP}",
                give_up_after.as_secs() / 60
            ));
        }
        let Some(frame) = source.next_frame(Duration::from_millis(500))? else { continue };
        let found = find_qr_codes(&frame);
        if found.is_empty() {
            hint.nothing_read(&frame, &mut status);
            if last_code.elapsed() > Duration::from_secs(12) && !told_tip {
                status.note(BACKUP_TIP);
                told_tip = true;
            }
            continue;
        }
        hint.reset();
        last_code = Instant::now();
        told_tip = false;
        let quads: Vec<Quad> = found.iter().map(|f| f.quad).collect();
        source.found(&quads, None);
        for code in found {
            let text = code.text.trim();
            let upper = text.to_uppercase();
            if upper.starts_with(backup::TEXT_PREFIX) {
                if backup::from_text(text).is_ok() {
                    source.found(&quads, Some((1, 1)));
                    status.show("Read the backup's QR code.");
                    status.finish_line();
                    return Ok(text.to_string());
                }
                status.show("A SeedLock QR code was seen, but it did not read correctly. Hold it steady.");
            } else if upper.starts_with("UR:") {
                status.show("That is a transaction's QR code, not a backup. Show the backup's QR code (made with menu 8).");
            } else {
                status.show("A QR code was seen, but it is not a SeedLock backup.");
            }
        }
    }
}

/// Read a multisig wallet's descriptor from Sparrow's QR code: a UR crypto-output (one code or an
/// animated series) or a descriptor as plain text. Other QR codes are passed over with a word.
pub fn scan_descriptor(source: &mut dyn FrameSource, give_up_after: Duration) -> Res<seedlock_core::multisig::MultisigWallet> {
    use seedlock_core::multisig::{DescriptorScanner, DescriptorUpdate};
    let started = Instant::now();
    let mut scanner = DescriptorScanner::new();
    let mut status = Status::new();
    let mut hint = UnreadHint::new();
    status.show("Camera is on. Looking for the wallet's QR code ...");
    loop {
        if started.elapsed() > give_up_after {
            status.finish_line();
            return fail(format!("No descriptor was read within {} minutes, so SeedLock stopped.\n{NO_CODE_TIP}", give_up_after.as_secs() / 60));
        }
        let Some(frame) = source.next_frame(Duration::from_millis(500))? else { continue };
        let found = find_qr_codes(&frame);
        if found.is_empty() {
            hint.nothing_read(&frame, &mut status);
            continue;
        }
        hint.reset();
        let quads: Vec<Quad> = found.iter().map(|f| f.quad).collect();
        source.found(&quads, None);
        for code in found {
            match scanner.add(&code.text) {
                DescriptorUpdate::Done(wallet) => {
                    source.found(&quads, Some((1, 1)));
                    status.show("Read the wallet's QR code.");
                    status.finish_line();
                    return Ok(wallet);
                }
                DescriptorUpdate::Progress { have, total } => {
                    source.found(&quads, Some((have, total)));
                    status.show(&format!("{} {have} of {total} parts received. Keep the camera on the code.", bar(have, total)));
                }
                DescriptorUpdate::Ignored(why) => status.show(&format!("A QR code was seen, but {why}.")),
                DescriptorUpdate::Refused(why) => {
                    status.finish_line();
                    return fail(format!("Sparrow's QR code was read, but that wallet cannot be registered: {why}"));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The camera, through ffmpeg
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Mode {
    width: usize,
    height: usize,
    framerate: Option<u32>,
    mjpeg: bool,
}

/// Picture sizes to try, best first (not every camera offers every size).
const MODES: [Mode; 3] = [
    Mode { width: 1280, height: 720, framerate: Some(30), mjpeg: true },
    Mode { width: 1280, height: 720, framerate: None, mjpeg: false },
    Mode { width: 640, height: 480, framerate: None, mjpeg: false },
];

/// The ffmpeg program: `SEEDLOCK_FFMPEG` if set, else `ffmpeg` from the PATH.
pub fn ffmpeg_program() -> OsString {
    std::env::var_os("SEEDLOCK_FFMPEG").filter(|v| !v.is_empty()).unwrap_or_else(|| "ffmpeg".into())
}

/// The ffplay program that shows the preview: `SEEDLOCK_FFPLAY` if set, else the one next to
/// the chosen ffmpeg, else `ffplay` from the PATH.
pub fn ffplay_program() -> OsString {
    if let Some(program) = std::env::var_os("SEEDLOCK_FFPLAY").filter(|v| !v.is_empty()) {
        return program;
    }
    let ffmpeg = ffmpeg_program();
    let path = Path::new(&ffmpeg);
    if path.file_stem().is_some_and(|s| s == "ffmpeg") && path.parent().is_some_and(|d| !d.as_os_str().is_empty()) {
        return path.with_file_name(if cfg!(windows) { "ffplay.exe" } else { "ffplay" }).into_os_string();
    }
    "ffplay".into()
}

/// Names of the cameras in the output of `ffmpeg -list_devices true -f dshow -i dummy` (Windows).
pub fn parse_dshow_video_devices(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| {
            let line = line.trim_end();
            if !line.ends_with("(video)") {
                return None;
            }
            let rest = &line[line.find('"')? + 1..];
            Some(rest[..rest.rfind('"')?].to_string())
        })
        .collect()
}

fn windows_camera_name(program: &OsStr, index: u32) -> Result<String, String> {
    let output = Command::new(program)
        .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    let names = parse_dshow_video_devices(&String::from_utf8_lossy(&output.stderr));
    names.get(index as usize).cloned().ok_or_else(|| format!("no camera number {index} was found (ffmpeg sees {} cameras)", names.len()))
}

/// The ffmpeg options that say which camera to read (they differ per operating system).
fn input_args(program: &OsStr, index: u32, mode: Mode) -> Result<Vec<OsString>, String> {
    let size = format!("{}x{}", mode.width, mode.height);
    let mut a: Vec<String> = Vec::new();
    if cfg!(target_os = "windows") {
        a.extend(["-f", "dshow", "-video_size"].map(String::from));
        a.push(size);
        if let Some(fps) = mode.framerate {
            a.extend(["-framerate".to_string(), fps.to_string()]);
        }
        a.extend(["-i".to_string(), format!("video={}", windows_camera_name(program, index)?)]);
    } else if cfg!(target_os = "macos") {
        a.extend(["-f", "avfoundation"].map(String::from));
        if let Some(fps) = mode.framerate {
            a.extend(["-framerate".to_string(), fps.to_string()]);
        }
        a.extend(["-video_size".to_string(), size, "-i".to_string(), index.to_string()]);
    } else {
        a.extend(["-f", "v4l2"].map(String::from));
        if mode.mjpeg {
            a.extend(["-input_format", "mjpeg"].map(String::from));
        }
        a.extend(["-video_size".to_string(), size]);
        if let Some(fps) = mode.framerate {
            a.extend(["-framerate".to_string(), fps.to_string()]);
        }
        a.extend(["-i".to_string(), format!("/dev/video{index}")]);
    }
    Ok(a.into_iter().map(OsString::from).collect())
}

/// Collect what a helper program writes to its error output (the last few lines).
pub(crate) fn collect_messages(stderr: std::process::ChildStderr) -> Arc<Mutex<String>> {
    let log = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&log);
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut text) = shared.lock() {
                if text.len() < 2000 {
                    text.push_str(&line);
                    text.push('\n');
                }
            }
        }
    });
    log
}

/// A running ffmpeg that sends camera pictures, and (when possible) an ffplay window that
/// shows them. Both are stopped when this is dropped.
pub struct Camera {
    child: Child,
    pictures: Receiver<Vec<u8>>,
    first: Option<Vec<u8>>,
    pub width: usize,
    pub height: usize,
    log: Arc<Mutex<String>>,
    shared: Arc<Shared>,
    window: Option<Child>,
    /// Why there is no preview window (`None`: there is one, or none was asked for).
    pub preview_note: Option<String>,
}

impl Camera {
    /// Open camera number `index` (0 is the first), with a preview window if `preview`.
    pub fn open(index: u32, preview: bool) -> Res<Camera> {
        Camera::open_with(&ffmpeg_program(), index, preview)
    }

    pub fn open_with(program: &OsStr, index: u32, preview: bool) -> Res<Camera> {
        let window = if preview && std::env::var_os("SEEDLOCK_NO_PREVIEW").is_none() {
            Some((ffplay_program(), std::env::var_os("SEEDLOCK_FFPLAY").is_none()))
        } else {
            None
        };
        let mut camera = Camera::open_full(program, index, window.as_ref().map(|(p, need)| (p.as_os_str(), *need)))?;
        if preview && window.is_none() {
            camera.preview_note = Some("it was switched off (SEEDLOCK_NO_PREVIEW)".to_string());
        }
        Ok(camera)
    }

    /// `preview`: the ffplay program, and whether a graphical screen must exist.
    fn open_full(program: &OsStr, index: u32, preview: Option<(&OsStr, bool)>) -> Res<Camera> {
        match Command::new(program).arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return fail(FFMPEG_HELP),
            Err(e) => return fail(format!("SeedLock cannot run ffmpeg: {e}")),
        }
        let mut last = String::new();
        for mode in MODES {
            match Camera::start(program, index, mode) {
                Ok(mut camera) => {
                    if let Some((ffplay, need_screen)) = preview {
                        if let Err(why) = camera.start_preview(ffplay, need_screen) {
                            camera.preview_note = Some(why);
                        }
                    }
                    return Ok(camera);
                }
                Err(e) => last = e,
            }
        }
        fail(format!("SeedLock could not get pictures from camera {index}.\n{}\n\n{CAMERA_TIPS}", last.trim()))
    }

    fn start(program: &OsStr, index: u32, mode: Mode) -> Result<Camera, String> {
        let input = input_args(program, index, mode)?;
        let mut child = Command::new(program)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(&input)
            .args(["-an", "-s"])
            .arg(format!("{}x{}", mode.width, mode.height))
            .args(["-pix_fmt", "gray", "-f", "rawvideo", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start ffmpeg: {e}"))?;
        let stdout = child.stdout.take().ok_or("ffmpeg gave no output")?;
        let stderr = child.stderr.take().ok_or("ffmpeg gave no messages")?;
        let log = collect_messages(stderr);
        let shared = Arc::new(Shared { overlay: Mutex::new(Overlay::default()), preview: Mutex::new(None) });

        // Only the newest picture matters: when the scanner is busy, older ones are dropped.
        let (send, pictures) = mpsc::sync_channel::<Vec<u8>>(1);
        let (width, height) = (mode.width, mode.height);
        let for_window = Arc::clone(&shared);
        thread::spawn(move || {
            let mut out = stdout;
            loop {
                let mut picture = vec![0u8; width * height];
                if out.read_exact(&mut picture).is_err() {
                    break; // ffmpeg ended
                }
                to_preview(&for_window, &picture, width, height);
                if let Err(TrySendError::Disconnected(_)) = send.try_send(picture) {
                    break;
                }
            }
        });

        match pictures.recv_timeout(Duration::from_secs(12)) {
            Ok(first) => Ok(Camera { child, pictures, first: Some(first), width, height, log, shared, window: None, preview_note: None }),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                thread::sleep(Duration::from_millis(200)); // let the error text arrive
                let text = log.lock().map(|t| t.trim().to_string()).unwrap_or_default();
                Err(if text.is_empty() { "no picture arrived within 12 seconds".to_string() } else { text })
            }
        }
    }

    /// Open the window that shows the camera pictures.
    fn start_preview(&mut self, ffplay: &OsStr, need_screen: bool) -> Result<(), String> {
        if need_screen && cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err("no graphical screen was found (the DISPLAY variable is not set)".to_string());
        }
        let mut child = Command::new(ffplay)
            .args(["-hide_banner", "-loglevel", "error", "-nostats", "-autoexit", "-f", "rawvideo", "-pixel_format", "gray", "-video_size"])
            .arg(format!("{}x{}", self.width, self.height))
            .args(["-framerate", "30", "-window_title", PREVIEW_TITLE, "-x", "800", "-i", "pipe:0"])
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
        let stdin = child.stdin.take().ok_or("ffplay gave no input")?;
        let stderr = child.stderr.take().ok_or("ffplay gave no messages")?;
        let log = collect_messages(stderr);
        let (send, pictures) = mpsc::sync_channel::<Vec<u8>>(1);
        thread::spawn(move || {
            let mut input = stdin;
            for picture in pictures {
                if input.write_all(&picture).is_err() {
                    break; // the window was closed
                }
            }
        });
        // If ffplay cannot open a window it stops within a moment: say why.
        thread::sleep(Duration::from_millis(600));
        if let Ok(Some(_)) = child.try_wait() {
            thread::sleep(Duration::from_millis(100));
            let text = log.lock().map(|t| t.trim().to_string()).unwrap_or_default();
            return Err(if text.is_empty() { "ffplay stopped at once".to_string() } else { text });
        }
        if let Ok(mut slot) = self.shared.preview.lock() {
            *slot = Some(send);
        }
        self.window = Some(child);
        Ok(())
    }

    fn messages(&self) -> String {
        self.log.lock().map(|t| t.trim().to_string()).unwrap_or_default()
    }
}

impl FrameSource for Camera {
    fn next_frame(&mut self, wait: Duration) -> Res<Option<Frame>> {
        let luma = match self.first.take() {
            Some(first) => first,
            None => match self.pictures.recv_timeout(wait) {
                Ok(picture) => picture,
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    return fail(format!("The camera stopped sending pictures.\n{}\n{CAMERA_TIPS}", self.messages()));
                }
            },
        };
        Ok(Some(Frame { width: self.width, height: self.height, luma }))
    }

    fn found(&mut self, quads: &[Quad], progress: Option<(usize, usize)>) {
        if let Ok(mut overlay) = self.shared.overlay.lock() {
            if !quads.is_empty() {
                overlay.quads = quads.to_vec();
                overlay.seen = Some(Instant::now());
            }
            if progress.is_some() {
                overlay.progress = progress;
            }
        }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.shared.preview.lock() {
            *slot = None;
        }
        if let Some(window) = self.window.as_mut() {
            let _ = window.kill();
            let _ = window.wait();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The camera check: opens the camera and its preview window, counts pictures and tries to
/// read a QR code that is held in front of it.
pub fn camera_test(index: u32) -> Res<()> {
    let program = ffmpeg_program();
    eprintln!("Testing camera {index}.\n");
    if let Ok(out) = Command::new(&program).arg("-version").stdin(Stdio::null()).output() {
        if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
            eprintln!("ffmpeg found: {line}");
        }
    }
    let mut camera = Camera::open_with(&program, index, true)?;
    eprintln!("Camera opened: pictures of {} x {} pixels.", camera.width, camera.height);
    match &camera.preview_note {
        None => eprintln!("A window shows what the camera sees."),
        Some(why) => eprintln!("There is no preview window: {why}."),
    }
    eprintln!("Hold a QR code in front of the camera, for example the animated code in Sparrow.");
    eprintln!("The test ends as soon as a QR code is read, or after 20 seconds.");
    let started = Instant::now();
    let (mut pictures, mut seen) = (0u32, None::<String>);
    while started.elapsed() < Duration::from_secs(20) && seen.is_none() {
        if let Some(frame) = camera.next_frame(Duration::from_millis(500))? {
            pictures += 1;
            let codes = find_qr_codes(&frame);
            if !codes.is_empty() {
                let quads: Vec<Quad> = codes.iter().map(|c| c.quad).collect();
                camera.found(&quads, None);
                seen = codes.into_iter().next().map(|c| c.text);
                thread::sleep(Duration::from_millis(600)); // let the outline show in the window
            }
        }
    }
    let seconds = started.elapsed().as_secs_f32().max(0.1);
    eprintln!("\nReceived {pictures} pictures in {seconds:.0} seconds (about {:.0} per second).", pictures as f32 / seconds);
    if pictures == 0 {
        return fail(format!("The camera opened but sent no pictures.\n{}\n{CAMERA_TIPS}", camera.messages()));
    }
    match seen {
        Some(text) => eprintln!("A QR code was read ({} characters).", text.len()),
        None => eprintln!("No QR code was read. That is normal if none was in front of the camera."),
    }
    eprintln!("\nThe camera works. When you sign a transaction, choose \"With the camera\".");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcode::{types::{Color, Version}, EcLevel, QrCode};
    // (reading QR codes in camera pictures is tested in the seedlock-qr crate)
    use seedlock_core::bitcoin::{absolute::LockTime, transaction::Version as TxVersion, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};

    /// A grey picture like a webcam sees a screen: the code somewhere in the picture, a
    /// grey surrounding, soft focus (`blur` passes of a 3x3 average) and sensor noise.
    fn camera_picture(text: &str, width: usize, height: usize, px_per_module: usize, blur: usize, seed: u64) -> Frame {
        let code = QrCode::with_version(text.as_bytes(), Version::Normal(7), EcLevel::L).unwrap();
        camera_picture_of(&code, width, height, px_per_module, blur, seed)
    }

    fn camera_picture_of(code: &QrCode, width: usize, height: usize, px_per_module: usize, blur: usize, seed: u64) -> Frame {
        let n = code.width();
        let colors = code.to_colors();
        let quiet = 4;
        let side = (n + 2 * quiet) * px_per_module;
        let (x0, y0) = ((width - side) / 3, (height - side) / 2);
        let mut luma = vec![190u8; width * height];
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = (x / px_per_module, y / px_per_module);
                let dark = mx >= quiet && mx < quiet + n && my >= quiet && my < quiet + n && colors[(my - quiet) * n + mx - quiet] == Color::Dark;
                luma[(y0 + y) * width + x0 + x] = if dark { 30 } else { 235 };
            }
        }
        for _ in 0..blur {
            let src = luma.clone();
            for y in 1..height - 1 {
                for x in 1..width - 1 {
                    let mut sum = 0u32;
                    for dy in 0..3 {
                        for dx in 0..3 {
                            sum += u32::from(src[(y + dy - 1) * width + x + dx - 1]);
                        }
                    }
                    luma[y * width + x] = (sum / 9) as u8;
                }
            }
        }
        let mut state = seed | 1;
        for v in luma.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let noise = ((state >> 33) % 17) as i32 - 8;
            *v = (i32::from(*v) + noise).clamp(0, 255) as u8;
        }
        Frame { width, height, luma }
    }

    fn sample_psbt() -> Psbt {
        let tx = Transaction {
            version: TxVersion::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: (0..8).map(|i| TxOut { value: Amount::from_sat(1000 + i), script_pubkey: ScriptBuf::from(vec![0x51 + (i as u8 % 8); 34]) }).collect(),
        };
        Psbt::from_unsigned_tx(tx).unwrap()
    }

    struct Pictures(std::vec::IntoIter<Frame>);
    impl FrameSource for Pictures {
        fn next_frame(&mut self, _wait: Duration) -> Res<Option<Frame>> {
            match self.0.next() {
                Some(f) => Ok(Some(f)),
                None => fail("the camera stopped"),
            }
        }
    }

    #[test]
    fn the_overlay_outlines_codes_and_fills_the_progress_bar() {
        let (w, h) = (320usize, 240usize);
        let mut picture = vec![128u8; w * h];
        let quad: Quad = [(100, 60), (200, 60), (200, 160), (100, 160)];
        draw_overlay(&mut picture, w, h, &[quad], Some((3, 10)));
        // the outline lies 7% outside the code: its top line is at y = 57 (the code starts at 60) and its
        // left line at x = 97 (the code starts at 100), white (2 px each side) with a black edge (4 px)
        let at = |x: usize, y: usize| picture[y * w + x];
        assert_eq!(at(150, 57), 255, "white line above the code");
        assert_eq!(at(150, 53), 0, "black edge outside it");
        assert_eq!(at(150, 50), 128, "the picture beyond the outline is untouched");
        assert_eq!(at(150, 110), 128, "the picture inside is untouched");
        assert_eq!(at(97, 110), 255, "white line left of the code");
        assert_eq!(at(93, 110), 0, "black edge outside it");
        // the bar: 8 px high at the bottom, filled for 3 of 10 parts
        assert_eq!(at(20, h - 4), 255);
        assert_eq!(at(w - 20, h - 4), 0);
        let filled = (0..w).filter(|&x| at(x, h - 4) == 255).count();
        assert!((90..=100).contains(&filled), "{filled}");
        // no code, no progress: nothing is drawn
        let mut plain = vec![128u8; w * h];
        draw_overlay(&mut plain, w, h, &[], None);
        assert!(plain.iter().all(|&v| v == 128));
    }

    #[test]
    fn collects_a_whole_transaction_from_pictures_in_any_order() {
        let psbt = sample_psbt();
        let frames = seedlock_core::psbt::ur_frames(&psbt).unwrap();
        assert!(frames.len() > 4, "a multi-part code");
        // start in the middle of the loop and see every picture twice, in upper case
        let pictures: Vec<Frame> = frames
            .iter()
            .cycle()
            .skip(3)
            .take(frames.len() * 2)
            .enumerate()
            .map(|(i, f)| camera_picture(&f.to_uppercase(), 640, 480, 6, 1, i as u64))
            .collect();
        let got = scan_psbt(&mut Pictures(pictures.into_iter()), Duration::from_secs(60)).unwrap();
        assert_eq!(got, psbt);
    }

    #[test]
    fn gives_a_clear_error_when_the_camera_stops_or_ffmpeg_is_missing() {
        let e = scan_psbt(&mut Pictures(Vec::new().into_iter()), Duration::from_secs(5)).map(|_| ()).unwrap_err();
        assert!(e.to_string().contains("camera stopped"));
        let missing = match Camera::open_with(OsStr::new("/nonexistent/ffmpeg-not-here"), 0, true) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("there is no such program"),
        };
        assert!(missing.contains("ffmpeg") && missing.contains("SEEDLOCK_FFMPEG") && missing.contains("pacman"), "{missing}");
    }

    /// The text of a real backup (with tiny key-stretching settings, so that making it is quick).
    fn backup_text() -> String {
        let quick = backup::KdfParams { m_cost_kib: 64, t_cost: 1, parallelism: 1 };
        backup::to_text(&backup::encrypt(&[7u8; 16], "a test password", quick).unwrap())
    }

    #[test]
    fn reads_a_backup_qr_code_and_passes_over_other_codes() {
        let text = backup_text();
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).unwrap();
        let pictures = vec![
            camera_picture("UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ", 640, 480, 6, 1, 1), // a transaction's code
            camera_picture("HTTPS://EXAMPLE.COM/SOMETHING-ELSE", 640, 480, 6, 1, 2), // some other code
            camera_picture_of(&code, 640, 480, 6, 1, 3),                           // the backup, soft and noisy
        ];
        let got = scan_backup_text(&mut Pictures(pictures.into_iter()), Duration::from_secs(60)).unwrap();
        assert_eq!(got, text);
        assert!(backup::from_text(&got).is_ok());
    }

    #[test]
    fn a_damaged_backup_code_is_never_taken() {
        let good = backup_text();
        let mut chars: Vec<char> = good.chars().collect();
        chars[30] = if chars[30] == 'A' { 'B' } else { 'A' }; // one changed character: the checksum no longer fits
        let bad: String = chars.into_iter().collect();
        assert!(backup::from_text(&bad).is_err());
        let code = QrCode::with_error_correction_level(bad.as_bytes(), EcLevel::M).unwrap();
        let pictures = vec![camera_picture_of(&code, 640, 480, 6, 1, 4)];
        let e = scan_backup_text(&mut Pictures(pictures.into_iter()), Duration::from_secs(60)).map(|_| ()).unwrap_err();
        assert!(e.to_string().contains("camera stopped"), "{e}");
    }

    #[test]
    fn lists_windows_cameras() {
        let listing = "[dshow @ 000001a2b3c4] DirectShow video devices (some may be both video and audio devices)\n\
                       [dshow @ 000001a2b3c4]  \"Integrated Camera\" (video)\n\
                       [dshow @ 000001a2b3c4]   Alternative name \"@device_pnp_\\\\?\\usb#vid_0c45\"\n\
                       [dshow @ 000001a2b3c4]  \"OBS Virtual Camera\" (video)\n\
                       [dshow @ 000001a2b3c4] DirectShow audio devices\n\
                       [dshow @ 000001a2b3c4]  \"Microphone (Realtek Audio)\" (audio)\n";
        assert_eq!(parse_dshow_video_devices(listing), ["Integrated Camera", "OBS Virtual Camera"]);
    }

    /// Stand-ins for ffmpeg (a camera) and ffplay (the preview window): scripts that behave
    /// like the real programs at the pipe level.
    #[cfg(unix)]
    fn fake_programs(name: &str, ffplay_script: &str) -> (std::path::PathBuf, std::path::PathBuf, Psbt, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let (w, h) = (1280, 720);
        let psbt = sample_psbt();
        let frames = seedlock_core::psbt::ur_frames(&psbt).unwrap();
        let dir = std::env::temp_dir().join(format!("seedlock-fake-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut raw = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            raw.extend(camera_picture(&f.to_uppercase(), w, h, 7, 1, i as u64).luma);
        }
        std::fs::write(dir.join("pictures.raw"), &raw).unwrap();
        let ffmpeg = dir.join("ffmpeg");
        std::fs::write(
            &ffmpeg,
            format!(
                "#!/bin/sh\n\
                 for a in \"$@\"; do [ \"$a\" = \"-version\" ] && {{ echo 'ffmpeg version test'; exit 0; }}; done\n\
                 i=0\nwhile :; do\n  dd if='{}' bs={} skip=$((i % {})) count=1 2>/dev/null || exit 1\n  i=$((i+1)); sleep 0.05\ndone\n",
                dir.join("pictures.raw").display(),
                w * h,
                frames.len()
            ),
        )
        .unwrap();
        let ffplay = dir.join("ffplay");
        std::fs::write(&ffplay, ffplay_script.replace("{DIR}", &dir.display().to_string())).unwrap();
        for f in [&ffmpeg, &ffplay] {
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (ffmpeg, ffplay, psbt, dir)
    }

    /// The whole camera path with a stand-in for ffmpeg: a script that sends the same raw
    /// grey pictures a real ffmpeg would send through the pipe.
    #[cfg(unix)]
    #[test]
    fn reads_pictures_from_an_ffmpeg_process() {
        let (ffmpeg, _, psbt, dir) = fake_programs("cam", "#!/bin/sh\nexit 0\n");
        let mut camera = Camera::open_with(ffmpeg.as_os_str(), 0, false).unwrap();
        assert_eq!((camera.width, camera.height), (1280, 720));
        assert!(camera.preview_note.is_none(), "no preview was asked for");
        let got = scan_psbt(&mut camera, Duration::from_secs(60)).unwrap();
        assert_eq!(got, psbt);
        drop(camera);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn the_preview_window_gets_every_picture_with_the_overlay() {
        let ffplay = "#!/bin/sh\necho \"$@\" > '{DIR}/ffplay.args'\nexec cat > '{DIR}/ffplay.received'\n";
        let (ffmpeg, ffplay_path, psbt, dir) = fake_programs("window", ffplay);
        let mut camera = Camera::open_full(ffmpeg.as_os_str(), 0, Some((ffplay_path.as_os_str(), false))).unwrap();
        assert!(camera.preview_note.is_none(), "{:?}", camera.preview_note);
        // scanning tells the window what it found; the window keeps getting pictures meanwhile
        let got = scan_psbt(&mut camera, Duration::from_secs(60)).unwrap();
        assert_eq!(got, psbt);
        thread::sleep(Duration::from_millis(300));
        drop(camera); // closes the window
        let args = std::fs::read_to_string(dir.join("ffplay.args")).unwrap();
        for wanted in ["rawvideo", "gray", "1280x720", "pipe:0", "SeedLock camera", "-autoexit"] {
            assert!(args.contains(wanted), "{wanted} missing in: {args}");
        }
        let received = std::fs::read(dir.join("ffplay.received")).unwrap();
        let picture = 1280 * 720;
        let whole = received.len() / picture;
        assert!(whole >= 4, "only {whole} pictures reached the window");
        // Closing the window can cut off the picture being written at that moment (harmless: the window
        // is going away); every picture before it is whole, which the overlay check below relies on: the
        // progress bar lands along the bottom edge only if the pictures line up.
        // the last pictures carry the progress bar: a black strip with white in it along the bottom
        let last = &received[(whole - 1) * picture..whole * picture];
        let bottom = &last[(720 - 3) * 1280..(720 - 2) * 1280];
        assert!(bottom.iter().any(|&v| v == 255) && bottom.iter().any(|&v| v < 60), "no progress bar in the last picture");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_or_failing_preview_never_stops_the_scan() {
        let failing = "#!/bin/sh\necho 'Could not initialize SDL - No available video device' >&2\nexit 1\n";
        let (ffmpeg, ffplay_path, psbt, dir) = fake_programs("nowindow", failing);
        let mut camera = Camera::open_full(ffmpeg.as_os_str(), 0, Some((ffplay_path.as_os_str(), false))).unwrap();
        assert!(camera.preview_note.as_deref().unwrap_or("").contains("No available video device"), "{:?}", camera.preview_note);
        assert_eq!(scan_psbt(&mut camera, Duration::from_secs(60)).unwrap(), psbt);
        drop(camera);
        let mut camera = Camera::open_full(ffmpeg.as_os_str(), 0, Some((OsStr::new("/nonexistent/ffplay"), false))).unwrap();
        assert!(camera.preview_note.as_deref().unwrap_or("").contains("ffplay"), "{:?}", camera.preview_note);
        assert_eq!(scan_psbt(&mut camera, Duration::from_secs(60)).unwrap(), psbt);
        drop(camera);
        // without a graphical screen the window is not even tried
        let mut camera = Camera::open_full(ffmpeg.as_os_str(), 0, Some((ffplay_path.as_os_str(), true))).unwrap();
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            assert!(camera.preview_note.as_deref().unwrap_or("").contains("graphical screen"), "{:?}", camera.preview_note);
        }
        assert_eq!(scan_psbt(&mut camera, Duration::from_secs(60)).unwrap(), psbt);
        drop(camera);
        std::fs::remove_dir_all(&dir).ok();
    }
}
