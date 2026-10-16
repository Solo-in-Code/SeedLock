//! Reading QR codes from camera pictures, and making QR codes to show.
//!
//! A grey camera picture is read as it is, and when that fails, enlarged, sharpened, reduced and with the
//! light evened out, which reads far more codes from a soft, noisy picture of a screen. The command-line
//! program (its `scan.rs`, with the camera through ffmpeg) and the phone app both read QR codes with
//! this crate.
//!
//! No I/O: pictures come in as bytes, codes go out as grids of modules.
#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

pub use qrcode::EcLevel;
use qrcode::{types::Color, types::Version, QrCode};

// ---------------------------------------------------------------------------
// Reading QR codes in a picture
// ---------------------------------------------------------------------------

/// One grey picture from a camera (one byte per pixel, row after row).
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub luma: Vec<u8>,
}

/// The four corners of a QR code in a picture, in pixels.
pub type Quad = [(i32, i32); 4];

/// A QR code that was read: what it says and where it is in the picture.
pub struct Found {
    pub text: String,
    pub quad: Quad,
}

/// How long one picture may take, at most, before the remaining tries are skipped (a camera sends
/// several pictures a second, and an animated code must not miss its frames).
pub const TIME_PER_PICTURE: Duration = Duration::from_millis(300);

/// `scale` is how much larger than the camera picture the analysed picture is, so that
/// the corners can be given in the camera picture's own pixels.
fn decode_grid(width: usize, height: usize, luma: &[u8], scale: f32) -> Vec<Found> {
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * width + x]);
    image
        .detect_grids()
        .into_iter()
        .filter_map(|grid| {
            let (_, text) = grid.decode().ok()?;
            let corner = |i: usize| ((grid.bounds[i].x as f32 / scale).round() as i32, (grid.bounds[i].y as f32 / scale).round() as i32);
            Some(Found { text, quad: [corner(0), corner(1), corner(2), corner(3)] })
        })
        .collect()
}

/// Twice as large (bilinear): helps when the modules of the code are only a few pixels wide.
fn enlarge(width: usize, height: usize, p: &[u8]) -> (usize, usize, Vec<u8>) {
    let (nw, nh) = (width * 2, height * 2);
    let at = |x: isize, y: isize| {
        f32::from(p[(y.clamp(0, height as isize - 1) as usize) * width + x.clamp(0, width as isize - 1) as usize])
    };
    let mut out = vec![0u8; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let (fx, fy) = ((x as f32 + 0.5) / 2.0 - 0.5, (y as f32 + 0.5) / 2.0 - 0.5);
            let (x0, y0) = (fx.floor() as isize, fy.floor() as isize);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let v = at(x0, y0) * (1.0 - tx) * (1.0 - ty)
                + at(x0 + 1, y0) * tx * (1.0 - ty)
                + at(x0, y0 + 1) * (1.0 - tx) * ty
                + at(x0 + 1, y0 + 1) * tx * ty;
            out[y * nw + x] = v.round() as u8;
        }
    }
    (nw, nh, out)
}

/// Half as large (average of 2x2): removes noise and speeds up very large pictures.
fn shrink(width: usize, height: usize, p: &[u8]) -> (usize, usize, Vec<u8>) {
    let (nw, nh) = (width / 2, height / 2);
    let mut out = vec![0u8; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let sum = u32::from(p[2 * y * width + 2 * x])
                + u32::from(p[2 * y * width + 2 * x + 1])
                + u32::from(p[(2 * y + 1) * width + 2 * x])
                + u32::from(p[(2 * y + 1) * width + 2 * x + 1]);
            out[y * nw + x] = (sum / 4) as u8;
        }
    }
    (nw, nh, out)
}

/// Unsharp mask: makes soft edges crisper.
fn sharpen(width: usize, height: usize, p: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let (mut sum, mut count) = (0.0f32, 0.0f32);
            for yy in y.saturating_sub(2)..(y + 3).min(height) {
                for xx in x.saturating_sub(2)..(x + 3).min(width) {
                    sum += f32::from(p[yy * width + xx]);
                    count += 1.0;
                }
            }
            let centre = f32::from(p[y * width + x]);
            out[y * width + x] = (centre + 2.0 * (centre - sum / count)).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Find and read the QR codes in a camera picture. A blurry or noisy picture often
/// fails as it is but works enlarged, sharpened or reduced, so those are tried in turn
/// (measured on simulated webcam pictures: this reads twice as many as a single try).
/// A picture whose size does not match its bytes gives nothing.
pub fn find_qr_codes(frame: &Frame) -> Vec<Found> {
    find_qr_codes_within(frame, TIME_PER_PICTURE)
}

/// [`find_qr_codes`] with its time per picture as a parameter: the tests give it more, so that a slow
/// (unoptimised) test build still runs every try.
pub fn find_qr_codes_within(frame: &Frame, budget: Duration) -> Vec<Found> {
    let started = Instant::now();
    let (w, h, p) = (frame.width, frame.height, &frame.luma[..]);
    if w < 21 || h < 21 || p.len() != w * h {
        return Vec::new();
    }
    let found = decode_grid(w, h, p, 1.0);
    if !found.is_empty() {
        return found;
    }
    if w <= 1000 {
        let (uw, uh, up) = enlarge(w, h, p);
        let found = decode_grid(uw, uh, &up, 2.0);
        if !found.is_empty() {
            return found;
        }
    }
    if started.elapsed() < budget {
        let found = decode_grid(w, h, &sharpen(w, h, p), 1.0);
        if !found.is_empty() {
            return found;
        }
    }
    if started.elapsed() < budget {
        let (dw, dh, down) = shrink(w, h, p);
        let found = decode_grid(dw, dh, &down, 0.5);
        if !found.is_empty() {
            return found;
        }
    }
    // A dense code (a whole wallet in one QR code) photographed on a screen: glare, a darker edge and
    // the screen's own pixel pattern get in the way. With the light evened out (each pixel compared with
    // its neighbourhood), and once more after a slight blur that washes out the screen's pattern.
    // (Sharpening helps too, but only after the blur: on the evened-out picture alone it amplifies the
    // noise so much that one try can take a second, which would make animated codes miss their frames.)
    for (softened, prepared) in [(false, even_out_light(w, h, p)), (true, even_out_light(w, h, &soften(w, h, p)))] {
        if started.elapsed() >= budget {
            break;
        }
        let found = decode_grid(w, h, &prepared, 1.0);
        if !found.is_empty() {
            return found;
        }
        if started.elapsed() < budget {
            let (dw, dh, down) = shrink(w, h, &prepared);
            let found = decode_grid(dw, dh, &down, 0.5);
            if !found.is_empty() {
                return found;
            }
        }
        if softened && started.elapsed() < budget {
            let found = decode_grid(w, h, &sharpen(w, h, &prepared), 1.0);
            if !found.is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}

/// Each pixel compared with its neighbourhood: (value - local mean) / local spread, so that glare,
/// shadows and a darker edge no longer move the line between black and white. The neighbourhood is
/// about a fourteenth of the picture's height (51 pixels in a 720-line picture).
fn even_out_light(width: usize, height: usize, p: &[u8]) -> Vec<u8> {
    let radius = (height.min(width) / 28).max(7);
    let stride = width + 1;
    let mut sum = vec![0u64; stride * (height + 1)];
    let mut squares = vec![0u64; stride * (height + 1)];
    for y in 0..height {
        let (mut row, mut row_sq) = (0u64, 0u64);
        for x in 0..width {
            let v = u64::from(p[y * width + x]);
            row += v;
            row_sq += v * v;
            sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row;
            squares[(y + 1) * stride + x + 1] = squares[y * stride + x + 1] + row_sq;
        }
    }
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        let (y0, y1) = (y.saturating_sub(radius), (y + radius + 1).min(height));
        for x in 0..width {
            let (x0, x1) = (x.saturating_sub(radius), (x + radius + 1).min(width));
            let area = ((y1 - y0) * (x1 - x0)) as f32;
            let total = |t: &[u64]| (t[y1 * stride + x1] + t[y0 * stride + x0] - t[y0 * stride + x1] - t[y1 * stride + x0]) as f32;
            let mean = total(&sum) / area;
            let spread = (total(&squares) / area - mean * mean).max(0.0).sqrt();
            let v = (f32::from(p[y * width + x]) - mean) / (spread + 8.0) * 60.0 + 128.0;
            out[y * width + x] = v.clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// A slight blur (1 4 6 4 1 in each direction), enough to wash out a screen's pixel pattern while
/// keeping the squares of the code.
fn soften(width: usize, height: usize, p: &[u8]) -> Vec<u8> {
    const K: [u32; 5] = [1, 4, 6, 4, 1];
    let mut across = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0;
            for (i, k) in K.iter().enumerate() {
                let xx = (x + i).saturating_sub(2).min(width - 1);
                acc += k * u32::from(p[y * width + xx]);
            }
            across[y * width + x] = ((acc + 8) / 16) as u8;
        }
    }
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0;
            for (i, k) in K.iter().enumerate() {
                let yy = (y + i).saturating_sub(2).min(height - 1);
                acc += k * u32::from(across[yy * width + x]);
            }
            out[y * width + x] = ((acc + 8) / 16) as u8;
        }
    }
    out
}

/// True when a QR code is in the picture (its three corner squares are found), read or not. Looked for
/// as the reader looks: in the picture as it is, then with the light evened out after a slight blur
/// (a code on a screen often shows its corners only that way).
pub fn qr_in_view(frame: &Frame) -> bool {
    let (w, h, p) = (frame.width, frame.height, &frame.luma[..]);
    if w < 21 || h < 21 || p.len() != w * h {
        return false;
    }
    let seen = |luma: &[u8]| {
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| luma[y * w + x]);
        !image.detect_grids().is_empty()
    };
    seen(p) || seen(&even_out_light(w, h, &soften(w, h, p)))
}

// ---------------------------------------------------------------------------
// Making QR codes
// ---------------------------------------------------------------------------

/// A QR code as a square grid of modules, without the quiet zone: `width` x `width` values,
/// row after row, `true` for a dark module. Whoever draws it adds a light border of 4 modules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub width: usize,
    pub dark: Vec<bool>,
}

impl Matrix {
    fn of(code: &QrCode) -> Matrix {
        Matrix { width: code.width(), dark: code.to_colors().into_iter().map(|c| c == Color::Dark).collect() }
    }
}

/// Why a QR code could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooLong;

impl std::fmt::Display for TooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the text is too long for one QR code")
    }
}

impl std::error::Error for TooLong {}

/// One QR code of `text`.
pub fn matrix(text: &str, level: EcLevel) -> Result<Matrix, TooLong> {
    QrCode::with_error_correction_level(text.as_bytes(), level).map(|c| Matrix::of(&c)).map_err(|_| TooLong)
}

/// Several QR codes that all have exactly the same size, which a steady animation needs
/// (frames of different sizes would jump around): the size of the largest one.
pub fn same_size_matrices(texts: &[String], level: EcLevel) -> Result<Vec<Matrix>, TooLong> {
    let mut version = 1;
    for text in texts {
        if let Version::Normal(v) = QrCode::with_error_correction_level(text.as_bytes(), level).map_err(|_| TooLong)?.version() {
            version = version.max(v);
        }
    }
    texts
        .iter()
        .map(|t| QrCode::with_version(t.as_bytes(), Version::Normal(version), level).map(|c| Matrix::of(&c)).map_err(|_| TooLong))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_qr_texts(frame: &Frame) -> Vec<String> {
        find_qr_codes(frame).into_iter().map(|f| f.text).collect()
    }

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
        blur_and_noise(&mut luma, width, height, blur, seed, 17);
        Frame { width, height, luma }
    }

    fn blur_and_noise(luma: &mut [u8], width: usize, height: usize, blur: usize, seed: u64, spread: u64) {
        for _ in 0..blur {
            let src = luma.to_vec();
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
            let noise = ((state >> 33) % spread) as i32 - (spread / 2) as i32;
            *v = (i32::from(*v) + noise).clamp(0, 255) as u8;
        }
    }

    /// A dense code photographed on a screen: the screen's own pixel pattern (every third column darker),
    /// light falling off across the picture, lifted blacks, a soft focus and noise.
    fn screen_photo_of(n: usize, colors: &[Color], px: usize, glare: f32, stripes: f32, blur: usize, seed: u64) -> Frame {
        let (width, height) = (1280usize, 720usize);
        let quiet = 2;
        let side = (n + 2 * quiet) * px;
        let (x0, y0) = ((width - side) / 2, (height - side) / 2);
        let mut f = vec![200f32; width * height];
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = (x / px, y / px);
                let dark = mx >= quiet && mx < quiet + n && my >= quiet && my < quiet + n && colors[(my - quiet) * n + mx - quiet] == Color::Dark;
                f[(y0 + y) * width + x0 + x] = if dark { 0.0 } else { 255.0 };
            }
        }
        for y in 0..height {
            for x in 0..width {
                let stripe = if x % 3 == 2 { 1.0 - stripes } else { 1.0 };
                let light = 1.0 - glare * (x as f32 / width as f32) - glare * 0.5 * (y as f32 / height as f32);
                f[y * width + x] = f[y * width + x] * stripe * light * 0.8 + 40.0;
            }
        }
        let mut luma: Vec<u8> = f.iter().map(|v| v.clamp(0.0, 255.0) as u8).collect();
        blur_and_noise(&mut luma, width, height, blur, seed, 13);
        Frame { width, height, luma }
    }

    #[test]
    fn reads_a_qr_code_in_a_soft_noisy_camera_picture() {
        let text = "UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ";
        for (px, blur) in [(6, 0), (6, 1), (5, 2)] {
            let frame = camera_picture(text, 640, 480, px, blur, 7);
            assert_eq!(find_qr_texts(&frame), [text], "{px} px per module, blur {blur}");
        }
        let empty = Frame { width: 640, height: 480, luma: vec![128; 640 * 480] };
        assert!(find_qr_texts(&empty).is_empty());
    }

    #[test]
    fn a_picture_whose_size_does_not_fit_its_bytes_gives_nothing() {
        let frame = camera_picture("UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ", 640, 480, 6, 0, 7);
        let wrong = Frame { width: 641, height: 480, luma: frame.luma.clone() };
        assert!(find_qr_codes(&wrong).is_empty());
        assert!(!qr_in_view(&wrong));
        assert!(find_qr_codes(&Frame { width: 0, height: 0, luma: Vec::new() }).is_empty());
    }

    /// What the reader tried before the light-evening tries were added (for the tests).
    fn read_by_the_first_tries(frame: &Frame) -> bool {
        let (w, h, p) = (frame.width, frame.height, &frame.luma[..]);
        let (dw, dh, down) = shrink(w, h, p);
        !decode_grid(w, h, p, 1.0).is_empty() || !decode_grid(w, h, &sharpen(w, h, p), 1.0).is_empty() || !decode_grid(dw, dh, &down, 0.5).is_empty()
    }

    #[test]
    fn reads_a_dense_code_photographed_on_a_screen() {
        // a whole wallet in one QR code (version 13), as Sparrow shows it at medium density
        let text = format!("UR:CRYPTO-OUTPUT/{}", "LPADAXCFAXJNCYJZ".repeat(33));
        let code = QrCode::with_version(text.as_bytes(), Version::Normal(13), EcLevel::L).unwrap();
        for (px, glare, stripes, blur) in [(4, 0.3, 0.3, 2), (4, 0.5, 0.45, 2), (4, 0.65, 0.3, 2), (5, 0.5, 0.15, 2), (5, 0.65, 0.45, 1)] {
            let frame = screen_photo_of(code.width(), &code.to_colors(), px, glare, stripes, blur, 9);
            let what = format!("{px} px per module, glare {glare}, stripes {stripes}, blur {blur}");
            assert!(!read_by_the_first_tries(&frame), "{what}: this picture is meant to need the light-evening tries");
            let read: Vec<String> = find_qr_codes_within(&frame, Duration::from_secs(120)).into_iter().map(|f| f.text).collect();
            assert_eq!(read, std::slice::from_ref(&text), "{what}");
        }
    }

    #[test]
    fn notices_a_code_in_view_that_cannot_be_read() {
        let code = QrCode::with_version(b"UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ", Version::Normal(7), EcLevel::L).unwrap();
        let mut frame = camera_picture_of(&code, 640, 480, 6, 0, 7);
        // grey out a band across the middle of the code: its corner squares stay, its data does not
        let side = (code.width() + 8) * 6;
        let (x0, y0) = ((640 - side) / 3, (480 - side) / 2);
        for y in y0 + side / 2 - 30..y0 + side / 2 + 30 {
            for x in x0 + 4 * 6..x0 + side - 4 * 6 {
                frame.luma[y * 640 + x] = if (x / 3 + y / 5) % 2 == 0 { 110 } else { 150 };
            }
        }
        assert!(find_qr_texts(&frame).is_empty(), "it cannot be read");
        assert!(qr_in_view(&frame), "but it is in view, so SeedLock can say what helps");
        assert!(!qr_in_view(&Frame { width: 640, height: 480, luma: vec![128; 640 * 480] }));
        // a dense code photographed on a screen whose data is wrong in a band between its alignment marks
        // (version 13: marks around rows 6 and 34), far more than error correction L repairs, while its
        // corners, timing lines and alignment marks are intact, as with a code too soft to read
        let text = format!("UR:CRYPTO-OUTPUT/{}", "LPADAXCFAXJNCYJZ".repeat(33));
        let dense = QrCode::with_version(text.as_bytes(), Version::Normal(13), EcLevel::L).unwrap();
        let n = dense.width();
        let mut colors = dense.to_colors();
        for row in 10..31 {
            for col in 10..n - 12 {
                let c = &mut colors[row * n + col];
                *c = if *c == Color::Dark { Color::Light } else { Color::Dark };
            }
        }
        let photo = screen_photo_of(n, &colors, 4, 0.3, 0.3, 1, 9);
        assert!(find_qr_codes_within(&photo, Duration::from_secs(120)).is_empty(), "too damaged to read");
        assert!(qr_in_view(&photo), "but seen");
    }

    #[test]
    fn says_where_the_qr_code_is() {
        // 45 modules of 6 px; the picture puts the code's top-left module at (x0 + 24, y0 + 24)
        let (w, h, px) = (640usize, 480usize, 6i32);
        let side = (45 + 8) * px;
        let (x0, y0) = ((w as i32 - side) / 3, (h as i32 - side) / 2);
        let (left, top, size) = (x0 + 4 * px, y0 + 4 * px, 45 * px);
        let expected = [(left, top), (left + size, top), (left + size, top + size), (left, top + size)];
        for blur in [0, 1] {
            let found = find_qr_codes(&camera_picture("UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ", w, h, px as usize, blur, 3));
            assert_eq!(found.len(), 1);
            for corner in expected {
                let nearest = found[0].quad.iter().map(|q| (q.0 - corner.0).abs().max((q.1 - corner.1).abs())).min().unwrap();
                assert!(nearest <= 2 * px, "corner {corner:?} is {nearest} px from the reported outline {:?}", found[0].quad);
            }
        }
        // ... also when the picture is analysed enlarged or reduced (the corners come back in the camera's pixels)
        let soft = camera_picture("UR:CRYPTO-PSBT/1-4/LPADAXCFAXJNCYJZ", 640, 480, 4, 2, 5);
        for code in find_qr_codes(&soft) {
            assert!(code.quad.iter().all(|q| q.0 > 0 && q.0 < 640 && q.1 > 0 && q.1 < 480), "{:?}", code.quad);
        }
    }

    #[test]
    fn made_codes_read_back_and_animated_ones_share_one_size() {
        let texts: Vec<String> = ["UR:CRYPTO-PSBT/1-3/LPADAXCFAXJNCYJZ", "UR:CRYPTO-PSBT/2-3/LPAOAXCFAXJNCYJZHDWLSKD", "UR:CRYPTO-PSBT/3-3/X"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let codes = same_size_matrices(&texts, EcLevel::L).unwrap();
        assert!(codes.iter().all(|c| c.width == codes[0].width && c.dark.len() == c.width * c.width));
        // draw each one as a camera would see it, and read it back
        for (text, code) in texts.iter().zip(&codes) {
            let px = 6;
            let side = (code.width + 8) * px;
            let mut luma = vec![235u8; side * side];
            for y in 0..code.width * px {
                for x in 0..code.width * px {
                    if code.dark[(y / px) * code.width + x / px] {
                        luma[(y + 4 * px) * side + x + 4 * px] = 30;
                    }
                }
            }
            assert_eq!(find_qr_texts(&Frame { width: side, height: side, luma }), std::slice::from_ref(text));
        }
        let one = matrix("SEEDLOCK1:ABC", EcLevel::M).unwrap();
        assert_eq!(one.dark.len(), one.width * one.width);
        assert_eq!(matrix(&"A".repeat(10_000), EcLevel::M), Err(TooLong));
    }
}
