//! QR codes as picture files (PNG) and as a web page.
//!
//! A terminal can only draw a QR code as big as its characters allow, and a phone or
//! a webcam needs it big and sharp. A PNG file (for the backup) and a web page that
//! fills the browser window (for the signed transaction) solve that. Nothing here
//! needs the network: the page contains no links, images or scripts from elsewhere.

use qrcode::{types::Color, QrCode};
use std::fmt::Write as _;

/// Empty modules around a QR code (the "quiet zone" the standard asks for).
const QUIET: usize = 4;

fn dark_modules(code: &QrCode) -> Vec<bool> {
    code.to_colors().into_iter().map(|c| c == Color::Dark).collect()
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

/// The code as a grey picture (one byte per pixel, 0 = black, 255 = white, row after row), with
/// the white border, every square `module_px` pixels wide: `(side, pixels)`. The QR window shows
/// these.
pub fn luma(code: &QrCode, module_px: usize) -> (usize, Vec<u8>) {
    let n = code.width();
    let dark = dark_modules(code);
    let side = (n + 2 * QUIET) * module_px;
    let mut out = vec![255u8; side * side];
    for my in 0..n {
        for mx in 0..n {
            if dark[my * n + mx] {
                let (x0, y0) = ((QUIET + mx) * module_px, (QUIET + my) * module_px);
                for y in y0..y0 + module_px {
                    out[y * side + x0..y * side + x0 + module_px].fill(0);
                }
            }
        }
    }
    (side, out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A black-and-white PNG of the QR code, `module_px` pixels per module, with a quiet
/// zone. A small, dependency-free encoder: 1 bit per pixel, uncompressed.
pub fn png(code: &QrCode, module_px: usize) -> Vec<u8> {
    let n = code.width();
    let dark = dark_modules(code);
    let side = (n + 2 * QUIET) * module_px;
    let stride = side.div_ceil(8);
    let mut raw = Vec::with_capacity((stride + 1) * side);
    for y in 0..side {
        raw.push(0); // filter type: none
        let mut row = vec![0xFFu8; stride]; // 1 = white
        let my = y / module_px;
        if my >= QUIET && my < QUIET + n {
            for x in 0..side {
                let mx = x / module_px;
                if mx >= QUIET && mx < QUIET + n && dark[(my - QUIET) * n + (mx - QUIET)] {
                    row[x / 8] &= !(0x80 >> (x % 8));
                }
            }
        }
        raw.extend_from_slice(&row);
    }
    // zlib container with "stored" (uncompressed) blocks
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::new();
    header.extend_from_slice(&(side as u32).to_be_bytes());
    header.extend_from_slice(&(side as u32).to_be_bytes());
    header.extend_from_slice(&[1, 0, 0, 0, 0]); // 1 bit, greyscale, deflate, no filter, no interlace
    png_chunk(&mut out, b"IHDR", &header);
    png_chunk(&mut out, b"IDAT", &z);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

// ---------------------------------------------------------------------------
// Web page
// ---------------------------------------------------------------------------

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// One QR code as inline SVG: a white square with the dark modules as one path.
fn svg_frame(code: &QrCode, index: usize, interval_ms: u32) -> String {
    let n = code.width();
    let total = n + 2 * QUIET;
    let dark = dark_modules(code);
    let mut path = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if dark[y * n + x] {
                let start = x;
                while x < n && dark[y * n + x] {
                    x += 1;
                }
                let run = x - start;
                let _ = write!(path, "M{},{}h{run}v1h-{run}z", start + QUIET, y + QUIET);
            } else {
                x += 1;
            }
        }
    }
    let shown = if index == 0 { " on" } else { "" };
    let delay = index as u32 * interval_ms;
    // The SVG sits in a div because a script cannot set the class of an SVG element in a
    // browser (`className` is read-only there, and assigning to it silently does nothing).
    format!(
        "<div class=\"frame{shown}\" style=\"animation-delay:{delay}ms\"><svg viewBox=\"0 0 {total} {total}\" \
         shape-rendering=\"crispEdges\"><rect width=\"{total}\" height=\"{total}\" fill=\"#fff\"/>\
         <path d=\"{path}\" fill=\"#000\"/></svg></div>\n"
    )
}

const STYLE: &str = "<style>\n\
html,body{margin:0;background:#fff;color:#111;font-family:system-ui,-apple-system,\"Segoe UI\",Roboto,Arial,sans-serif}\n\
body{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1.2vh;padding:1vh 2vw;box-sizing:border-box;min-height:100vh}\n\
h1{font-size:max(20px,3.2vh);margin:0;text-align:center}\n\
p{font-size:max(14px,2.1vh);margin:0;text-align:center;max-width:92vw;color:#333}\n\
#box{position:relative;width:min(72vh,92vw);height:min(72vh,92vw);background:#fff;flex:none}\n\
.frame{position:absolute;left:0;top:0;width:100%;height:100%;display:none}\n\
.frame.on{display:block}\n\
.frame svg{display:block;width:100%;height:100%}\n\
</style>\n";

/// A complete web page that shows the QR codes as large as the window allows. With
/// several codes they are shown one after the other, in a loop (an animated QR code).
/// All codes must have the same size. Works offline, with or without JavaScript.
pub fn page(title: &str, heading: &str, hints: &[&str], codes: &[QrCode], interval_ms: u32) -> String {
    let n = codes.len();
    let mut html = String::new();
    let _ = write!(html, "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n<title>{}</title>\n", escape(title));
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<meta name=\"color-scheme\" content=\"light\">\n");
    html.push_str(STYLE);
    if n > 1 {
        // Without JavaScript the pictures still take turns, by CSS alone.
        let cycle = n as u32 * interval_ms;
        let step = 100.0 / n as f64;
        let _ = write!(
            html,
            "<noscript><style>.frame{{display:block;opacity:0;animation:cycle {cycle}ms steps(1,end) infinite}}\
             @keyframes cycle{{0%{{opacity:1}}{step:.4}%{{opacity:0}}100%{{opacity:0}}}}</style></noscript>\n"
        );
    }
    html.push_str("</head><body>\n");
    let _ = writeln!(html, "<h1>{}</h1>", escape(heading));
    for hint in hints {
        let _ = writeln!(html, "<p>{}</p>", escape(hint));
    }
    html.push_str("<div id=\"box\">\n");
    for (i, code) in codes.iter().enumerate() {
        html.push_str(&svg_frame(code, i, interval_ms));
    }
    html.push_str("</div>\n");
    if n > 1 {
        let _ = writeln!(html, "<p id=\"count\">Picture 1 of {n}</p>");
        let _ = write!(
            html,
            "<script>\n(function(){{var f=document.querySelectorAll('.frame'),c=document.getElementById('count'),i=0,n=f.length;\n\
             if(n<2)return;\n\
             setInterval(function(){{f[i].setAttribute('class','frame');i=(i+1)%n;f[i].setAttribute('class','frame on');c.textContent='Picture '+(i+1)+' of '+n;}},{interval_ms});}})();\n</script>\n"
        );
    }
    html.push_str("</body></html>\n");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcode::{types::Version, EcLevel};

    fn code(text: &str) -> QrCode {
        QrCode::with_version(text.as_bytes(), Version::Normal(5), EcLevel::L).unwrap()
    }

    /// Read back our own PNG: check the structure and return the pixels (true = dark).
    fn decode_png(png: &[u8]) -> (usize, Vec<bool>) {
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut pos = 8;
        let (mut side, mut idat) = (0usize, Vec::new());
        let mut kinds = Vec::new();
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &png[pos + 4..pos + 8];
            let data = &png[pos + 8..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(&png[pos + 4..pos + 8 + len]), "bad CRC in {}", String::from_utf8_lossy(kind));
            kinds.push(String::from_utf8_lossy(kind).to_string());
            match kind {
                b"IHDR" => {
                    side = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
                    assert_eq!(u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize, side);
                    assert_eq!(&data[8..], &[1, 0, 0, 0, 0]);
                }
                b"IDAT" => idat.extend_from_slice(data),
                _ => {}
            }
            pos += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        // zlib with stored blocks
        assert_eq!(&idat[..2], &[0x78, 0x01]);
        let (mut raw, mut p) = (Vec::new(), 2);
        loop {
            let last = idat[p] == 1;
            let len = u16::from_le_bytes([idat[p + 1], idat[p + 2]]) as usize;
            assert_eq!(u16::from_le_bytes([idat[p + 3], idat[p + 4]]), !(len as u16));
            raw.extend_from_slice(&idat[p + 5..p + 5 + len]);
            p += 5 + len;
            if last {
                break;
            }
        }
        assert_eq!(u32::from_be_bytes(idat[p..p + 4].try_into().unwrap()), adler32(&raw));
        let stride = side.div_ceil(8);
        let mut pixels = Vec::new();
        for y in 0..side {
            assert_eq!(raw[y * (stride + 1)], 0);
            for x in 0..side {
                pixels.push(raw[y * (stride + 1) + 1 + x / 8] & (0x80 >> (x % 8)) == 0);
            }
        }
        (side, pixels)
    }

    #[test]
    fn png_is_valid_and_shows_exactly_the_qr_modules() {
        let c = code("SEEDLOCK1:TEST");
        let n = c.width();
        let (side, pixels) = decode_png(&png(&c, 6));
        assert_eq!(side, (n + 2 * QUIET) * 6);
        let dark = dark_modules(&c);
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = (x / 6, y / 6);
                let inside = mx >= QUIET && mx < QUIET + n && my >= QUIET && my < QUIET + n;
                let expected = inside && dark[(my - QUIET) * n + (mx - QUIET)];
                assert_eq!(pixels[y * side + x], expected, "pixel {x},{y}");
            }
        }
        // large enough to need several stored blocks (65535 bytes each)
        let big = png(&code("A"), 40);
        assert!(big.len() > 65_535);
        decode_png(&big);
    }

    #[test]
    fn page_shows_every_code_at_one_size_and_needs_no_network() {
        let codes: Vec<QrCode> = ["UR:A", "UR:B", "UR:C"].iter().map(|t| code(t)).collect();
        let html = page("T <&>", "Scan me", &["hint one", "hint \"two\""], &codes, 250);
        assert_eq!(html.matches("<div class=\"frame").count(), 3);
        assert_eq!(html.matches("class=\"frame on\"").count(), 1, "only the first picture is shown at the start");
        assert_eq!(html.matches("viewBox=\"0 0 45 45\"").count(), 3, "all pictures have the same size");
        assert!(html.contains("<script>") && html.contains("<noscript>"), "JavaScript plus a CSS-only fallback");
        assert!(html.contains("animation-delay:500ms"));
        assert!(html.contains("T &lt;&amp;&gt;") && html.contains("hint &quot;two&quot;"), "text is escaped");
        assert!(!html.contains("http") && !html.contains("src=") && !html.contains("href="), "nothing is loaded from anywhere");
        // a single code: no animation machinery
        let one = page("T", "H", &[], &codes[..1], 250);
        assert!(!one.contains("<script>") && !one.contains("noscript") && !one.contains("Picture 1 of"));
    }
}
