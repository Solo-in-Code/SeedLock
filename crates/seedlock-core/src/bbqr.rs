//! BBQr ("Better Bitcoin QR", by Coinkite): one file spread over a series of QR codes. Each code
//! starts with an 8-character header: `B$`, the encoding, the file type, the number of parts and
//! this part's number (two base-36 digits each, the first part being 00). For example `B$ZP0300`:
//! encoding Z, a PSBT, 3 parts, part 0. Encodings: H (hexadecimal), 2 (base32) and Z (deflate-
//! compressed with a 2^10 window and no header, then base32). Each part decodes to whole bytes on its
//! own; the parts may be scanned in any order and repeat, but all must agree on encoding, type and
//! number of parts. Sparrow shows PSBTs and wallet descriptors this way when BBQr is chosen in its QR
//! window (a descriptor as text, type U), and Coldcard uses it.

use data_encoding::{BASE32_NOPAD, HEXUPPER_PERMISSIVE};

/// The largest file SeedLock takes from a BBQr series, after unpacking: a compressed code cannot
/// make it fill the memory.
pub const MAX_BBQR_BYTES: usize = 16 * 1024 * 1024;

/// What one more scanned code did, see [`BbqrCollector`].
#[derive(Debug, PartialEq, Eq)]
pub enum BbqrUpdate {
    /// The whole file: its type letter (P a PSBT, T a transaction, U text, J JSON, ...) and its bytes.
    Done { file_type: char, data: Vec<u8> },
    /// Part of a series: `have` of `total` parts so far.
    Progress { have: usize, total: usize },
    /// Not usable; the text says why.
    Ignored(String),
}

/// True when the text of a QR code starts like a BBQr part.
pub fn is_bbqr(text: &str) -> bool {
    text.trim_start().starts_with("B$")
}

/// Collects the parts of a BBQr series, see the module documentation.
#[derive(Debug, Default)]
pub struct BbqrCollector {
    /// The first 6 characters every part of the current series shares.
    series: Option<String>,
    parts: Vec<Option<String>>,
}

impl BbqrCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the text of one scanned QR code.
    pub fn add(&mut self, text: &str) -> BbqrUpdate {
        let text = text.trim();
        if !is_bbqr(text) || text.len() < 8 || !text.is_char_boundary(8) {
            return BbqrUpdate::Ignored("it is not a BBQr code".into());
        }
        let (header, data) = text.split_at(8);
        let bytes = header.as_bytes();
        let (encoding, file_type) = (bytes[2] as char, bytes[3] as char);
        if !matches!(encoding, 'H' | '2' | 'Z') {
            return BbqrUpdate::Ignored(format!("it uses an encoding ({encoding}) that SeedLock does not know"));
        }
        let number = |digits: &str| usize::from_str_radix(digits, 36).ok().filter(|_| digits.bytes().all(|b| b.is_ascii_alphanumeric()));
        let (Some(total), Some(index)) = (number(&header[4..6]), number(&header[6..8])) else {
            return BbqrUpdate::Ignored("its header cannot be read".into());
        };
        if total == 0 || index >= total {
            return BbqrUpdate::Ignored("its part number does not fit the number of parts".into());
        }
        // Another series (or a part that disagrees with the one seen before): start again with it.
        let disagrees = self.parts.get(index).and_then(|p| p.as_deref()).is_some_and(|seen| seen != data);
        if self.series.as_deref() != Some(&header[..6]) || disagrees {
            self.series = Some(header[..6].to_string());
            self.parts = vec![None; total];
        }
        self.parts[index] = Some(data.to_string());
        let have = self.parts.iter().filter(|p| p.is_some()).count();
        if have < total {
            return BbqrUpdate::Progress { have, total };
        }
        let parts: Vec<String> = self.parts.drain(..).flatten().collect();
        self.series = None;
        match decode(encoding, &parts) {
            Ok(data) => BbqrUpdate::Done { file_type, data },
            Err(why) => BbqrUpdate::Ignored(why),
        }
    }
}

fn decode(encoding: char, parts: &[String]) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    for part in parts {
        let bytes = match encoding {
            'H' => HEXUPPER_PERMISSIVE.decode(part.as_bytes()),
            _ => BASE32_NOPAD.decode(part.as_bytes()),
        }
        .map_err(|_| "its data cannot be decoded".to_string())?;
        raw.extend_from_slice(&bytes);
        if raw.len() > MAX_BBQR_BYTES {
            return Err("it is too large".into());
        }
    }
    if encoding == 'Z' {
        raw = miniz_oxide::inflate::decompress_to_vec_with_limit(&raw, MAX_BBQR_BYTES)
            .map_err(|_| "its compressed data cannot be unpacked, or would be too large".to_string())?;
    }
    Ok(raw)
}

/// A BBQr series of `data` for the tests: compressed (Z), in about `parts` parts, each part whole bytes.
#[cfg(test)]
pub(crate) fn test_parts(file_type: char, data: &[u8], parts: usize) -> Vec<String> {
    let encoded = BASE32_NOPAD.encode(&miniz_oxide::deflate::compress_to_vec(data, 9));
    let per = (encoded.len().div_ceil(parts)).div_ceil(8) * 8;
    let chunks: Vec<&str> = encoded.as_bytes().chunks(per).map(|c| std::str::from_utf8(c).unwrap()).collect();
    let digits = |n: usize| format!("{}{}", char::from_digit((n / 36) as u32, 36).unwrap(), char::from_digit((n % 36) as u32, 36).unwrap()).to_uppercase();
    chunks.iter().enumerate().map(|(i, c)| format!("B$Z{file_type}{}{}{c}", digits(chunks.len()), digits(i))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made by Coinkite's reference implementation (github.com/coinkite/BBQr, python/bbqr split_qrs).
    const TEXT: &str = "wsh(sortedmulti(2,[73c5da0a/48h/1h/0h/2h]tpubDFH9dgzveyD8zTbPUFuLrGmCydNvxehyNdUXKJAQN8x4aZ4j6UZqGfnqFrD4NqyaTVGKbvEW54tsvPTK2UoSbCC1PJY8iCNiwTL3RWZEheQ/<0;1>/*))#abcd1234\nwsh(sortedmulti(2,[73c5da0a/48h/1h/0h/2h]tpubDFH9dgzveyD8zTbPUFuLrGmCydNvxehyNdUXKJAQN8x4aZ4j6UZqGfnqFrD4NqyaTVGKbvEW54tsvPTK2UoSbCC1PJY8iCNiwTL3RWZEheQ/<0;1>/*))#abcd1234\nwsh(sortedmulti(2,[73c5da0a/48h/1h/0h/2h]tpubDFH9dgzveyD8zTbPUFuLrGmCydNvxehyNdUXKJAQN8x4aZ4j6UZqGfnqFrD4NqyaTVGKbvEW54tsvPTK2UoSbCC1PJY8iCNiwTL3RWZEheQ/<0;1>/*))#abcd1234\n";
    const Z_PARTS: [&str; 3] = [
        "B$ZU03005XHESDUCGAAABQF337YIFRVEFRKTCGQTAOBBDUVAQIF4MQ3LVPCYQCUUWK6N4V3YONPDAVOBSXRJ2C2G2PZCSESFD6OMPRTVJCYYUAJU",
        "B$ZU0301HHIDQUBZ2D4UK7CKMI52WCN5W6JDLNWZQYSIRHGSZ7O5I2RISI26GDNC2HIVWLZWZCWCDDXBMMKMLGL3PNSU43SDSQ2TRXF3D2I4XQYQ",
        "B$ZU0302RJBANINHI7XR3MJMFVMJ7TGEIJERL6WG6YIC6OO3QCMTVVPGUDP6W5JRXFJE2N3AU75F673S7UBA",
    ];
    const BASE32_PARTS: [&str; 2] = [
        "B$2U0200O5ZWQKDTN5ZHIZLENV2WY5DJFAZCYWZXGNRTKZDBGBQS6NBYNAXTC2BPGBUC6MTILV2HA5LCIRDEQOLEM55HMZLZIQ4HUVDCKBKUM5KMOJDW2Q3ZMRHHM6DFNB4U4ZCVLBFUUQKRJY4HQNDBLI2GUNSVLJYUOZTOOFDHERBUJZYXSYKUKZDUWYTWIVLTKNDUON3FAVCLGJKW6U3CINBTCUCKLE4GSQ2ONF3VITBTKJLVURLIMVIS6PBQHMYT4LZKFEUSGYLCMNSDCMRTGQFHO43IFBZW64TUMVSG25LMORUSQMRMLM3TGYZVMRQTAYJPGQ4GQLZRNAXTA2BPGJUF25DQOVREIRSIHFSGO6TWMV4UIOD2KRRFAVKGOVGHER3NIN4WITTWPBSWQ6KOMRKVQS2KIFIU4ODYGRQVUNDKGZKVU4KHMZXHCRTSIQ2E44LZMFKFMR2L",
        "B$2U0201MJ3EKVZVGR2HG5SQKRFTEVLPKNREGQZRKBFFSODJINHGS52UJQZVEV22IVUGKUJPHQYDWMJ6F4VCSKJDMFRGGZBRGIZTICTXONUCQ43POJ2GKZDNOVWHI2JIGIWFWNZTMM2WIYJQMEXTIODIF4YWQLZQNAXTE2C5ORYHKYSEIZEDSZDHPJ3GK6KEHB5FIYSQKVDHKTDSI5WUG6LEJZ3HQZLIPFHGIVKYJNFECUKOHB4DIYK2GRVDMVK2OFDWM3TRIZZEINCOOF4WCVCWI5FWE5SFK42TI5DTOZIFISZSKVXVGYSDIMYVASSZHBUUGTTJO5KEYM2SK5NEK2DFKEXTYMB3GE7C6KRJFERWCYTDMQYTEMZUBI",
    ];
    const HEX_PART: &str = "B$HU0100536565644C6F636B2042425172";

    fn collect(parts: &[&str]) -> BbqrUpdate {
        let mut c = BbqrCollector::new();
        let mut last = BbqrUpdate::Ignored(String::new());
        for p in parts {
            last = c.add(p);
        }
        last
    }

    #[test]
    fn reads_coinkites_reference_codes_in_every_encoding_and_any_order() {
        let text = BbqrUpdate::Done { file_type: 'U', data: TEXT.as_bytes().to_vec() };
        assert_eq!(collect(&Z_PARTS), text, "compressed (Z), 3 parts");
        assert_eq!(collect(&[Z_PARTS[2], Z_PARTS[0], Z_PARTS[0], Z_PARTS[1]]), text, "out of order, with a repeat");
        assert_eq!(collect(&BASE32_PARTS), text, "base32, 2 parts");
        assert_eq!(collect(&[HEX_PART]), BbqrUpdate::Done { file_type: 'U', data: b"SeedLock BBQr".to_vec() }, "hex, 1 part");
        let mut c = BbqrCollector::new();
        assert_eq!(c.add(Z_PARTS[1]), BbqrUpdate::Progress { have: 1, total: 3 });
        assert_eq!(c.add(Z_PARTS[1]), BbqrUpdate::Progress { have: 1, total: 3 }, "a repeat adds nothing");
        assert_eq!(c.add(BASE32_PARTS[0]), BbqrUpdate::Progress { have: 1, total: 2 }, "another series starts over");
        assert_eq!(c.add(BASE32_PARTS[1]), text);
    }

    #[test]
    fn refuses_what_does_not_fit_and_codes_that_unpack_too_large() {
        for bad in ["B$", "B$ZU03", "B$QU0100ABCD", "B$ZU0003ABCD", "B$ZU00..ABCD", "B$HU0100ABC", "B$2U01001", "hello"] {
            assert!(matches!(collect(&[bad]), BbqrUpdate::Ignored(_)), "{bad}");
        }
        // 17 MB of zeros packs into a few kilobytes: it must not be unpacked past the limit
        let bomb = miniz_oxide::deflate::compress_to_vec(&vec![0u8; MAX_BBQR_BYTES + 1_000_000], 9);
        assert!(bomb.len() < 100_000);
        let encoded = BASE32_NOPAD.encode(&bomb);
        let chunks: Vec<String> = encoded.as_bytes().chunks(4000).map(|c| String::from_utf8(c.to_vec()).unwrap()).collect();
        let total = chunks.len();
        let mut c = BbqrCollector::new();
        let mut last = BbqrUpdate::Ignored(String::new());
        for (i, chunk) in chunks.iter().enumerate() {
            let digits = |n: usize| format!("{}{}", char::from_digit((n / 36) as u32, 36).unwrap(), char::from_digit((n % 36) as u32, 36).unwrap()).to_uppercase();
            last = c.add(&format!("B$ZU{}{}{chunk}", digits(total), digits(i)));
        }
        assert!(matches!(&last, BbqrUpdate::Ignored(why) if why.contains("too large")), "{last:?}");
    }
}
