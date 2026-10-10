//! Terminal helpers: prompts (with hidden input), screen clearing, the word
//! grid and QR codes.
//!
//! Everything meant for a human goes to **stderr**; **stdout** is reserved for
//! data, so `seedlock decrypt --raw > words.txt` or `$(seedlock verify ...)`
//! behave as scripts expect.

use qrcode::{render::svg, render::unicode, types::{Color, Version}, EcLevel, QrCode};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use zeroize::Zeroizing;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub fn fail<T>(msg: impl Into<String>) -> Res<T> {
    Err(msg.into().into())
}

/// True when a person is typing at a terminal (as opposed to input being piped in).
pub fn interactive() -> bool {
    io::stdin().is_terminal()
}

fn show_prompt(prompt: &str) {
    if !prompt.is_empty() {
        eprint!("{prompt}");
        let _ = io::stderr().flush();
    }
}

/// Read one visible line.
pub fn read_line(prompt: &str) -> Res<String> {
    show_prompt(prompt);
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return fail("unexpected end of input");
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Read a secret: hidden while typing at a terminal, one plain line when piped.
pub fn secret(prompt: &str) -> Res<Zeroizing<String>> {
    show_prompt(prompt);
    if interactive() {
        Ok(Zeroizing::new(read_hidden()?))
    } else {
        let mut line = Zeroizing::new(String::new());
        if io::stdin().lock().read_line(&mut line)? == 0 {
            return fail("unexpected end of input (a secret was expected on stdin)");
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        Ok(line)
    }
}

/// One hidden line from the terminal, read by rpassword. While it reads, the terminal shows
/// nothing that is typed; on Ctrl+C, rpassword sends SIGINT itself before it has put the terminal
/// back, and with SIGINT's usual action SeedLock would end right there, leaving the terminal
/// showing nothing that is typed afterwards. So SIGINT is ignored while it reads: rpassword then
/// puts the terminal back and returns "interrupted", and only then does SeedLock end, the way
/// Ctrl+C always ends it.
#[cfg(unix)]
#[allow(unsafe_code)]
fn read_hidden() -> io::Result<String> {
    // SAFETY: signal() and raise() with a valid signal number and valid handlers; the handler
    // that was there before is put back before anything else happens.
    let previous = unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
    let read = rpassword::read_password();
    if previous != libc::SIG_ERR {
        unsafe { libc::signal(libc::SIGINT, previous) };
    }
    if matches!(&read, Err(e) if e.kind() == io::ErrorKind::Interrupted) {
        unsafe { libc::raise(libc::SIGINT) };
    }
    read
}

#[cfg(not(unix))]
fn read_hidden() -> io::Result<String> {
    rpassword::read_password()
}

/// Yes/no question. When input is piped and runs out, the default is used.
pub fn confirm(prompt: &str, default: bool) -> Res<bool> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let answer = match read_line(&format!("{prompt} {hint} ")) {
            Ok(a) => a,
            Err(_) => return Ok(default),
        };
        match answer.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                eprintln!("Please answer y or n.");
                if !interactive() {
                    return Ok(default);
                }
            }
        }
    }
}

/// "Press Enter to continue" (skipped when input is piped).
pub fn pause(prompt: &str) {
    if interactive() {
        let _ = read_line(prompt);
    }
}

/// Wipe the visible screen *and* the scrollback so secrets do not linger.
pub fn clear_screen() {
    if io::stderr().is_terminal() {
        eprint!("\x1b[2J\x1b[3J\x1b[H");
        let _ = io::stderr().flush();
    }
}

/// Numbered grid of words, three per row.
pub fn print_words(words: &[&str]) {
    for (row, chunk) in words.chunks(3).enumerate() {
        let mut line = String::new();
        for (col, word) in chunk.iter().enumerate() {
            line.push_str(&format!("  {:>2}. {:<10}", row * 3 + col + 1, word));
        }
        eprintln!("{}", line.trim_end());
    }
}

/// Break `text` into lines of at most `width` characters, breaking only at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Print `title` and `paragraphs` inside a bordered box, a fixed 74 columns wide (fits an
/// ordinary 80-column window), so that it cannot be mistaken for one more line of scrolling
/// text. The border and title are bold; the border alone does not rely on colour, so it is
/// just as visible on a light or a dark background.
pub fn boxed_note(title: &str, paragraphs: &[&str]) {
    let w = 74;
    let bar = "─".repeat(w + 2);
    eprintln!("\n\x1b[1m┌{bar}┐");
    eprintln!("│ {title:<w$} │\x1b[0m");
    for para in paragraphs {
        eprintln!("\x1b[1m│\x1b[0m{}\x1b[1m│\x1b[0m", " ".repeat(w + 2));
        for line in wrap(para, w) {
            eprintln!("\x1b[1m│\x1b[0m {line:<w$} \x1b[1m│\x1b[0m");
        }
    }
    eprintln!("\x1b[1m└{bar}┘\x1b[0m");
}

/// True when animated output makes sense: a person at a terminal.
pub fn can_animate() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

/// One key press, see [`KeyReader`].
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    /// Ctrl+C, Ctrl+D or the end of the input.
    Cancel,
    Other,
}

/// Reads single key presses, without waiting for Enter and without showing them (a Unix
/// terminal is switched into that mode while this exists and put back when it is dropped).
/// Used for the dice counter, which must go up at the moment a roll is typed.
#[cfg(unix)]
pub struct KeyReader {
    original: libc::termios,
    pending: Option<u8>,
}

#[cfg(unix)]
impl KeyReader {
    /// `None` if the input is not a terminal.
    #[allow(unsafe_code)]
    pub fn new() -> Option<KeyReader> {
        if !io::stdin().is_terminal() {
            return None;
        }
        // SAFETY: plain termios calls on standard input, with a struct that is filled in by
        // tcgetattr before it is used.
        unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut term) != 0 {
                return None;
            }
            let original = term;
            // Ctrl+C arrives as a key (ISIG off) so that the terminal is always put back before exiting.
            term.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
            term.c_cc[libc::VMIN] = 1;
            term.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &term) != 0 {
                return None;
            }
            Some(KeyReader { original, pending: None })
        }
    }

    fn byte(&mut self) -> Option<u8> {
        if let Some(b) = self.pending.take() {
            return Some(b);
        }
        let mut one = [0u8; 1];
        loop {
            match io::stdin().lock().read(&mut one) {
                Ok(1) => return Some(one[0]),
                Ok(_) => return None, // end of input
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            }
        }
    }

    pub fn key(&mut self) -> Key {
        let Some(b) = self.byte() else { return Key::Cancel };
        match b {
            b'\r' | b'\n' => Key::Enter,
            0x7f | 0x08 => Key::Backspace,
            0x03 | 0x04 => Key::Cancel,
            0x1b => {
                // An escape sequence (arrow keys, Home, ...): skip all of it, so that the digits
                // inside it are not taken for dice rolls.
                match self.byte() {
                    Some(b'[') | Some(b'O') => {
                        for _ in 0..8 {
                            match self.byte() {
                                Some(c) if (0x40..=0x7e).contains(&c) => break,
                                Some(_) => {}
                                None => break,
                            }
                        }
                    }
                    Some(other) => self.pending = Some(other),
                    None => {}
                }
                Key::Other
            }
            0x20..=0x7e => Key::Char(b as char),
            _ => Key::Other,
        }
    }
}

#[cfg(unix)]
impl Drop for KeyReader {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: puts back the terminal settings that `new` saved.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }
}

/// Windows: the console's own single-key mode. While a KeyReader exists, the console is out of
/// line input and echo (it is put back when the reader is dropped); with virtual-terminal input on,
/// arrow keys arrive as the same escape sequences as on Unix and are skipped the same way.
///
/// This is compiled on Windows only; `--cfg seedlock_check_windows` also type-checks it elsewhere
/// (cargo check), which is how it is checked on Linux, since that cannot run it.
#[cfg(any(windows, seedlock_check_windows))]
#[cfg_attr(not(windows), allow(dead_code))]
mod windows_console {
    use super::Key;
    use std::ffi::c_void;

    type Handle = *mut c_void;
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    const ENABLE_LINE_INPUT: u32 = 0x0002;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
    const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> Handle;
        fn GetConsoleMode(console: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(console: Handle, mode: u32) -> i32;
        fn ReadConsoleW(console: Handle, buffer: *mut u16, to_read: u32, read: *mut u32, control: *mut c_void) -> i32;
    }

    fn usable(handle: Handle) -> bool {
        !handle.is_null() && handle as isize != -1
    }

    pub struct KeyReader {
        input: Handle,
        original: u32,
        pending: Option<u16>,
    }

    impl KeyReader {
        /// `None` if the input is not a console window (piped input, for example).
        #[allow(unsafe_code)]
        pub fn new() -> Option<KeyReader> {
            // SAFETY: plain console calls on this process's standard input; `original` is written
            // by GetConsoleMode before it is used.
            unsafe {
                let input = GetStdHandle(STD_INPUT_HANDLE);
                let mut original = 0u32;
                if !usable(input) || GetConsoleMode(input, &mut original) == 0 {
                    return None;
                }
                // Ctrl+C arrives as a key (processed input off), so the console is always put back first.
                let raw = original & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
                if SetConsoleMode(input, raw | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 && SetConsoleMode(input, raw) == 0 {
                    return None;
                }
                Some(KeyReader { input, original, pending: None })
            }
        }

        #[allow(unsafe_code)]
        fn unit(&mut self) -> Option<u16> {
            if let Some(unit) = self.pending.take() {
                return Some(unit);
            }
            let (mut unit, mut read) = (0u16, 0u32);
            // SAFETY: reads at most one UTF-16 unit into `unit`; `read` receives how many were read.
            let ok = unsafe { ReadConsoleW(self.input, &mut unit, 1, &mut read, std::ptr::null_mut()) };
            if ok == 0 || read == 0 {
                None
            } else {
                Some(unit)
            }
        }

        pub fn key(&mut self) -> Key {
            let Some(unit) = self.unit() else { return Key::Cancel };
            match unit {
                13 | 10 => Key::Enter,
                8 | 0x7f => Key::Backspace,
                3 | 4 | 26 => Key::Cancel, // Ctrl+C, Ctrl+D, Ctrl+Z
                0x1b => {
                    // An escape sequence (arrow keys, Home, ...): skip all of it, so that the digits
                    // inside it are not taken for dice rolls.
                    match self.unit() {
                        Some(0x5b) | Some(0x4f) => {
                            for _ in 0..8 {
                                match self.unit() {
                                    Some(c) if (0x40..=0x7e).contains(&c) => break,
                                    Some(_) => {}
                                    None => break,
                                }
                            }
                        }
                        Some(other) => self.pending = Some(other),
                        None => {}
                    }
                    Key::Other
                }
                0x20..=0x7e => Key::Char(unit as u8 as char),
                _ => Key::Other,
            }
        }
    }

    impl Drop for KeyReader {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: puts back the console mode that `new` saved.
            unsafe {
                SetConsoleMode(self.input, self.original);
            }
        }
    }

    /// Let the console window understand the terminal codes SeedLock writes (colours, moving the
    /// cursor, clearing a line). Windows Terminal does already; the older console window must be asked.
    #[allow(unsafe_code)]
    pub fn enable_terminal_codes() {
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: plain console calls; a handle that is not a console is left alone.
            unsafe {
                let handle = GetStdHandle(which);
                let mut mode = 0u32;
                if usable(handle) && GetConsoleMode(handle, &mut mode) != 0 {
                    SetConsoleMode(handle, mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use windows_console::{enable_terminal_codes, KeyReader};

/// Elsewhere there is no live key reading: callers fall back to whole lines.
#[cfg(not(any(unix, windows)))]
pub struct KeyReader;

#[cfg(not(any(unix, windows)))]
impl KeyReader {
    pub fn new() -> Option<KeyReader> {
        None
    }

    pub fn key(&mut self) -> Key {
        Key::Cancel
    }
}

/// Open a file with the program the computer normally uses for it (a web page opens in the
/// browser). Nothing is downloaded: this only starts the local program. Returns false when
/// that did not work (no desktop, no browser, ...), so the caller can show another way.
pub fn open_with_default_app(path: &Path) -> bool {
    if std::env::var_os("SEEDLOCK_NO_BROWSER").is_some() {
        return false;
    }
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let Ok(mut child) = Command::new(program).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    // These commands return quickly; a failure shows up as a non-zero exit. (explorer
    // reports 1 even when it worked.) If it is still running, the program has started.
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(status)) => return status.success() || cfg!(target_os = "windows"),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_) => return true,
        }
    }
    true
}

/// `~/x` -> `/home/you/x` (a terminal only does this for the shell, not for typed answers).
pub fn expand_home(path: &str) -> String {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return format!("{}{}", home.to_string_lossy(), &path[1..]);
        }
    }
    path.to_string()
}

/// Size of the terminal window as (lines, columns), when it can be found out.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn terminal_size() -> Option<(usize, usize)> {
    use std::os::fd::AsRawFd;
    let mut size = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: TIOCGWINSZ only fills in the `winsize` we point it to.
    let ok = unsafe { libc::ioctl(io::stderr().as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == 0;
    (ok && size.ws_row > 0 && size.ws_col > 0).then(|| (usize::from(size.ws_row), usize::from(size.ws_col)))
}

#[cfg(not(unix))]
pub fn terminal_size() -> Option<(usize, usize)> {
    None
}

/// Columns a line takes on screen (colour codes take none).
fn visible_width(line: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in line.chars() {
        if in_escape {
            in_escape = !c.is_ascii_alphabetic();
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            width += 1;
        }
    }
    width
}

/// (lines, columns) that a pre-drawn block takes on screen.
pub fn block_size(block: &str) -> (usize, usize) {
    (block.lines().count(), block.lines().map(visible_width).max().unwrap_or(0))
}

/// Make sure the window can show `lines` x `columns`; if it cannot, ask the person to
/// maximize it (an animation drawn in a window that is too small turns into a mess).
/// Returns false if they choose to skip.
pub fn wait_for_room(lines: usize, columns: usize, what: &str) -> Res<bool> {
    let mut asked = false;
    loop {
        let Some((rows, cols)) = terminal_size() else { return Ok(true) };
        if rows >= lines && cols >= columns {
            return Ok(true);
        }
        eprintln!("\nYour window is {cols} columns x {rows} lines. The {what} needs at least {columns} columns x {lines} lines.");
        if asked {
            // Maximizing was not enough (a small screen, or large text): zooming out makes room.
            eprintln!(
                "It is still too small. If the window is already maximized, zoom out with {} and - (a few times), then press Enter. Type q and press Enter to skip.",
                zoom_key()
            );
        } else {
            eprintln!("Maximize this window first, then press Enter to show the QR code. Type q and press Enter to skip.");
        }
        asked = true;
        if read_line("> ")?.trim().eq_ignore_ascii_case("q") {
            return Ok(false);
        }
    }
}

/// How a QR code is drawn in the terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrStyle {
    /// Two rows of the code per line of text, with the half-block characters ▀ ▄ █: half the
    /// size, and read without trouble on Linux and in iTerm2.
    Compact,
    /// One row of the code per line of text, every square two spaces with a black or white
    /// background and no block characters at all: twice as tall and twice as wide, but it does
    /// not depend on how a terminal draws block characters.
    Full,
}

/// True in Apple's own Terminal app, which says so (TERM_PROGRAM) to every program it starts.
pub fn is_apple_terminal() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("Apple_Terminal"))
}

/// The style for this terminal: `SEEDLOCK_QR_STYLE` (full or compact) when set, else full in
/// Apple's Terminal and compact everywhere else. Apple's Terminal draws block characters a
/// little shorter than a line, which leaves a thin white line through the black parts at the end
/// of every line of text: hard to notice, but a camera cannot read the code. That was measured
/// on a photo from a real Mac: filling in those thin lines alone made the photo readable.
pub fn qr_style() -> QrStyle {
    let chosen = std::env::var("SEEDLOCK_QR_STYLE").ok();
    let program = std::env::var("TERM_PROGRAM").ok();
    qr_style_from(chosen.as_deref(), program.as_deref())
}

fn qr_style_from(chosen: Option<&str>, term_program: Option<&str>) -> QrStyle {
    match chosen.map(|c| c.trim().to_ascii_lowercase()).as_deref() {
        Some("full") => QrStyle::Full,
        Some("compact") => QrStyle::Compact,
        _ if term_program.is_some_and(|p| p.eq_ignore_ascii_case("Apple_Terminal")) => QrStyle::Full,
        _ => QrStyle::Compact,
    }
}

/// The key that zooms the text of a terminal window: Cmd on a Mac, Ctrl elsewhere.
pub fn zoom_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

/// One QR code for the terminal, in the style this terminal needs ([`qr_style`]): always pure
/// black on pure white, with a white border, so it scans on dark and light themes alike.
pub fn paint(code: &QrCode) -> String {
    match qr_style() {
        QrStyle::Compact => paint_compact(code),
        QrStyle::Full => paint_full(code),
    }
}

const DARK_BACKGROUND: &str = "\x1b[48;5;16m";
const LIGHT_BACKGROUND: &str = "\x1b[48;5;231m";

/// The full style: one line of text per row of the code, with the usual white border of four
/// squares all around, and every square two spaces coloured only by their background. A terminal
/// paints a background across the whole height of the line (in the same photo from Apple's
/// Terminal, the white border, drawn exactly this way, is unbroken), so nothing is left that
/// could come out too short. The colour is only switched where it changes.
fn paint_full(code: &QrCode) -> String {
    const QUIET: usize = 4;
    let n = code.width();
    let colors = code.to_colors();
    let side = n + 2 * QUIET;
    let dark = |x: usize, y: usize| {
        (QUIET..QUIET + n).contains(&x) && (QUIET..QUIET + n).contains(&y) && colors[(y - QUIET) * n + x - QUIET] == Color::Dark
    };
    let mut out = String::with_capacity(side * (4 * side + 8));
    for y in 0..side {
        let mut current = None;
        for x in 0..side {
            let is_dark = dark(x, y);
            if current != Some(is_dark) {
                out.push_str(if is_dark { DARK_BACKGROUND } else { LIGHT_BACKGROUND });
                current = Some(is_dark);
            }
            out.push_str("  ");
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

/// The compact style: half-block characters, forced to black-on-white. This uses the 256-colour
/// palette's fixed black (16) and white (231) rather than the ordinary 8-colour codes (30/47): a
/// person's colour theme can (and often does, for a softer look) repaint "black" and "white" as
/// something other than pure black and white, which would quietly lower the contrast of a code
/// that most needs it. 256-colour support has been standard on every terminal capable of the
/// Unicode characters used here for well over a decade, so this is not a compatibility risk.
fn paint_compact(code: &QrCode) -> String {
    let art = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
    let mut out = String::new();
    for line in art.lines() {
        out.push_str("\x1b[38;5;16;48;5;231m");
        out.push_str(line);
        out.push_str("\x1b[0m\n");
    }
    out
}

/// A QR code for the terminal.
pub fn qr_terminal(text: &str, level: EcLevel) -> Res<String> {
    Ok(paint(&QrCode::with_error_correction_level(text.as_bytes(), level)?))
}

/// Several QR codes that all have exactly the same size, which a steady
/// animation needs (frames of different sizes would jump around).
pub fn same_size_codes(texts: &[String], level: EcLevel) -> Res<Vec<QrCode>> {
    let mut version = 1;
    for text in texts {
        if let Version::Normal(v) = QrCode::with_error_correction_level(text.as_bytes(), level)?.version() {
            version = version.max(v);
        }
    }
    texts
        .iter()
        .map(|t| Ok(QrCode::with_version(t.as_bytes(), Version::Normal(version), level)?))
        .collect()
}

/// Same-size QR codes drawn for the terminal.
pub fn qr_frames_terminal(texts: &[String], level: EcLevel) -> Res<Vec<String>> {
    Ok(same_size_codes(texts, level)?.iter().map(paint).collect())
}

/// Play pre-drawn frames in a loop, redrawing in place, until Enter is pressed.
/// Every frame must have the same number of lines.
pub fn animate(frames: &[String], delay_ms: u64) {
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line);
            stop.store(true, Ordering::SeqCst);
        });
    }
    let height = frames[0].lines().count();
    let mut i = 0;
    let mut home = String::new(); // nothing to undo before the very first frame is drawn
    loop {
        // Redrawing many lines is not one instant action: a camera taking many pictures a second
        // (unlike a person's eye, or a single photo) can catch the screen partway through, part old
        // frame and part new. "Synchronized output" is a terminal feature made for exactly this: the
        // whole redraw between [?2026h and [?2026l is shown to the person as one complete
        // picture, never half-drawn. A terminal that has not heard of it simply ignores those bytes.
        eprint!("\x1b[?2026h{home}{}\x1b[?2026l", frames[i]);
        let _ = io::stderr().flush();
        home = format!("\x1b[{height}A\r"); // back to the top-left corner of the frame, for next time
        let mut waited = 0;
        while waited < delay_ms && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(30));
            waited += 30;
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        i = (i + 1) % frames.len();
    }
    eprintln!();
}

/// The same QR code as an SVG document, for printing.
pub fn qr_svg(text: &str) -> Res<String> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(320, 320)
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ";

    /// Read a code drawn in the full style back into rows of squares (true = dark), checking on
    /// the way that only spaces are drawn, that every space has a background colour, and that
    /// both halves of every square have the same one.
    fn read_full(drawn: &str) -> Vec<Vec<bool>> {
        drawn
            .lines()
            .map(|line| {
                let (mut cells, mut background, mut rest) = (Vec::new(), None, line);
                while let Some(c) = rest.chars().next() {
                    if let Some(after) = rest.strip_prefix("\x1b[") {
                        let end = after.find(|c: char| c.is_ascii_alphabetic()).expect("a whole escape sequence");
                        background = match &after[..=end] {
                            "48;5;16m" => Some(true),
                            "48;5;231m" => Some(false),
                            "0m" => None,
                            other => panic!("unexpected escape sequence {other:?}"),
                        };
                        rest = &after[end + 1..];
                    } else {
                        assert_eq!(c, ' ', "only spaces may be drawn");
                        cells.push(background.expect("every space has a background colour"));
                        rest = &rest[1..];
                    }
                }
                assert_eq!(cells.len() % 2, 0);
                cells
                    .chunks(2)
                    .map(|pair| {
                        assert_eq!(pair[0], pair[1], "a square is two spaces of one colour");
                        pair[0]
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_full_style_draws_every_square_as_two_coloured_spaces_and_reads_back() {
        let code = QrCode::with_error_correction_level(TEXT, EcLevel::L).unwrap();
        let drawn = paint_full(&code);
        assert!(!drawn.contains(['▀', '▄', '█']), "no block characters at all");
        let rows = read_full(&drawn);
        let (n, quiet) = (code.width(), 4);
        assert_eq!(rows.len(), n + 2 * quiet, "one line of text per row, white border included");
        assert!(rows.iter().all(|row| row.len() == n + 2 * quiet), "square");
        let colors = code.to_colors();
        for (y, row) in rows.iter().enumerate() {
            for (x, &dark) in row.iter().enumerate() {
                let inside = (quiet..quiet + n).contains(&x) && (quiet..quiet + n).contains(&y);
                assert_eq!(dark, inside && colors[(y - quiet) * n + x - quiet] == Color::Dark, "square ({x}, {y})");
            }
        }
        // ... and a QR reader reads it back (every square drawn as 6 x 6 pixels)
        let (px, side) = (6, rows.len() * 6);
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| if rows[y / px][x / px] { 0 } else { 255 });
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].decode().unwrap().1, TEXT);
    }

    #[test]
    fn the_compact_style_still_draws_two_rows_per_line() {
        let code = QrCode::with_error_correction_level(TEXT, EcLevel::L).unwrap();
        let drawn = paint_compact(&code);
        assert!(drawn.contains('▀') && drawn.contains('▄'));
        assert_eq!(drawn.lines().count(), (code.width() + 8 + 1) / 2);
        assert!(drawn.lines().all(|l| l.starts_with("\x1b[38;5;16;48;5;231m") && l.ends_with("\x1b[0m")));
    }

    #[test]
    fn apple_terminal_gets_the_full_style_unless_told_otherwise() {
        assert_eq!(qr_style_from(None, Some("Apple_Terminal")), QrStyle::Full);
        assert_eq!(qr_style_from(None, Some("iTerm.app")), QrStyle::Compact);
        assert_eq!(qr_style_from(None, None), QrStyle::Compact);
        assert_eq!(qr_style_from(Some("compact"), Some("Apple_Terminal")), QrStyle::Compact);
        assert_eq!(qr_style_from(Some(" FULL "), Some("iTerm.app")), QrStyle::Full);
        assert_eq!(qr_style_from(Some("typo"), Some("Apple_Terminal")), QrStyle::Full);
        assert_eq!(qr_style_from(Some(""), None), QrStyle::Compact);
    }
}
