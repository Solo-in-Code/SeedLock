//! Checks the three ready-made programs that the fixed build environment makes (tools/reproducible), before they
//! leave it: `seedlock-tools check-programs DIR VERSION`. For each program (in DIR, under its release name):
//!
//! - **Linux** (`seedlock-VERSION-linux-x86_64`): an x86-64 program that needs no library of the system (static:
//!   no program interpreter, no shared library), position-independent (loaded at a random address each time),
//!   stripped of its symbols; on an x86-64 Linux it is started too, and must say `seedlock VERSION`.
//! - **Windows** (`seedlock-VERSION-windows-x86_64.exe`): a 64-bit x86-64 console program that needs only libraries
//!   that every Windows has (no runtime of a compiler, such as MinGW's or Visual C++'s, that would have to be
//!   installed first), with Windows's protections on: loaded at a random address (ASLR, also above 4 GB), and its
//!   data never run as code (DEP).
//! - **Mac** (`seedlock-VERSION-macos`): one file with the program for Intel chips and for Apple chips, which start
//!   on macOS 10.12 and 11 and newer, are position-independent, and need only the system's own libraries; the
//!   Apple-chip program carries an ad-hoc signature (without one, a Mac with an Apple chip stops it at once) whose
//!   page hashes fit the program.
//! - **All three**: they hold their version, nothing of the test builds (the stand-in network, which must never be in
//!   a released program), and no folder of the build environment or of a user (`/home/...`, `/Users/...`).

use std::fs;
use std::path::Path;

use bitcoin_hashes::{sha256, Hash};

/// `seedlock-tools check-programs DIR VERSION`
pub fn run(args: &[String]) -> Result<(), String> {
    let [dir, version] = args else {
        return Err("check-programs: give the folder and the version, for example:\n  seedlock-tools check-programs dist 0.3.1".into());
    };
    let mut problems = Vec::new();
    for kind in [Kind::Linux, Kind::Windows, Kind::Mac] {
        let name = kind.file_name(version);
        let path = Path::new(dir).join(&name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                problems.push(format!("{name}: cannot read {}: {e}", path.display()));
                continue;
            }
        };
        println!("{name} ({} KB):", bytes.len() / 1024);
        let mut report = check(kind, &bytes, version);
        if kind == Kind::Linux && cfg!(all(target_os = "linux", target_arch = "x86_64")) && report.problems.is_empty() {
            match std::process::Command::new(&path).arg("--version").output() {
                Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == format!("seedlock {version}") => {
                    report.checked.push(format!("started here, it says \"seedlock {version}\""))
                }
                Ok(out) => report.problems.push(format!(
                    "started here, it says \"{}\" instead of \"seedlock {version}\"",
                    String::from_utf8_lossy(&out.stdout).trim()
                )),
                Err(e) => report.problems.push(format!("it cannot be started here: {e}")),
            }
        }
        for line in &report.checked {
            println!("  {line}");
        }
        problems.extend(report.problems.iter().map(|p| format!("{name}: {p}")));
    }
    if problems.is_empty() {
        println!("All three programs are as they must be.");
        Ok(())
    } else {
        let list: Vec<String> = problems.iter().map(|p| format!("  - {p}")).collect();
        Err(format!("\nThese programs cannot be released:\n{}", list.join("\n")))
    }
}

/// The three programs of a release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Linux,
    Windows,
    Mac,
}

impl Kind {
    /// The program's name in a release (as tools/reproducible/build-programs.sh names it).
    pub fn file_name(self, version: &str) -> String {
        match self {
            Kind::Linux => format!("seedlock-{version}-linux-x86_64"),
            Kind::Windows => format!("seedlock-{version}-windows-x86_64.exe"),
            Kind::Mac => format!("seedlock-{version}-macos"),
        }
    }
}

/// What a check found: what is right, and what is wrong.
#[derive(Debug, Default)]
pub struct Report {
    pub checked: Vec<String>,
    pub problems: Vec<String>,
}

/// Text that must not be in a released program: the stand-in network of the test builds (crates/seedlock-cli,
/// feature test-hooks), the folders of the build environment (the source folder and Cargo's and rustup's, which Rust
/// is told to write as `.`, `~/.cargo` and `/rustc/...`; the home folder), and users' home folders.
const FORBIDDEN: [(&str, &str); 7] = [
    ("SEEDLOCK_TEST_NET_DIR", "the stand-in network of the test builds"),
    ("/seedlock/", "the build environment's source folder"),
    ("/usr/local/cargo", "the build environment's Cargo folder"),
    ("/usr/local/rustup", "the build environment's Rust folder"),
    ("/root/", "the build environment's home folder"),
    ("/home/", "a user's home folder"),
    ("/Users/", "a user's home folder"),
];

/// Checks one program (its bytes) of this version.
pub fn check(kind: Kind, bytes: &[u8], version: &str) -> Report {
    let mut report = Report::default();
    let result = match kind {
        Kind::Linux => check_elf(bytes, &mut report),
        Kind::Windows => check_pe(bytes, &mut report),
        Kind::Mac => check_mac(bytes, &mut report),
    };
    if let Err(e) = result {
        report.problems.push(e);
    }
    if find(bytes, version.as_bytes()).is_some() {
        report.checked.push(format!("holds its version, {version}"));
    } else {
        report.problems.push(format!("the version {version} is not in it: is it a program of another version?"));
    }
    let mut clean = true;
    for (text, what) in FORBIDDEN {
        if find(bytes, text.as_bytes()).is_some() {
            report.problems.push(format!("it contains \"{text}\" ({what})"));
            clean = false;
        }
    }
    if clean {
        report.checked.push("nothing of the test builds, no folder of the build environment or of a user".into());
    }
    report
}

/// Where `needle` first occurs in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn u16_le(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| "the file ends too early".to_string())
}

fn u32_le(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "the file ends too early".to_string())
}

fn u64_le(b: &[u8], at: usize) -> Result<u64, String> {
    b.get(at..at + 8)
        .map(|s| u64::from_le_bytes(s.try_into().expect("8 bytes")))
        .ok_or_else(|| "the file ends too early".to_string())
}

fn u32_be(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "the file ends too early".to_string())
}

/// A zero-terminated name that starts at `at`.
fn c_string(b: &[u8], at: usize) -> Result<String, String> {
    let rest = b.get(at..).ok_or("a name lies outside the file")?;
    let end = rest.iter().position(|&c| c == 0).ok_or("a name has no end")?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

// ---------------------------------------------------------------------------------------------------------------
// Linux: ELF
// ---------------------------------------------------------------------------------------------------------------

const ET_DYN: u16 = 3;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_NEEDED: u64 = 1;
const SHT_SYMTAB: u32 = 2;

fn check_elf(b: &[u8], report: &mut Report) -> Result<(), String> {
    if b.get(..4) != Some(b"\x7fELF".as_slice()) {
        return Err("it is not a Linux program (ELF)".into());
    }
    if b.get(4) != Some(&2) || b.get(5) != Some(&1) {
        return Err("it is not a 64-bit little-endian ELF program".into());
    }
    let machine = u16_le(b, 18)?;
    if machine != 62 {
        return Err(format!("it is for another kind of processor than x86-64 (ELF machine {machine})"));
    }
    report.checked.push("a 64-bit Linux program for x86-64".into());
    // ET_DYN: position-independent (a static-pie program); ET_EXEC: always loaded at the same address
    if u16_le(b, 16)? == ET_DYN {
        report.checked.push("position-independent: loaded at a random address each time".into());
    } else {
        report.problems.push("it is not position-independent: it would always be loaded at the same address".into());
    }
    let (phoff, phentsize, phnum) = (u64_le(b, 32)? as usize, u16_le(b, 54)? as usize, u16_le(b, 56)? as usize);
    let mut needed = 0;
    let mut interpreter = false;
    for i in 0..phnum {
        let at = phoff + i * phentsize;
        match u32_le(b, at)? {
            PT_INTERP => interpreter = true,
            PT_DYNAMIC => {
                let (offset, size) = (u64_le(b, at + 8)? as usize, u64_le(b, at + 32)? as usize);
                for entry in (offset..offset + size).step_by(16) {
                    match u64_le(b, entry)? {
                        0 => break,
                        DT_NEEDED => needed += 1,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if interpreter || needed > 0 {
        report.problems.push(format!(
            "it is not static: it needs {} (the static program needs nothing of the system)",
            if interpreter { "a program interpreter (ld.so)".to_string() } else { format!("{needed} shared libraries") }
        ));
    } else {
        report.checked.push("static: it needs no library of the system".into());
    }
    let (shoff, shentsize, shnum) = (u64_le(b, 40)? as usize, u16_le(b, 58)? as usize, u16_le(b, 60)? as usize);
    let mut symbols = false;
    for i in 0..shnum {
        symbols |= u32_le(b, shoff + i * shentsize + 4)? == SHT_SYMTAB;
    }
    if symbols {
        report.problems.push("it still has its symbol table (it is not stripped)".into());
    } else {
        report.checked.push("stripped of its symbols".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Windows: PE
// ---------------------------------------------------------------------------------------------------------------

/// DllCharacteristics of a PE program: Windows's protections.
const HIGH_ENTROPY_VA: u16 = 0x20;
const DYNAMIC_BASE: u16 = 0x40;
const NX_COMPAT: u16 = 0x100;

/// The libraries that every Windows (10 and newer) has. A program that needs any other one (MinGW's libgcc_s or
/// libwinpthread, Visual C++'s vcruntime140) does not start on a computer where it was not installed first.
const WINDOWS_LIBRARIES: [&str; 17] = [
    "kernel32.dll",
    "kernelbase.dll",
    "ntdll.dll",
    "advapi32.dll",
    "bcrypt.dll",
    "bcryptprimitives.dll",
    "userenv.dll",
    "ws2_32.dll",
    "msvcrt.dll",
    "ucrtbase.dll",
    "shell32.dll",
    "ole32.dll",
    "oleaut32.dll",
    "user32.dll",
    "secur32.dll",
    "crypt32.dll",
    "synchronization.dll",
];

/// Whether every Windows has this library (the "api-ms-win-..." names are parts of Windows itself).
pub fn windows_has(dll: &str) -> bool {
    let dll = dll.to_ascii_lowercase();
    dll.starts_with("api-ms-win-") || WINDOWS_LIBRARIES.contains(&dll.as_str())
}

fn check_pe(b: &[u8], report: &mut Report) -> Result<(), String> {
    if b.get(..2) != Some(b"MZ".as_slice()) {
        return Err("it is not a Windows program".into());
    }
    let pe = u32_le(b, 0x3c)? as usize;
    if b.get(pe..pe + 4) != Some(b"PE\0\0".as_slice()) {
        return Err("it is not a Windows program (no PE header)".into());
    }
    let coff = pe + 4;
    let machine = u16_le(b, coff)?;
    let sections = u16_le(b, coff + 2)? as usize;
    let optional_size = u16_le(b, coff + 16)? as usize;
    let opt = coff + 20;
    if machine != 0x8664 || u16_le(b, opt)? != 0x20b {
        return Err(format!("it is not a 64-bit program for x86-64 (machine {machine:#x})"));
    }
    let subsystem = u16_le(b, opt + 68)?;
    if subsystem == 3 {
        report.checked.push("a 64-bit Windows console program for x86-64".into());
    } else {
        report.problems.push(format!("it is not a console program (subsystem {subsystem})"));
    }
    let characteristics = u16_le(b, opt + 70)?;
    let missing: Vec<&str> = [
        (DYNAMIC_BASE, "loaded at a random address (ASLR)"),
        (HIGH_ENTROPY_VA, "random addresses also above 4 GB"),
        (NX_COMPAT, "data never run as code (DEP)"),
    ]
    .iter()
    .filter(|(flag, _)| characteristics & flag == 0)
    .map(|(_, what)| *what)
    .collect();
    if missing.is_empty() {
        report.checked.push("with Windows's protections: loaded at a random address (ASLR), data never run as code (DEP)".into());
    } else {
        report.problems.push(format!("Windows's protections are not all on, missing: {}", missing.join(", ")));
    }
    let directories = u32_le(b, opt + 108)? as usize;
    let section_table = opt + optional_size;
    let to_offset = |rva: u32| -> Result<usize, String> {
        for i in 0..sections {
            let s = section_table + i * 40;
            let (size, address) = (u32_le(b, s + 8)?.max(u32_le(b, s + 16)?), u32_le(b, s + 12)?);
            if rva >= address && rva < address + size {
                return Ok((rva - address + u32_le(b, s + 20)?) as usize);
            }
        }
        Err(format!("the address {rva:#x} is in none of its sections"))
    };
    let mut dlls = Vec::new();
    // the import table (data directory 1): 20 bytes per library, the name's address at 12
    if directories > 1 && u32_le(b, opt + 112 + 8)? != 0 {
        let mut at = to_offset(u32_le(b, opt + 112 + 8)?)?;
        loop {
            let name = u32_le(b, at + 12)?;
            if name == 0 {
                break;
            }
            dlls.push(c_string(b, to_offset(name)?)?);
            at += 20;
        }
    }
    // the delay-load import table (data directory 13): 32 bytes per library, the name's address at 4
    if directories > 13 && u32_le(b, opt + 112 + 13 * 8)? != 0 {
        let mut at = to_offset(u32_le(b, opt + 112 + 13 * 8)?)?;
        loop {
            let name = u32_le(b, at + 4)?;
            if name == 0 {
                break;
            }
            dlls.push(c_string(b, to_offset(name)?)?);
            at += 32;
        }
    }
    dlls.sort_by_key(|d| d.to_ascii_lowercase());
    dlls.dedup_by_key(|d| d.to_ascii_lowercase());
    let others: Vec<&String> = dlls.iter().filter(|d| !windows_has(d)).collect();
    if others.is_empty() {
        report.checked.push(format!("needs only libraries that every Windows has: {}", dlls.join(", ")));
    } else {
        let names: Vec<&str> = others.iter().map(|d| d.as_str()).collect();
        report.problems.push(format!(
            "it needs libraries that Windows does not have by itself: {} (it would not start on most computers)",
            names.join(", ")
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// The Mac: Mach-O
// ---------------------------------------------------------------------------------------------------------------

const CPU_X86_64: u32 = 0x0100_0007;
const CPU_ARM64: u32 = 0x0100_000C;
const LC_LOAD_DYLIB: u32 = 0xC;
const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const LC_REEXPORT_DYLIB: u32 = 0x8000_001F;
const LC_CODE_SIGNATURE: u32 = 0x1D;
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const LC_BUILD_VERSION: u32 = 0x32;
const CS_ADHOC: u32 = 0x2;
const MH_PIE: u32 = 0x0020_0000;

/// The oldest macOS each half starts on, as Rust builds for them: 10.12 on Intel chips, 11.0 on Apple chips.
const OLDEST_INTEL: u32 = 10 << 16 | 12 << 8;
const OLDEST_APPLE: u32 = 11 << 16;

fn macos_version(v: u32) -> String {
    match v & 0xff {
        0 => format!("{}.{}", v >> 16, (v >> 8) & 0xff),
        patch => format!("{}.{}.{patch}", v >> 16, (v >> 8) & 0xff),
    }
}

fn check_mac(b: &[u8], report: &mut Report) -> Result<(), String> {
    if u32_be(b, 0)? != 0xCAFE_BABE {
        return Err("it is not one file for both kinds of Mac (a universal Mach-O file)".into());
    }
    let count = u32_be(b, 4)? as usize;
    let mut halves = Vec::new();
    for i in 0..count {
        let at = 8 + i * 20;
        let (cpu, offset, size) = (u32_be(b, at)?, u32_be(b, at + 8)? as usize, u32_be(b, at + 12)? as usize);
        let slice = b.get(offset..offset + size).ok_or("one of its programs lies outside the file")?;
        halves.push((cpu, slice));
    }
    let cpus: Vec<u32> = halves.iter().map(|(cpu, _)| *cpu).collect();
    if cpus.len() != 2 || !cpus.contains(&CPU_X86_64) || !cpus.contains(&CPU_ARM64) {
        return Err("it does not hold exactly the program for Intel chips and the one for Apple chips".into());
    }
    report.checked.push("one file with the program for Intel chips and the one for Apple chips".into());
    for (cpu, slice) in halves {
        let (name, oldest) = if cpu == CPU_ARM64 { ("Apple chips", OLDEST_APPLE) } else { ("Intel chips", OLDEST_INTEL) };
        check_mac_half(slice, cpu, name, oldest, report)?;
    }
    Ok(())
}

fn check_mac_half(b: &[u8], cpu: u32, name: &str, oldest: u32, report: &mut Report) -> Result<(), String> {
    if u32_le(b, 0)? != 0xFEED_FACF || u32_le(b, 4)? != cpu || u32_le(b, 12)? != 2 {
        return Err(format!("the program for {name} is not a 64-bit Mac program"));
    }
    if u32_le(b, 24)? & MH_PIE == 0 {
        report.problems.push(format!("the program for {name} is not position-independent"));
    }
    let ncmds = u32_le(b, 16)? as usize;
    let mut at = 32;
    let mut minimum = None;
    let mut libraries = Vec::new();
    let mut signature = None;
    for _ in 0..ncmds {
        let (cmd, size) = (u32_le(b, at)?, u32_le(b, at + 4)? as usize);
        match cmd {
            LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB | LC_REEXPORT_DYLIB => libraries.push(c_string(b, at + u32_le(b, at + 8)? as usize)?),
            LC_VERSION_MIN_MACOSX => minimum = Some(u32_le(b, at + 8)?),
            LC_BUILD_VERSION => minimum = Some(u32_le(b, at + 12)?),
            LC_CODE_SIGNATURE => signature = Some((u32_le(b, at + 8)? as usize, u32_le(b, at + 12)? as usize)),
            _ => {}
        }
        if size < 8 {
            return Err(format!("the program for {name} has a broken load command"));
        }
        at += size;
    }
    match minimum {
        Some(v) if v == oldest => report.checked.push(format!(
            "the program for {name} starts on macOS {} and newer, and is position-independent",
            macos_version(v)
        )),
        Some(v) => report.problems.push(format!(
            "the program for {name} needs macOS {} or newer, instead of {}",
            macos_version(v),
            macos_version(oldest)
        )),
        None => report.problems.push(format!("the program for {name} does not say which macOS it needs")),
    }
    let others: Vec<&String> = libraries.iter().filter(|l| !l.starts_with("/usr/lib/") && !l.starts_with("/System/Library/")).collect();
    if others.is_empty() {
        report.checked.push(format!("the program for {name} needs only the system's own libraries ({})", libraries.len()));
    } else {
        let names: Vec<&str> = others.iter().map(|l| l.as_str()).collect();
        report.problems.push(format!("the program for {name} needs libraries that macOS does not have: {}", names.join(", ")));
    }
    match (signature, cpu == CPU_ARM64) {
        (Some((offset, size)), _) => match check_signature(b, offset, size) {
            Ok(pages) => report.checked.push(format!("the program for {name} is signed (ad hoc), and all {pages} page hashes fit")),
            Err(e) => report.problems.push(format!("the program for {name}: {e}")),
        },
        (None, true) => report.problems.push(format!(
            "the program for {name} is not signed: a Mac with an Apple chip would stop it at once"
        )),
        (None, false) => report.checked.push(format!("the program for {name} is not signed (Intel Macs do not need it)")),
    }
    Ok(())
}

/// Checks an ad-hoc code signature: its code directory's hash of each page of the program (up to the signature)
/// must be the page's SHA-256, as the Mac checks it when it loads the page. Returns the number of pages.
pub fn check_signature(b: &[u8], offset: usize, size: usize) -> Result<usize, String> {
    let blob = b.get(offset..offset + size).ok_or("its signature lies outside the file")?;
    if u32_be(blob, 0)? != 0xFADE_0CC0 {
        return Err("its signature is not a code signature".into());
    }
    let count = u32_be(blob, 8)? as usize;
    for i in 0..count {
        if u32_be(blob, 12 + i * 8)? != 0 {
            continue; // not the code directory
        }
        let cd = blob.get(u32_be(blob, 16 + i * 8)? as usize..).ok_or("its code directory lies outside the signature")?;
        if u32_be(cd, 0)? != 0xFADE_0C02 {
            return Err("its code directory is broken".into());
        }
        let flags = u32_be(cd, 12)?;
        let hash_offset = u32_be(cd, 16)? as usize;
        let pages = u32_be(cd, 28)? as usize;
        let code_limit = u32_be(cd, 32)? as usize;
        let (hash_size, hash_type, page_shift) = (cd.get(36).copied(), cd.get(37).copied(), cd.get(39).copied());
        if hash_type != Some(2) || hash_size != Some(32) {
            return Err("its signature does not use SHA-256".into());
        }
        if flags & CS_ADHOC == 0 {
            return Err("its signature is not an ad-hoc one".into());
        }
        if code_limit != offset {
            return Err("its signature does not cover the whole program".into());
        }
        let page = 1usize << page_shift.unwrap_or(12).min(20);
        if pages != code_limit.div_ceil(page) {
            return Err("its signature does not have one hash for each page".into());
        }
        for p in 0..pages {
            let want = cd.get(hash_offset + p * 32..hash_offset + (p + 1) * 32).ok_or("its page hashes are cut off")?;
            let got = sha256::Hash::hash(&b[p * page..((p + 1) * page).min(code_limit)]);
            if want != got.as_byte_array() {
                return Err(format!("its signature does not fit the program (page {p}): the Mac would stop it"));
            }
        }
        return Ok(pages);
    }
    Err("its signature has no code directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSION: &str = "9.8.7";

    fn put16(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put64(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// A minimal 64-bit x86-64 ELF program: two program headers (PT_LOAD, and `second`), one section of type `section`;
    /// position-independent (ET_DYN).
    fn elf(second: u32, needed: bool, section: u32) -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        put16(&mut b, 16, ET_DYN);
        put16(&mut b, 18, 62);
        put64(&mut b, 32, 64); // program headers at 64
        put64(&mut b, 40, 256); // section headers at 256
        put16(&mut b, 54, 56);
        put16(&mut b, 56, 2);
        put16(&mut b, 58, 64);
        put16(&mut b, 60, 1);
        put32(&mut b, 64, 1); // PT_LOAD
        put32(&mut b, 120, second);
        put64(&mut b, 120 + 8, 400); // its contents: the dynamic entries, at 400
        put64(&mut b, 120 + 32, 48);
        if needed {
            put64(&mut b, 400, DT_NEEDED);
        } else {
            put64(&mut b, 400, 0x1e); // DT_FLAGS
        }
        put32(&mut b, 256 + 4, section);
        b.extend_from_slice(VERSION.as_bytes());
        b
    }

    #[test]
    fn the_linux_program_must_be_static_and_stripped() {
        let good = check(Kind::Linux, &elf(PT_DYNAMIC, false, 3), VERSION);
        assert!(good.problems.is_empty(), "{:?}", good.problems);
        assert!(good.checked.iter().any(|c| c.contains("static")));
        let interp = check(Kind::Linux, &elf(PT_INTERP, false, 3), VERSION);
        assert!(interp.problems.iter().any(|p| p.contains("program interpreter")), "{:?}", interp.problems);
        let shared = check(Kind::Linux, &elf(PT_DYNAMIC, true, 3), VERSION);
        assert!(shared.problems.iter().any(|p| p.contains("1 shared libraries")), "{:?}", shared.problems);
        let symbols = check(Kind::Linux, &elf(PT_DYNAMIC, false, SHT_SYMTAB), VERSION);
        assert!(symbols.problems.iter().any(|p| p.contains("symbol table")), "{:?}", symbols.problems);
        let mut fixed = elf(PT_DYNAMIC, false, 3);
        put16(&mut fixed, 16, 2); // ET_EXEC
        let fixed = check(Kind::Linux, &fixed, VERSION);
        assert!(fixed.problems.iter().any(|p| p.contains("not position-independent")), "{:?}", fixed.problems);
        assert!(!check(Kind::Linux, b"MZ not an ELF file", VERSION).problems.is_empty());
    }

    /// A minimal 64-bit x86-64 PE program whose import table names these libraries, with Windows's protections on.
    fn pe(dlls: &[&str], subsystem: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[..2].copy_from_slice(b"MZ");
        put32(&mut b, 0x3c, 0x40);
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        put16(&mut b, coff, 0x8664);
        put16(&mut b, coff + 2, 1); // one section
        put16(&mut b, coff + 16, 240); // the optional header's size
        let opt = coff + 20;
        put16(&mut b, opt, 0x20b);
        put16(&mut b, opt + 68, subsystem);
        put16(&mut b, opt + 70, DYNAMIC_BASE | HIGH_ENTROPY_VA | NX_COMPAT);
        put32(&mut b, opt + 108, 16);
        put32(&mut b, opt + 112 + 8, 0x1000); // the import table, at address 0x1000
        let section = opt + 240;
        b[section..section + 6].copy_from_slice(b".idata");
        put32(&mut b, section + 8, 0x200);
        put32(&mut b, section + 12, 0x1000); // the section's address
        put32(&mut b, section + 16, 0x200);
        put32(&mut b, section + 20, 0x200); // ... at 0x200 in the file
        let mut name = 0x300;
        for (i, dll) in dlls.iter().enumerate() {
            put32(&mut b, 0x200 + i * 20 + 12, 0x1000 + name as u32 - 0x200);
            b[name..name + dll.len()].copy_from_slice(dll.as_bytes());
            name += dll.len() + 1;
        }
        b.extend_from_slice(VERSION.as_bytes());
        b
    }

    #[test]
    fn the_windows_program_needs_only_what_every_windows_has() {
        let good = check(Kind::Windows, &pe(&["KERNEL32.dll", "ntdll.dll", "api-ms-win-crt-runtime-l1-1-0.dll"], 3), VERSION);
        assert!(good.problems.is_empty(), "{:?}", good.problems);
        assert!(good.checked.iter().any(|c| c.contains("KERNEL32.dll, ntdll.dll")), "{:?}", good.checked);
        let mingw = check(Kind::Windows, &pe(&["KERNEL32.dll", "libgcc_s_seh-1.dll", "libwinpthread-1.dll"], 3), VERSION);
        assert!(mingw.problems.iter().any(|p| p.contains("libgcc_s_seh-1.dll, libwinpthread-1.dll")), "{:?}", mingw.problems);
        assert!(!check(Kind::Windows, &pe(&["VCRUNTIME140.dll"], 3), VERSION).problems.is_empty());
        let window = check(Kind::Windows, &pe(&["KERNEL32.dll"], 2), VERSION);
        assert!(window.problems.iter().any(|p| p.contains("console")), "{:?}", window.problems);
        let mut unprotected = pe(&["KERNEL32.dll"], 3);
        put16(&mut unprotected, 0x58 + 70, NX_COMPAT); // no ASLR
        let unprotected = check(Kind::Windows, &unprotected, VERSION);
        assert!(
            unprotected.problems.iter().any(|p| p.contains("missing: loaded at a random address (ASLR), random addresses also above 4 GB")),
            "{:?}",
            unprotected.problems
        );
        assert!(windows_has("Api-MS-Win-Core-Synch-L1-2-0.DLL") && !windows_has("libstdc++-6.dll"));
    }

    /// A minimal Mac program for one kind of processor: its oldest macOS, the libraries it loads, and (`signed`) an
    /// ad-hoc signature with 4 KiB pages.
    fn mac_half(cpu: u32, minimum: u32, libraries: &[&str], signed: bool) -> Vec<u8> {
        let mut b = vec![0u8; 6000];
        put32(&mut b, 0, 0xFEED_FACF);
        put32(&mut b, 4, cpu);
        put32(&mut b, 12, 2);
        put32(&mut b, 24, MH_PIE);
        let mut at = 32;
        let mut ncmds = 0;
        let mut command = |b: &mut Vec<u8>, cmd: u32, body: &[u8]| {
            let size = (8 + body.len()).div_ceil(8) * 8;
            put32(b, at, cmd);
            put32(b, at + 4, size as u32);
            b[at + 8..at + 8 + body.len()].copy_from_slice(body);
            at += size;
            ncmds += 1;
        };
        let mut build = vec![0u8; 16];
        build[..4].copy_from_slice(&1u32.to_le_bytes());
        build[4..8].copy_from_slice(&minimum.to_le_bytes());
        command(&mut b, LC_BUILD_VERSION, &build);
        for library in libraries {
            let mut body = vec![0u8; 16];
            body[..4].copy_from_slice(&24u32.to_le_bytes()); // the name, after the 24 bytes of the command
            body.extend_from_slice(library.as_bytes());
            body.push(0);
            command(&mut b, LC_LOAD_DYLIB, &body);
        }
        if signed {
            // at 4096, 96 bytes: the SuperBlob (12), its one index entry (8), the CodeDirectory (44) and its hash (32)
            let mut body = vec![0u8; 8];
            body[..4].copy_from_slice(&4096u32.to_le_bytes());
            body[4..].copy_from_slice(&96u32.to_le_bytes());
            command(&mut b, LC_CODE_SIGNATURE, &body);
        }
        put32(&mut b, 16, ncmds);
        b[1000..1000 + VERSION.len()].copy_from_slice(VERSION.as_bytes()); // after the load commands
        b.truncate(4096);
        if signed {
            // a SuperBlob with one CodeDirectory: one hash (the 4096 bytes before it are one page)
            let hash = sha256::Hash::hash(&b[..4096]);
            let mut cd = vec![0u8; 44];
            cd[0..4].copy_from_slice(&0xFADE_0C02u32.to_be_bytes());
            cd[12..16].copy_from_slice(&(CS_ADHOC | 0x20000).to_be_bytes());
            cd[16..20].copy_from_slice(&44u32.to_be_bytes()); // the hashes, right after
            cd[28..32].copy_from_slice(&1u32.to_be_bytes());
            cd[32..36].copy_from_slice(&4096u32.to_be_bytes());
            cd[36] = 32;
            cd[37] = 2;
            cd[39] = 12;
            cd.extend_from_slice(hash.as_byte_array());
            let mut blob = Vec::new();
            blob.extend_from_slice(&0xFADE_0CC0u32.to_be_bytes());
            blob.extend_from_slice(&((20 + cd.len()) as u32).to_be_bytes());
            blob.extend_from_slice(&1u32.to_be_bytes());
            blob.extend_from_slice(&0u32.to_be_bytes());
            blob.extend_from_slice(&20u32.to_be_bytes());
            blob.extend_from_slice(&cd);
            assert_eq!(blob.len(), 96);
            b.extend_from_slice(&blob);
        }
        b
    }

    fn fat(halves: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut b = vec![0u8; 0x1000];
        b[..4].copy_from_slice(&0xCAFE_BABEu32.to_be_bytes());
        b[4..8].copy_from_slice(&(halves.len() as u32).to_be_bytes());
        for (i, (cpu, half)) in halves.iter().enumerate() {
            let offset = b.len().div_ceil(0x1000) * 0x1000;
            b.resize(offset, 0);
            let at = 8 + i * 20;
            b[at..at + 4].copy_from_slice(&cpu.to_be_bytes());
            b[at + 8..at + 12].copy_from_slice(&(offset as u32).to_be_bytes());
            b[at + 12..at + 16].copy_from_slice(&(half.len() as u32).to_be_bytes());
            b.extend_from_slice(half);
        }
        b
    }

    const SYSTEM: [&str; 2] = ["/usr/lib/libSystem.B.dylib", "/usr/lib/libiconv.2.dylib"];

    #[test]
    fn the_mac_program_holds_both_halves_with_their_oldest_macos_and_a_signature_that_fits() {
        let good = fat(&[
            (CPU_X86_64, mac_half(CPU_X86_64, OLDEST_INTEL, &SYSTEM, false)),
            (CPU_ARM64, mac_half(CPU_ARM64, OLDEST_APPLE, &SYSTEM, true)),
        ]);
        let report = check(Kind::Mac, &good, VERSION);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(report.checked.iter().any(|c| c.contains("starts on macOS 10.12 and newer")), "{:?}", report.checked);
        assert!(report.checked.iter().any(|c| c.contains("starts on macOS 11.0 and newer")), "{:?}", report.checked);
        assert!(report.checked.iter().any(|c| c.contains("all 1 page hashes fit")), "{:?}", report.checked);

        // macOS 13 (what zig makes by itself), a library from elsewhere, no signature on the Apple-chip half
        let wrong = fat(&[
            (CPU_X86_64, mac_half(CPU_X86_64, 13 << 16, &SYSTEM, false)),
            (CPU_ARM64, mac_half(CPU_ARM64, OLDEST_APPLE, &["/opt/homebrew/lib/libfoo.dylib"], false)),
        ]);
        let problems = check(Kind::Mac, &wrong, VERSION).problems.join("\n");
        assert!(problems.contains("needs macOS 13.0 or newer, instead of 10.12"), "{problems}");
        assert!(problems.contains("/opt/homebrew/lib/libfoo.dylib"), "{problems}");
        assert!(problems.contains("is not signed: a Mac with an Apple chip would stop it"), "{problems}");

        // a byte changed after signing
        let mut changed = good.clone();
        let arm = u32::from_be_bytes(changed[8 + 20 + 8..8 + 20 + 12].try_into().unwrap()) as usize;
        changed[arm + 200] ^= 1;
        let problems = check(Kind::Mac, &changed, VERSION).problems.join("\n");
        assert!(problems.contains("does not fit the program (page 0)"), "{problems}");

        // only one half
        let one = fat(&[(CPU_ARM64, mac_half(CPU_ARM64, OLDEST_APPLE, &SYSTEM, true))]);
        assert!(check(Kind::Mac, &one, VERSION).problems.iter().any(|p| p.contains("exactly")));

        // not position-independent (the Intel half, which is not signed, so that the change touches nothing else)
        let mut intel = mac_half(CPU_X86_64, OLDEST_INTEL, &SYSTEM, false);
        put32(&mut intel, 24, 0);
        let fixed = fat(&[(CPU_X86_64, intel), (CPU_ARM64, mac_half(CPU_ARM64, OLDEST_APPLE, &SYSTEM, true))]);
        let problems = check(Kind::Mac, &fixed, VERSION).problems.join("\n");
        assert!(problems.contains("the program for Intel chips is not position-independent"), "{problems}");
    }

    fn read(path: &str) -> String {
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)).unwrap()
    }

    #[test]
    fn the_scripts_name_the_programs_and_the_oldest_macos_as_the_check_does() {
        // the names, with the version as each script writes it
        for (script, version) in [
            ("tools/reproducible/build-programs.sh", "$version"),
            ("tools/build-release.sh", "$version"),
            ("tools/build-release.ps1", "$version"),
        ] {
            let text = read(script);
            for kind in [Kind::Linux, Kind::Windows, Kind::Mac] {
                assert!(text.contains(&kind.file_name(version)), "{script} does not name {}", kind.file_name(version));
            }
        }
        // zig.sh makes the Mac program for the oldest versions that check_mac wants
        let zig = read("tools/reproducible/zig.sh");
        assert!(zig.contains(&format!("arg=x86_64-macos.{}-none", macos_version(OLDEST_INTEL))), "{zig}");
        assert!(zig.contains(&format!("arg=aarch64-macos.{}-none", macos_version(OLDEST_APPLE))), "{zig}");
    }

    #[test]
    fn every_program_holds_its_version_and_nothing_of_the_tests_or_the_build_folders() {
        let mut linux = elf(PT_DYNAMIC, false, 3);
        assert!(check(Kind::Linux, &linux, "1.2.3").problems.iter().any(|p| p.contains("version 1.2.3 is not in it")));
        linux.extend_from_slice(b"SEEDLOCK_TEST_NET_DIR /usr/local/cargo/registry/src/x /home/me");
        let problems = check(Kind::Linux, &linux, VERSION).problems.join("\n");
        for wanted in ["stand-in network", "Cargo folder", "a user's home folder"] {
            assert!(problems.contains(wanted), "{wanted}: {problems}");
        }
        assert_eq!(Kind::Mac.file_name("0.3.1"), "seedlock-0.3.1-macos");
        assert_eq!(Kind::Windows.file_name("0.3.1"), "seedlock-0.3.1-windows-x86_64.exe");
        assert_eq!(macos_version(10 << 16 | 12 << 8), "10.12");
        assert_eq!(macos_version(11 << 16 | 2 << 8 | 3), "11.2.3");
    }
}
