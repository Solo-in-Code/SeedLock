//! Checks a release APK of SeedLock's phone app before it is published, and puts it into `dist/` under its
//! release name (DEVELOPMENT.md, Making a release). What it checks:
//!
//! - it is the app (`org.seedlock.app`) of this version (the versionName and versionCode that
//!   build.gradle.kts makes from Cargo.toml), and not a build that can be debugged or is for tests only;
//! - it asks Android only for the camera and the network state, never for INTERNET;
//! - it holds the Rust library and JNA's library for the three kinds of phone processor, and no other;
//! - its code is SeedLock's, Kotlin's, JNA's and JetBrains' annotations' only: the libraries its license
//!   notices name;
//! - it carries the license notices of this version;
//! - it is signed (APK Signature Scheme v2 or v3) with SeedLock's release certificate, not with Android
//!   Studio's test key;
//! - neither the path of this computer's home folder (and with it the user name) nor a folder of the fixed
//!   build environment is in it;
//! - without its signature, it is byte for byte the app built in the fixed build environment
//!   (`dist/seedlock-VERSION-android-unsigned.apk`, tools/reproducible).
//!
//! It reads the signature's certificate but does not check the signature itself: Android does that when it
//! installs the app (and `apksigner verify` from the Android SDK can too, as tools/sign-apk.sh does).
//!
//! `check-apk --unsigned FILE` checks the app as the fixed build environment makes it, before it is signed:
//! the same, but no signature yet, and none of the environment's folders inside. `unsigned-apk SIGNED OUT`
//! writes a signed APK without its signature, as it was before it was signed: so anyone can compare a
//! release's app with the one they built themselves.
//!
//! Signing an APK (with APK Signature Scheme v2 or v3) changes nothing in it but this: an APK Signing Block
//! is put between the files and the ZIP archive's central directory (apksigner first pads the files with
//! zeros, so that the block starts at a multiple of 4096 bytes), and the end record, which says where the
//! central directory starts, is changed to match. Taking the block (and that padding) off gives back the APK
//! as it was before it was signed.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use bitcoin_hashes::{sha256, Hash};

use crate::notices;

/// The SHA-256 fingerprint of the certificate of SeedLock's Android signing key (as `keytool -list -v`
/// shows it), published in android/README.md and DEVELOPMENT.md. (None was only for before that key existed.)
pub const RELEASE_CERTIFICATE: Option<&str> =
    Some("02:27:2A:CC:06:84:2D:36:3A:19:A6:CF:48:3D:D9:28:5F:CB:CA:C5:38:AB:98:24:51:14:35:2A:B9:68:63:44");

/// The application id of the released app (test builds are `org.seedlock.app.debug`).
pub const APP_ID: &str = "org.seedlock.app";

/// The kinds of phone processor the app is built for (android/app/build.gradle.kts, abiFilters).
pub const ABIS: [&str; 3] = ["arm64-v8a", "armeabi-v7a", "x86_64"];

/// The native libraries in the app, for each kind of processor: SeedLock's and JNA's.
pub const LIBRARIES: [&str; 2] = ["libseedlock_ffi.so", "libjnidispatch.so"];

/// The Java packages the app's code may be in: SeedLock's own, Kotlin's standard library, JNA, and the
/// annotations that Kotlin's standard library uses.
const CODE: [(&str, &str); 6] = [
    ("org/seedlock/app/", "SeedLock"),
    ("org/seedlock/ffi/", "SeedLock"),
    ("kotlin/", "Kotlin"),
    ("com/sun/jna/", "JNA"),
    ("org/jetbrains/annotations/", "JetBrains' annotations"),
    ("org/intellij/lang/annotations/", "JetBrains' annotations"),
];

/// The version code that android/app/build.gradle.kts gives a version: 0.2.1 is 201.
pub fn version_code(version: &str) -> Option<u32> {
    let parts: Vec<u32> = version.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts.as_slice() {
        [major, minor, patch] if *minor < 100 && *patch < 100 => Some(major * 10_000 + minor * 100 + patch),
        _ => None,
    }
}

/// The fixed build environment's folders (tools/reproducible): none of them may be inside the app it builds.
/// ("/seedlock/" alone would also find the app's own Java packages, org/seedlock/...)
pub const ENVIRONMENT_FOLDERS: [&str; 6] =
    ["/seedlock/android/", "/seedlock/crates/", "/seedlock/target/", "/usr/local/cargo/", "/usr/local/rustup/", "/opt/android-sdk/"];

/// Where the fixed build environment's app is, before it is signed: tools/build-release.sh puts it there.
pub fn unsigned_name(version: &str) -> String {
    format!("seedlock-{version}-android-unsigned.apk")
}

/// `seedlock-tools check-apk [--unsigned] FILE`
pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [flag, path] if flag == "--unsigned" => run_unsigned(path),
        [path] if !path.starts_with("--") => run_signed(path),
        _ => Err("check-apk: give the APK's file, for example:\n  seedlock-tools check-apk dist/seedlock-X.Y.Z-android.apk\n\
                  or, for the app the fixed build environment made, before it is signed:\n  \
                  seedlock-tools check-apk --unsigned dist/seedlock-X.Y.Z-android-unsigned.apk"
            .into()),
    }
}

/// `seedlock-tools check-apk --unsigned FILE`: the app as the fixed build environment makes it.
fn run_unsigned(path: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let version = env!("CARGO_PKG_VERSION");
    println!("Checking {path} ({} KB) for SeedLock {version}, before it is signed ...", bytes.len() / 1024);
    let (_, signed) = without_signature(&bytes)?;
    if signed {
        return Err("\nIt is signed already: this check is for the app before it is signed (check-apk without --unsigned \
                    checks a signed one)."
            .into());
    }
    let mut apk = Apk::default();
    check_contents(&Zip::read(&bytes)?, version, &forbidden_folders(), &mut apk)?;
    for line in &apk.checked {
        println!("  {line}");
    }
    if !apk.problems.is_empty() {
        let list: Vec<String> = apk.problems.iter().map(|p| format!("  - {p}")).collect();
        return Err(format!("\nThis app cannot be released:\n{}", list.join("\n")));
    }
    println!("  Not signed yet, and nothing between its files and its central directory");
    println!("SHA-256: {}", sha256::Hash::hash(&bytes));
    Ok(())
}

/// `seedlock-tools unsigned-apk SIGNED OUT`: a signed APK without its signature, as it was before it was signed.
pub fn run_take_off_signature(args: &[String]) -> Result<(), String> {
    let [input, output] = args else {
        return Err("unsigned-apk: give the signed APK and the file to write, for example:\n  \
                    seedlock-tools unsigned-apk seedlock-X.Y.Z-android.apk without-signature.apk"
            .into());
    };
    let bytes = fs::read(input).map_err(|e| format!("cannot read {input}: {e}"))?;
    let (unsigned, signed) = without_signature(&bytes)?;
    if !signed {
        return Err(format!("{input} is not signed (it has no APK Signing Block)."));
    }
    let zip = Zip::read(&bytes)?;
    let (certificate, scheme, _) = signing_certificate(&bytes, zip.central_directory)?;
    let certificate = fingerprint(&certificate);
    println!("{input}: signed (APK Signature Scheme {scheme}) with the certificate");
    println!("  SHA-256 {certificate}");
    match RELEASE_CERTIFICATE {
        Some(expected) if same_fingerprint(expected, &certificate) => println!("  That is SeedLock's release certificate."),
        _ => println!("  That is NOT SeedLock's release certificate."),
    }
    fs::write(output, &unsigned).map_err(|e| format!("cannot write {output}: {e}"))?;
    println!("Without its signature: {output} ({} KB), SHA-256 {}", unsigned.len() / 1024, sha256::Hash::hash(&unsigned));
    Ok(())
}

/// `seedlock-tools check-apk FILE`: a signed release APK.
fn run_signed(path: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let version = env!("CARGO_PKG_VERSION");
    println!("Checking {path} ({} KB) for SeedLock {version} ...", bytes.len() / 1024);
    let apk = check(&bytes, version, &forbidden_folders())?;
    for line in &apk.checked {
        println!("  {line}");
    }
    println!("  Signed (APK Signature Scheme {}) with the certificate", apk.scheme);
    println!("    SHA-256 {}", apk.certificate);
    if !apk.names.is_empty() {
        println!("    {}", apk.names.join(", "));
    }
    if !apk.problems.is_empty() {
        let list: Vec<String> = apk.problems.iter().map(|p| format!("  - {p}")).collect();
        return Err(format!("\nThis APK cannot be released:\n{}", list.join("\n")));
    }
    let Some(expected) = RELEASE_CERTIFICATE else {
        return Err(format!(
            "\nEverything else is right, but SeedLock's release certificate is not written into this check yet, so \
             it cannot say whether this is the one. If this APK is signed with SeedLock's own Android key, this is \
             the fingerprint of that key's certificate:\n  {}\nIt goes into crates/seedlock-tools/src/apk.rs \
             (RELEASE_CERTIFICATE) and into the README and the guides: DEVELOPMENT.md, The Android signing key.",
            apk.certificate
        ));
    };
    if !same_fingerprint(expected, &apk.certificate) {
        return Err(format!(
            "\nThis APK is signed with another key than SeedLock's own:\n  this one   {}\n  SeedLock's {expected}\n\
             Sign it again with SeedLock's Android key (DEVELOPMENT.md, The Android signing key).",
            apk.certificate
        ));
    }
    println!("  That is SeedLock's release certificate.");
    // the app the fixed build environment made, which this one must be with a signature added, and nothing else
    let built = PathBuf::from("dist").join(unsigned_name(version));
    let Ok(unsigned) = fs::read(&built) else {
        return Err(format!(
            "\n{} is not there. A release's app is the one built in the fixed build environment: build it with \
             tools/build-release.sh, sign it with tools/sign-apk.sh, then check it again (DEVELOPMENT.md, Making a \
             release).",
            built.display()
        ));
    };
    let (taken_off, _) = without_signature(&bytes)?;
    if taken_off != unsigned {
        return Err(format!(
            "\nWithout its signature, this APK is not the app built in the fixed build environment ({}). Sign that \
             one, with tools/sign-apk.sh, and do not build the app otherwise for a release.",
            built.display()
        ));
    }
    println!("  Without its signature, it is byte for byte the app built in the fixed build environment ({})", built.display());
    let target = PathBuf::from("dist").join(format!("seedlock-{version}-android.apk"));
    match fs::read(&target) {
        Ok(existing) if existing == bytes => println!("\nIt is already in {} (the same file).", target.display()),
        Ok(_) => {
            return Err(format!(
                "\n{} already exists and is another file. Nothing is replaced: move it away first if this one should \
                 take its place.",
                target.display()
            ))
        }
        Err(_) => {
            fs::create_dir_all("dist").map_err(|e| format!("cannot create the folder dist: {e}"))?;
            fs::write(&target, &bytes).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
            println!("\nReady for the release: {}", target.display());
        }
    }
    println!("SHA-256: {}  {}", sha256::Hash::hash(&bytes), target.file_name().and_then(|n| n.to_str()).unwrap_or(""));
    Ok(())
}

/// Two certificate fingerprints are the same, whether written with colons or without, in capitals or not.
pub fn same_fingerprint(a: &str, b: &str) -> bool {
    let plain = |s: &str| s.chars().filter(char::is_ascii_hexdigit).map(|c| c.to_ascii_lowercase()).collect::<String>();
    plain(a).len() == 64 && plain(a) == plain(b)
}

/// The paths that must not be inside the app: this computer's home folder (it holds the user name), Cargo's
/// folder (the way Unix and Windows write them), and the fixed build environment's folders.
fn forbidden_folders() -> Vec<String> {
    let mut forms = home_folders();
    forms.extend(ENVIRONMENT_FOLDERS.iter().map(|f| f.to_string()));
    forms
}

/// This computer's home folder and Cargo's folder, the way Unix and Windows write them.
fn home_folders() -> Vec<String> {
    let mut forms = Vec::new();
    for variable in ["HOME", "USERPROFILE", "CARGO_HOME"] {
        let Ok(folder) = std::env::var(variable) else { continue };
        let folder = folder.trim_end_matches(['/', '\\']);
        if folder.len() < 4 {
            continue;
        }
        forms.push(format!("{folder}/"));
        forms.push(format!("{folder}\\"));
        forms.push(format!("{}/", folder.replace('\\', "/")));
    }
    forms.sort();
    forms.dedup();
    forms
}

/// What was found in an APK.
#[derive(Debug, Default)]
pub struct Apk {
    /// What is right, one line each.
    pub checked: Vec<String>,
    /// What is wrong, one line each.
    pub problems: Vec<String>,
    /// The signing certificate's SHA-256, as "AB:CD:...".
    pub certificate: String,
    /// "v2" or "v3".
    pub scheme: &'static str,
    /// The names in the certificate ("CN=SeedLock releases", ...).
    pub names: Vec<String>,
}

/// Checks an APK's bytes for SeedLock `version`; `home` lists paths that must not be inside it. An APK that
/// cannot even be read is an error; what is wrong with one that can is listed in [`Apk::problems`].
pub fn check(bytes: &[u8], version: &str, home: &[String]) -> Result<Apk, String> {
    let zip = Zip::read(bytes)?;
    let mut apk = Apk::default();
    check_contents(&zip, version, home, &mut apk)?;

    // its signature
    match signing_certificate(bytes, zip.central_directory) {
        Ok((certificate, scheme, signers)) => {
            apk.certificate = fingerprint(&certificate);
            apk.scheme = scheme;
            apk.names = certificate_names(&certificate);
            if signers != 1 {
                apk.problems.push(format!("It is signed by {signers} keys, not by one."));
            }
            if apk.names.iter().any(|n| n.contains("Android Debug")) {
                apk.problems.push("It is signed with Android Studio's test key (Android Debug), not with SeedLock's own key.".into());
            }
        }
        Err(e) => apk.problems.push(e),
    }
    Ok(apk)
}

/// Everything [`check`] checks but the signature: what the app is, and what is in it.
fn check_contents(zip: &Zip, version: &str, home: &[String], apk: &mut Apk) -> Result<(), String> {
    let problem = |apk: &mut Apk, text: String| apk.problems.push(text);

    // the app, its version, a release build
    let manifest = Manifest::read(&zip.data("AndroidManifest.xml")?)?;
    let code = version_code(version).ok_or_else(|| format!("{version} is not a version build.gradle.kts can number"))?;
    if manifest.package == APP_ID && manifest.version_name == version && manifest.version_code == Some(code) {
        apk.checked.push(format!("The app {APP_ID}, version {version} (code {code})"));
    } else if manifest.package == format!("{APP_ID}.debug") {
        problem(apk, "This is a test build (org.seedlock.app.debug, \"SeedLock test\"), not the release build.".into());
    } else {
        problem(
            apk,
            format!(
                "It is {} version {} (code {}), not {APP_ID} version {version} (code {code}): build it from this version's files.",
                manifest.package,
                manifest.version_name,
                manifest.version_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            ),
        );
    }
    match (manifest.debuggable, manifest.test_only) {
        (Some(false) | None, Some(false) | None) => apk.checked.push("A release build: it cannot be debugged".into()),
        (Some(true), _) => problem(apk, "It can be debugged (android:debuggable): build the release variant, not debug.".into()),
        _ => problem(apk, "It is a build for tests only (android:testOnly).".into()),
    }
    let permissions: BTreeSet<&str> = manifest.permissions.iter().map(String::as_str).collect();
    let allowed = BTreeSet::from(["android.permission.ACCESS_NETWORK_STATE", "android.permission.CAMERA"]);
    if permissions == allowed {
        apk.checked.push("Permissions: the camera and the network state only (no INTERNET)".into());
    } else {
        problem(apk, format!("It asks for other permissions than the camera and the network state: {permissions:?}"));
    }

    // its native libraries
    let mut found = BTreeSet::new();
    for name in zip.names() {
        if let Some(rest) = name.strip_prefix("lib/") {
            found.insert(rest.to_string());
        }
    }
    let wanted: BTreeSet<String> = ABIS.iter().flat_map(|abi| LIBRARIES.iter().map(move |lib| format!("{abi}/{lib}"))).collect();
    if found == wanted {
        apk.checked.push(format!("Native libraries: SeedLock's and JNA's, for {}", ABIS.join(", ")));
    } else {
        let missing: Vec<&String> = wanted.difference(&found).collect();
        let extra: Vec<&String> = found.difference(&wanted).collect();
        problem(apk, format!("Native libraries: missing {missing:?}, not expected {extra:?} (did android/build-rust.sh run?)"));
    }

    // its code
    let mut unexpected = BTreeSet::new();
    let mut libraries = Vec::new();
    let mut classes = 0;
    for name in zip.names().filter(|n| n.starts_with("classes") && n.ends_with(".dex") && !n.contains('/')) {
        for class in dex_classes(&zip.data(name)?)? {
            classes += 1;
            match CODE.iter().find(|(prefix, _)| class.starts_with(prefix)) {
                Some((_, library)) => {
                    if !libraries.contains(library) {
                        libraries.push(*library);
                    }
                }
                None => {
                    unexpected.insert(class.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or(class));
                }
            }
        }
    }
    if classes == 0 {
        problem(apk, "It has no code (classes.dex).".into());
    } else if unexpected.is_empty() {
        // in the order the notices name them
        libraries.sort_by_key(|library| CODE.iter().position(|(_, l)| l == library));
        apk.checked.push(format!("Code ({classes} classes): {} only", libraries.join(", ")));
    } else {
        let first: Vec<&String> = unexpected.iter().take(10).collect();
        problem(apk, format!("Code from a library the license notices do not name, in {first:?}"));
    }

    // its license notices
    match zip.data("assets/notices.txt") {
        Ok(text) if text.starts_with(notices::title(version).as_bytes()) => {
            apk.checked.push(format!("The license notices of SeedLock {version}"))
        }
        Ok(_) => problem(apk, "Its license notices are not this version's: run android/build-rust.sh, then build again.".into()),
        Err(_) => problem(apk, "It has no license notices (assets/notices.txt): run android/build-rust.sh, then build again.".into()),
    }

    // no folder of the computer that built it inside (the home folder holds the user name)
    let mut holds = BTreeSet::new();
    for name in zip.names() {
        let data = zip.data(name)?;
        for form in home {
            if contains(&data, form.as_bytes()) {
                holds.insert(name.to_string());
            }
        }
    }
    if holds.is_empty() {
        apk.checked.push("No folder of this computer (with your user name) or of the build environment is in it".into());
    } else {
        problem(apk, format!("It contains the path of a folder of the computer that built it (which may hold your user name), in {holds:?}"));
    }

    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// A certificate's SHA-256 fingerprint as keytool writes it: "AB:CD:...".
pub fn fingerprint(der: &[u8]) -> String {
    let hash = sha256::Hash::hash(der);
    hash.as_byte_array().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// The names in an X.509 certificate (common name, organisation, ...), as "CN=...": each once.
pub fn certificate_names(der: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut i = 0;
    while i + 7 <= der.len() {
        // OBJECT IDENTIFIER 2.5.4.x (an attribute of a name), then its value as a string
        if der[i..i + 4] == [0x06, 0x03, 0x55, 0x04] {
            let label = match der[i + 4] {
                0x03 => "CN",
                0x06 => "C",
                0x07 => "L",
                0x08 => "ST",
                0x0a => "O",
                0x0b => "OU",
                _ => "",
            };
            let (tag, len) = (der[i + 5], der[i + 6] as usize);
            if !label.is_empty() && matches!(tag, 0x0c | 0x13 | 0x14 | 0x16) && len < 0x80 && i + 7 + len <= der.len() {
                let value = String::from_utf8_lossy(&der[i + 7..i + 7 + len]);
                let name = format!("{label}={value}");
                if !names.contains(&name) {
                    names.push(name);
                }
                i += 7 + len;
                continue;
            }
        }
        i += 1;
    }
    names
}

// ---------------------------------------------------------------------------------------------------
// ZIP
// ---------------------------------------------------------------------------------------------------

struct Entry {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    offset: usize,
}

/// An APK as a ZIP archive: its entries, where its central directory starts (the APK Signing Block ends
/// right before it), and where its end record is.
struct Zip<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    central_directory: usize,
    end_record: usize,
}

/// A signed APK without its signature: its bytes as they were before it was signed, and whether it was
/// signed at all (an APK that is not is given back as it is). See the notes at the top of this file.
pub fn without_signature(bytes: &[u8]) -> Result<(Vec<u8>, bool), String> {
    let zip = Zip::read(bytes)?;
    let files_end = zip.files_end()?;
    let directory = zip.central_directory;
    let block_start = if directory >= files_end + 32 && bytes[directory - 16..directory] == *b"APK Sig Block 42" {
        let size = u64_at(bytes, directory - 24)? as usize;
        let start = directory.checked_sub(size + 8).filter(|&s| s >= files_end).ok_or("the APK's signature block is damaged")?;
        if u64_at(bytes, start)? as usize != size {
            return Err("the APK's signature block is damaged".into());
        }
        start
    } else {
        directory
    };
    let padding = &bytes[files_end..block_start];
    if block_start == directory && !padding.is_empty() {
        return Err(format!("There are {} unknown bytes between the APK's files and its central directory.", padding.len()));
    }
    if padding.len() >= 4096 || padding.iter().any(|&b| b != 0) {
        return Err("Something other than zeros comes before the APK's signature block.".into());
    }
    let removed = directory - files_end;
    let mut unsigned = Vec::with_capacity(bytes.len() - removed);
    unsigned.extend_from_slice(&bytes[..files_end]);
    unsigned.extend_from_slice(&bytes[directory..]);
    let offset = zip.end_record - removed + 16;
    unsigned[offset..offset + 4].copy_from_slice(&(files_end as u32).to_le_bytes());
    Ok((unsigned, block_start != directory))
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| "the file ends too early".into())
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "the file ends too early".into())
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    let s = b.get(at..at + 8).ok_or("the file ends too early")?;
    Ok(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

impl<'a> Zip<'a> {
    fn read(bytes: &'a [u8]) -> Result<Zip<'a>, String> {
        let not_apk = || "This is not an APK (not a ZIP archive).".to_string();
        let lowest = bytes.len().saturating_sub(22 + 65_535);
        let end = (lowest..=bytes.len().saturating_sub(22))
            .rev()
            .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
            .ok_or_else(not_apk)?;
        let count = u16_at(bytes, end + 10)? as usize;
        let start = u32_at(bytes, end + 16)? as usize;
        let mut entries = Vec::with_capacity(count);
        let mut at = start;
        for _ in 0..count {
            if u32_at(bytes, at)? != 0x0201_4b50 {
                return Err(not_apk());
            }
            let name_len = u16_at(bytes, at + 28)? as usize;
            let skip = name_len + u16_at(bytes, at + 30)? as usize + u16_at(bytes, at + 32)? as usize;
            let name = bytes.get(at + 46..at + 46 + name_len).ok_or_else(not_apk)?;
            let (compressed, size, offset) = (u32_at(bytes, at + 20)?, u32_at(bytes, at + 24)?, u32_at(bytes, at + 42)?);
            if [compressed, size, offset].contains(&u32::MAX) {
                return Err("This APK uses ZIP64, which no APK needs.".into());
            }
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                method: u16_at(bytes, at + 10)?,
                compressed: compressed as usize,
                size: size as usize,
                offset: offset as usize,
            });
            at += 46 + skip;
        }
        if at != end {
            return Err("The APK's central directory does not end where its end record starts.".into());
        }
        Ok(Zip { bytes, entries, central_directory: start, end_record: end })
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// Where the last file's record ends (its header, name, extra field, data, and the data descriptor that
    /// may follow it): what comes after it, up to the central directory, is the APK Signing Block.
    fn files_end(&self) -> Result<usize, String> {
        let mut end = 0;
        for entry in &self.entries {
            let at = entry.offset;
            if u32_at(self.bytes, at)? != 0x0403_4b50 {
                return Err(format!("{}: damaged ZIP entry", entry.name));
            }
            let flags = u16_at(self.bytes, at + 6)?;
            let mut stop = at + 30 + u16_at(self.bytes, at + 26)? as usize + u16_at(self.bytes, at + 28)? as usize + entry.compressed;
            if flags & 8 != 0 {
                // a data descriptor: its signature (which may be left out), the CRC and the two sizes
                stop += if u32_at(self.bytes, stop)? == 0x0807_4b50 { 16 } else { 12 };
            }
            end = end.max(stop);
        }
        if end > self.central_directory {
            return Err("The APK's files overlap its central directory.".into());
        }
        Ok(end)
    }

    /// An entry's contents.
    fn data(&self, name: &str) -> Result<Vec<u8>, String> {
        let entry = self.entries.iter().find(|e| e.name == name).ok_or_else(|| format!("{name} is not in the APK"))?;
        if u32_at(self.bytes, entry.offset)? != 0x0403_4b50 {
            return Err(format!("{name}: damaged ZIP entry"));
        }
        let start = entry.offset + 30 + u16_at(self.bytes, entry.offset + 26)? as usize + u16_at(self.bytes, entry.offset + 28)? as usize;
        let raw = self.bytes.get(start..start + entry.compressed).ok_or_else(|| format!("{name}: the APK ends too early"))?;
        match entry.method {
            0 => Ok(raw.to_vec()),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, entry.size.max(1))
                .map_err(|e| format!("{name} cannot be unpacked: {e:?}")),
            other => Err(format!("{name}: unknown ZIP compression {other}")),
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// The APK Signing Block (APK Signature Scheme v2 and v3)
// ---------------------------------------------------------------------------------------------------

const V2: u32 = 0x7109_871a;
const V3: u32 = 0xf053_68c0;

/// A length-prefixed part (a little-endian u32 length, then that many bytes), taken off the front of `data`.
fn prefixed<'a>(data: &mut &'a [u8]) -> Result<&'a [u8], String> {
    let damaged = || "the APK's signature block is damaged".to_string();
    let len = u32_at(data, 0).map_err(|_| damaged())? as usize;
    let part = data.get(4..4 + len).ok_or_else(damaged)?;
    *data = &data[4 + len..];
    Ok(part)
}

/// The first signer's certificate (DER), the signature scheme ("v3" if the APK has it, else "v2"), and how
/// many signers there are.
fn signing_certificate(bytes: &[u8], central_directory: usize) -> Result<(Vec<u8>, &'static str, usize), String> {
    let unsigned = || "It is not signed with APK Signature Scheme v2 or v3 (sign it in Android Studio: Build > Generate Signed App Bundle or APK).".to_string();
    if central_directory < 32 || bytes.get(central_directory - 16..central_directory) != Some(b"APK Sig Block 42".as_slice()) {
        return Err(unsigned());
    }
    let size = u64_at(bytes, central_directory - 24)? as usize;
    let start = central_directory.checked_sub(size + 8).ok_or_else(unsigned)?;
    if u64_at(bytes, start)? as usize != size {
        return Err("the APK's signature block is damaged".into());
    }
    let mut pairs = &bytes[start + 8..central_directory - 24];
    let mut schemes = Vec::new();
    while pairs.len() >= 12 {
        let len = u64_at(pairs, 0)? as usize;
        let id = u32_at(pairs, 8)?;
        let value = pairs.get(12..8 + len).ok_or("the APK's signature block is damaged")?;
        schemes.push((id, value));
        pairs = &pairs[8 + len..];
    }
    let (scheme, value) = schemes
        .iter()
        .find(|(id, _)| *id == V3)
        .map(|(_, v)| ("v3", *v))
        .or_else(|| schemes.iter().find(|(id, _)| *id == V2).map(|(_, v)| ("v2", *v)))
        .ok_or_else(unsigned)?;
    let mut value = value;
    let mut signers = prefixed(&mut value)?;
    let mut first = prefixed(&mut signers)?;
    let mut count = 1;
    while !signers.is_empty() {
        prefixed(&mut signers)?;
        count += 1;
    }
    let mut signed_data = prefixed(&mut first)?;
    let _digests = prefixed(&mut signed_data)?;
    let mut certificates = prefixed(&mut signed_data)?;
    let certificate = prefixed(&mut certificates)?;
    Ok((certificate.to_vec(), scheme, count))
}

// ---------------------------------------------------------------------------------------------------
// The binary AndroidManifest.xml
// ---------------------------------------------------------------------------------------------------

/// What the check needs from the app's manifest.
#[derive(Debug, Default, PartialEq)]
pub struct Manifest {
    pub package: String,
    pub version_code: Option<u32>,
    pub version_name: String,
    pub debuggable: Option<bool>,
    pub test_only: Option<bool>,
    pub permissions: Vec<String>,
}

const ATTR_NAME: u32 = 0x0101_0003;
const ATTR_DEBUGGABLE: u32 = 0x0101_000f;
const ATTR_VERSION_CODE: u32 = 0x0101_021b;
const ATTR_VERSION_NAME: u32 = 0x0101_021c;
const ATTR_TEST_ONLY: u32 = 0x0101_0272;

/// A string of an Android binary XML string pool (UTF-8 or UTF-16).
fn pool_string(chunk: &[u8], strings_start: usize, offset: usize, utf8: bool) -> Result<String, String> {
    let at = strings_start + offset;
    if utf8 {
        // its length in UTF-16 units, then in bytes (each one or two bytes long), then the bytes
        let skip = |at: usize| -> Result<(usize, usize), String> {
            let first = *chunk.get(at).ok_or("damaged string pool")? as usize;
            if first & 0x80 != 0 {
                Ok((((first & 0x7f) << 8) | *chunk.get(at + 1).ok_or("damaged string pool")? as usize, at + 2))
            } else {
                Ok((first, at + 1))
            }
        };
        let (_, at) = skip(at)?;
        let (len, at) = skip(at)?;
        let bytes = chunk.get(at..at + len).ok_or("damaged string pool")?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    } else {
        let first = u16_at(chunk, at)? as usize;
        let (len, at) = if first & 0x8000 != 0 { (((first & 0x7fff) << 16) | u16_at(chunk, at + 2)? as usize, at + 4) } else { (first, at + 2) };
        let units: Vec<u16> = (0..len).map(|i| u16_at(chunk, at + 2 * i)).collect::<Result<_, _>>()?;
        Ok(String::from_utf16_lossy(&units))
    }
}

impl Manifest {
    /// Reads the binary XML that Android's build tools make of the manifest.
    pub fn read(xml: &[u8]) -> Result<Manifest, String> {
        let damaged = || "AndroidManifest.xml is damaged".to_string();
        if u16_at(xml, 0)? != 0x0003 {
            return Err(damaged());
        }
        let mut strings: Vec<String> = Vec::new();
        let mut ids: Vec<u32> = Vec::new();
        let mut manifest = Manifest::default();
        let mut at = u16_at(xml, 2)? as usize;
        while at + 8 <= xml.len() {
            let kind = u16_at(xml, at)?;
            let header = u16_at(xml, at + 2)? as usize;
            let size = u32_at(xml, at + 4)? as usize;
            let chunk = xml.get(at..at + size).ok_or_else(damaged)?;
            match kind {
                0x0001 => {
                    let count = u32_at(chunk, 8)? as usize;
                    let utf8 = u32_at(chunk, 16)? & 0x100 != 0;
                    let strings_start = u32_at(chunk, 20)? as usize;
                    strings = (0..count)
                        .map(|i| pool_string(chunk, strings_start, u32_at(chunk, header + 4 * i)? as usize, utf8))
                        .collect::<Result<_, _>>()?;
                }
                0x0180 => ids = (0..(size - header) / 4).map(|i| u32_at(chunk, header + 4 * i)).collect::<Result<_, _>>()?,
                0x0102 => {
                    let string = |index: u32| strings.get(index as usize).cloned().unwrap_or_default();
                    let element = string(u32_at(chunk, 20)?);
                    let (attr_start, attr_size, attr_count) = (u16_at(chunk, 24)? as usize, u16_at(chunk, 26)? as usize, u16_at(chunk, 28)? as usize);
                    for i in 0..attr_count {
                        let a = 16 + attr_start + i * attr_size;
                        let name_index = u32_at(chunk, a + 4)?;
                        let id = ids.get(name_index as usize).copied().unwrap_or(0);
                        let name = string(name_index);
                        let raw = u32_at(chunk, a + 8)?;
                        let data_type = *chunk.get(a + 15).ok_or_else(damaged)?;
                        let data = u32_at(chunk, a + 16)?;
                        let text = if raw != u32::MAX { Some(string(raw)) } else if data_type == 0x03 { Some(string(data)) } else { None };
                        let int = matches!(data_type, 0x10 | 0x11).then_some(data);
                        let boolean = (data_type == 0x12).then_some(data != 0);
                        let is = |attr: u32, plain: &str| id == attr || (id == 0 && name == plain);
                        match element.as_str() {
                            "manifest" if name == "package" && id == 0 => manifest.package = text.unwrap_or_default(),
                            "manifest" if is(ATTR_VERSION_CODE, "versionCode") => manifest.version_code = int,
                            "manifest" if is(ATTR_VERSION_NAME, "versionName") => manifest.version_name = text.unwrap_or_default(),
                            "application" if is(ATTR_DEBUGGABLE, "debuggable") => manifest.debuggable = Some(boolean.ok_or("android:debuggable is not a plain true or false")?),
                            "application" if is(ATTR_TEST_ONLY, "testOnly") => manifest.test_only = Some(boolean.ok_or("android:testOnly is not a plain true or false")?),
                            "uses-permission" | "uses-permission-sdk-23" if is(ATTR_NAME, "name") => manifest.permissions.extend(text),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            if size < 8 {
                return Err(damaged());
            }
            at += size;
        }
        Ok(manifest)
    }
}

// ---------------------------------------------------------------------------------------------------
// The code (classes.dex)
// ---------------------------------------------------------------------------------------------------

/// The classes a DEX file defines, as "com/sun/jna/Native".
pub fn dex_classes(dex: &[u8]) -> Result<Vec<String>, String> {
    if !dex.starts_with(b"dex\n") {
        return Err("classes.dex is not a DEX file".into());
    }
    let (string_ids, type_ids) = (u32_at(dex, 0x3c)? as usize, u32_at(dex, 0x44)? as usize);
    let (count, defs) = (u32_at(dex, 0x60)? as usize, u32_at(dex, 0x64)? as usize);
    let mut classes = Vec::with_capacity(count);
    for i in 0..count {
        let type_index = u32_at(dex, defs + 32 * i)? as usize;
        let string_index = u32_at(dex, type_ids + 4 * type_index)? as usize;
        let mut at = u32_at(dex, string_ids + 4 * string_index)? as usize;
        // the string's length in UTF-16 units (ULEB128), then its bytes (MUTF-8) up to a zero
        while dex.get(at).ok_or("classes.dex is damaged")? & 0x80 != 0 {
            at += 1;
        }
        at += 1;
        let end = dex[at..].iter().position(|&b| b == 0).ok_or("classes.dex is damaged")?;
        let descriptor = String::from_utf8_lossy(&dex[at..at + end]);
        classes.push(descriptor.trim_start_matches('L').trim_end_matches(';').to_string());
    }
    Ok(classes)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    // ---- a small APK writer, for the tests ----

    /// An Android binary XML document: elements with attributes (name, resource id, value).
    pub enum Value {
        Text(&'static str),
        Int(u32),
        Bool(bool),
    }

    /// An element: its name, and its attributes (name, resource id or 0, value).
    pub type Element<'a> = (&'a str, Vec<(&'a str, u32, Value)>);

    pub fn binary_xml(elements: &[Element], utf8: bool) -> Vec<u8> {
        // the string pool: attribute names first (they match the resource map), then the rest
        let mut strings: Vec<String> = Vec::new();
        let mut ids: Vec<u32> = Vec::new();
        for (_, attrs) in elements {
            for (name, id, _) in attrs {
                if *id != 0 && !strings.contains(&name.to_string()) {
                    strings.push(name.to_string());
                    ids.push(*id);
                }
            }
        }
        let index = |s: &str, strings: &mut Vec<String>| -> u32 {
            match strings.iter().position(|x| x == s) {
                Some(i) => i as u32,
                None => {
                    strings.push(s.to_string());
                    strings.len() as u32 - 1
                }
            }
        };
        let mut body = Vec::new();
        for (element, attrs) in elements {
            let name = index(element, &mut strings);
            let mut chunk = Vec::new();
            chunk.extend(0x0102u16.to_le_bytes());
            chunk.extend(16u16.to_le_bytes());
            chunk.extend(((36 + 20 * attrs.len()) as u32).to_le_bytes());
            chunk.extend(1u32.to_le_bytes()); // line
            chunk.extend(u32::MAX.to_le_bytes()); // comment
            chunk.extend(u32::MAX.to_le_bytes()); // namespace
            chunk.extend(name.to_le_bytes());
            chunk.extend(20u16.to_le_bytes());
            chunk.extend(20u16.to_le_bytes());
            chunk.extend((attrs.len() as u16).to_le_bytes());
            chunk.extend([0u8; 6]);
            for (attr, id, value) in attrs {
                let attr_index = index(attr, &mut strings);
                let (raw, data_type, data) = match value {
                    Value::Text(s) => {
                        let i = index(s, &mut strings);
                        (i, 0x03u8, i)
                    }
                    Value::Int(n) => (u32::MAX, 0x10, *n),
                    Value::Bool(b) => (u32::MAX, 0x12, if *b { u32::MAX } else { 0 }),
                };
                chunk.extend((if *id != 0 { 0u32 } else { u32::MAX }).to_le_bytes());
                chunk.extend(attr_index.to_le_bytes());
                chunk.extend(raw.to_le_bytes());
                chunk.extend(8u16.to_le_bytes());
                chunk.push(0);
                chunk.push(data_type);
                chunk.extend(data.to_le_bytes());
            }
            body.extend(chunk);
            // its end
            body.extend(0x0103u16.to_le_bytes());
            body.extend(16u16.to_le_bytes());
            body.extend(24u32.to_le_bytes());
            body.extend([0u8; 16]);
        }
        // the string pool
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for s in &strings {
            offsets.push(data.len() as u32);
            if utf8 {
                data.push(s.encode_utf16().count() as u8);
                data.push(s.len() as u8);
                data.extend(s.as_bytes());
                data.push(0);
            } else {
                let units: Vec<u16> = s.encode_utf16().collect();
                data.extend((units.len() as u16).to_le_bytes());
                units.iter().for_each(|u| data.extend(u.to_le_bytes()));
                data.extend([0, 0]);
            }
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let header = 28;
        let strings_start = header + 4 * strings.len();
        let mut pool = Vec::new();
        pool.extend(0x0001u16.to_le_bytes());
        pool.extend((header as u16).to_le_bytes());
        pool.extend(((strings_start + data.len()) as u32).to_le_bytes());
        pool.extend((strings.len() as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        pool.extend((if utf8 { 0x100u32 } else { 0 }).to_le_bytes());
        pool.extend((strings_start as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        offsets.iter().for_each(|o| pool.extend(o.to_le_bytes()));
        pool.extend(data);
        let mut map = Vec::new();
        map.extend(0x0180u16.to_le_bytes());
        map.extend(8u16.to_le_bytes());
        map.extend(((8 + 4 * ids.len()) as u32).to_le_bytes());
        ids.iter().for_each(|i| map.extend(i.to_le_bytes()));
        let mut xml = Vec::new();
        xml.extend(0x0003u16.to_le_bytes());
        xml.extend(8u16.to_le_bytes());
        xml.extend(((8 + pool.len() + map.len() + body.len()) as u32).to_le_bytes());
        xml.extend(pool);
        xml.extend(map);
        xml.extend(body);
        xml
    }

    /// A DEX file that defines these classes (and nothing else that a reader of class names needs).
    pub fn dex(classes: &[&str]) -> Vec<u8> {
        let n = classes.len();
        let (string_ids, type_ids) = (0x70, 0x70 + 4 * n);
        let defs = type_ids + 4 * n;
        let mut data_at = defs + 32 * n;
        let mut out = vec![0u8; data_at];
        out[..8].copy_from_slice(b"dex\n035\0");
        out[0x38..0x3c].copy_from_slice(&(n as u32).to_le_bytes());
        out[0x3c..0x40].copy_from_slice(&(string_ids as u32).to_le_bytes());
        out[0x40..0x44].copy_from_slice(&(n as u32).to_le_bytes());
        out[0x44..0x48].copy_from_slice(&(type_ids as u32).to_le_bytes());
        out[0x60..0x64].copy_from_slice(&(n as u32).to_le_bytes());
        out[0x64..0x68].copy_from_slice(&(defs as u32).to_le_bytes());
        for (i, class) in classes.iter().enumerate() {
            let descriptor = format!("L{class};");
            out[string_ids + 4 * i..string_ids + 4 * i + 4].copy_from_slice(&(data_at as u32).to_le_bytes());
            out[type_ids + 4 * i..type_ids + 4 * i + 4].copy_from_slice(&(i as u32).to_le_bytes());
            out[defs + 32 * i..defs + 32 * i + 4].copy_from_slice(&(i as u32).to_le_bytes());
            out.push(descriptor.len() as u8);
            out.extend(descriptor.as_bytes());
            out.push(0);
            data_at = out.len();
        }
        out
    }

    /// A ZIP archive of these entries (compressed with deflate, except .so files), with an APK Signing Block
    /// holding a signer with `certificate` (scheme v2 or v3), if one is given.
    pub fn apk(entries: &[(&str, Vec<u8>)], signature: Option<(u32, &[u8])>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let stored = name.ends_with(".so");
            let packed = if stored { data.clone() } else { miniz_oxide::deflate::compress_to_vec(data, 6) };
            let method: u16 = if stored { 0 } else { 8 };
            let offset = out.len() as u32;
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend([20, 0, 0, 0]);
            out.extend(method.to_le_bytes());
            out.extend([0u8; 8]); // time, date, crc (not checked)
            out.extend((packed.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(&packed);
            central.extend(0x0201_4b50u32.to_le_bytes());
            central.extend([20, 0, 20, 0, 0, 0]);
            central.extend(method.to_le_bytes());
            central.extend([0u8; 8]);
            central.extend((packed.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0u8; 12]);
            central.extend(offset.to_le_bytes());
            central.extend(name.as_bytes());
        }
        if let Some((scheme, certificate)) = signature {
            let lp = |b: &[u8]| -> Vec<u8> { [(b.len() as u32).to_le_bytes().to_vec(), b.to_vec()].concat() };
            let signed_data = [lp(&[]), lp(&lp(certificate)), lp(&[])].concat();
            let signer = [lp(&signed_data), lp(&[]), lp(&[])].concat();
            let value = lp(&lp(&signer));
            let pair = [((value.len() + 4) as u64).to_le_bytes().to_vec(), scheme.to_le_bytes().to_vec(), value].concat();
            let size = (pair.len() + 8 + 16) as u64;
            out.extend(size.to_le_bytes());
            out.extend(pair);
            out.extend(size.to_le_bytes());
            out.extend(b"APK Sig Block 42");
        }
        let start = out.len() as u32;
        let count = entries.len() as u16;
        out.extend(&central);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(count.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    /// A stand-in X.509 certificate: only what the check reads (the names), DER-encoded.
    pub fn certificate(common_name: &str) -> Vec<u8> {
        let mut der = vec![0x30, 0x00, 0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, common_name.len() as u8];
        der.extend(common_name.as_bytes());
        der
    }

    fn manifest(package: &'static str, version_code: u32, version_name: &'static str, debuggable: Option<bool>, permissions: &[&'static str]) -> Vec<u8> {
        let mut elements = vec![(
            "manifest",
            vec![("versionCode", ATTR_VERSION_CODE, Value::Int(version_code)), ("versionName", ATTR_VERSION_NAME, Value::Text(version_name)), ("package", 0, Value::Text(package))],
        )];
        for p in permissions {
            elements.push(("uses-permission", vec![("name", ATTR_NAME, Value::Text(p))]));
        }
        let mut app = vec![("label", 0x0101_0001, Value::Text("SeedLock"))];
        if let Some(d) = debuggable {
            app.push(("debuggable", ATTR_DEBUGGABLE, Value::Bool(d)));
        }
        elements.push(("application", app));
        binary_xml(&elements, false)
    }

    const PERMISSIONS: [&str; 2] = ["android.permission.CAMERA", "android.permission.ACCESS_NETWORK_STATE"];

    fn release_entries(version: &'static str, code: u32) -> Vec<(&'static str, Vec<u8>)> {
        let mut entries = vec![
            ("AndroidManifest.xml", manifest(APP_ID, code, version, None, &PERMISSIONS)),
            ("classes.dex", dex(&["org/seedlock/app/MainActivity", "org/seedlock/ffi/Wallet", "kotlin/Unit", "com/sun/jna/Native", "org/jetbrains/annotations/NotNull"])),
            ("assets/notices.txt", format!("{}\n\nthe notices", notices::title(version)).into_bytes()),
        ];
        for abi in ABIS {
            for lib in LIBRARIES {
                entries.push((Box::leak(format!("lib/{abi}/{lib}").into_boxed_str()), b"\x7fELF a library".to_vec()));
            }
        }
        entries
    }

    fn checked(entries: &[(&str, Vec<u8>)], certificate: &[u8]) -> Apk {
        check(&apk(entries, Some((V3, certificate))), "0.2.1", &["/home/someone/".to_string()]).unwrap()
    }

    #[test]
    fn a_right_release_apk_passes_and_its_certificate_is_read() {
        let cert = certificate("SeedLock releases");
        let found = checked(&release_entries("0.2.1", 201), &cert);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        assert_eq!(found.checked.len(), 7, "{:#?}", found.checked);
        assert_eq!(found.certificate, fingerprint(&cert));
        assert_eq!(found.scheme, "v3");
        assert_eq!(found.names, ["CN=SeedLock releases"]);
        assert!(found.certificate.len() == 95 && found.certificate.split(':').count() == 32);
        // the same with scheme v2 only, and a manifest whose strings are UTF-8
        let mut entries = release_entries("0.2.1", 201);
        entries[0].1 = {
            let elements = vec![
                ("manifest", vec![("versionCode", ATTR_VERSION_CODE, Value::Int(201)), ("versionName", ATTR_VERSION_NAME, Value::Text("0.2.1")), ("package", 0, Value::Text(APP_ID))]),
                ("uses-permission", vec![("name", ATTR_NAME, Value::Text(PERMISSIONS[0]))]),
                ("uses-permission", vec![("name", ATTR_NAME, Value::Text(PERMISSIONS[1]))]),
                ("application", vec![("debuggable", ATTR_DEBUGGABLE, Value::Bool(false))]),
            ];
            binary_xml(&elements, true)
        };
        let found = check(&apk(&entries, Some((V2, &cert))), "0.2.1", &[]).unwrap();
        assert!(found.problems.is_empty() && found.scheme == "v2", "{:?}", found.problems);
        assert!(same_fingerprint(&fingerprint(&cert), &fingerprint(&cert).replace(':', "").to_lowercase()));
        assert!(!same_fingerprint(&fingerprint(&cert), &fingerprint(&certificate("other"))));
        assert!(!same_fingerprint("", ""), "an empty fingerprint is no fingerprint");
    }

    fn problems(entries: &[(&str, Vec<u8>)]) -> String {
        checked(entries, &certificate("SeedLock releases")).problems.join("\n")
    }

    #[test]
    fn every_mistake_is_found() {
        let good = || release_entries("0.2.1", 201);
        // a debug build, a test build, another version
        let mut e = good();
        e[0].1 = manifest(APP_ID, 201, "0.2.1", Some(true), &PERMISSIONS);
        assert!(problems(&e).contains("can be debugged"), "{}", problems(&e));
        e[0].1 = manifest("org.seedlock.app.debug", 201, "0.2.1", Some(true), &PERMISSIONS);
        assert!(problems(&e).contains("test build"), "{}", problems(&e));
        let e = release_entries("0.2.0", 200);
        assert!(problems(&e).contains("not org.seedlock.app version 0.2.1 (code 201)") && problems(&e).contains("notices are not this version"), "{}", problems(&e));
        // the INTERNET permission
        let mut e = good();
        e[0].1 = manifest(APP_ID, 201, "0.2.1", None, &["android.permission.CAMERA", "android.permission.ACCESS_NETWORK_STATE", "android.permission.INTERNET"]);
        assert!(problems(&e).contains("android.permission.INTERNET"), "{}", problems(&e));
        // a library missing, another kind of processor
        let mut e = good();
        e.retain(|(n, _)| *n != "lib/x86_64/libseedlock_ffi.so");
        e.push(("lib/x86/libjnidispatch.so", b"x".to_vec()));
        let p = problems(&e);
        assert!(p.contains("missing [\"x86_64/libseedlock_ffi.so\"]") && p.contains("not expected [\"x86/libjnidispatch.so\"]"), "{p}");
        // code of another library, no notices
        let mut e = good();
        e[1].1 = dex(&["org/seedlock/app/MainActivity", "androidx/core/app/ActivityCompat"]);
        e.retain(|(n, _)| *n != "assets/notices.txt");
        let p = problems(&e);
        assert!(p.contains("androidx/core/app") && p.contains("no license notices"), "{p}");
        // the home folder's path inside a library
        let mut e = good();
        e[3].1 = b"\x7fELF ... /home/someone/.cargo/registry/src/x.rs".to_vec();
        assert!(problems(&e).contains("a folder of the computer that built it"), "{}", problems(&e));
        // Android Studio's test key; no v2/v3 signature at all
        let debug = checked(&good(), &certificate("Android Debug"));
        assert!(debug.problems.join(" ").contains("test key"), "{:?}", debug.problems);
        let unsigned = check(&apk(&good(), None), "0.2.1", &[]).unwrap();
        assert!(unsigned.problems.join(" ").contains("not signed with APK Signature Scheme v2 or v3"), "{:?}", unsigned.problems);
        // not an APK at all
        assert!(check(b"hello", "0.2.1", &[]).is_err());
    }

    /// `apk` with `n` bytes of `fill` put between its files and its APK Signing Block (or its central directory),
    /// and the end record changed to match: what apksigner does with zeros, to align the block.
    fn padded(apk: &[u8], n: usize, fill: u8) -> Vec<u8> {
        let zip = Zip::read(apk).unwrap();
        let at = zip.files_end().unwrap();
        let end_record = zip.end_record + n;
        let mut out = [&apk[..at], &vec![fill; n][..], &apk[at..]].concat();
        let directory = u32::from_le_bytes(out[end_record + 16..end_record + 20].try_into().unwrap()) as usize + n;
        out[end_record + 16..end_record + 20].copy_from_slice(&(directory as u32).to_le_bytes());
        out
    }

    #[test]
    fn taking_the_signature_off_gives_back_the_apk_as_it_was_before_it_was_signed() {
        let entries = release_entries("0.2.1", 201);
        let cert = certificate("SeedLock releases");
        let unsigned = apk(&entries, None);
        for scheme in [V2, V3] {
            let signed = apk(&entries, Some((scheme, &cert)));
            assert!(signed.len() > unsigned.len());
            assert_eq!(without_signature(&signed).unwrap(), (unsigned.clone(), true));
            // the zeros apksigner puts before the block, so that it starts at a multiple of 4096 bytes
            assert_eq!(without_signature(&padded(&signed, 72, 0)).unwrap(), (unsigned.clone(), true));
            assert_eq!(without_signature(&padded(&signed, 4095, 0)).unwrap(), (unsigned.clone(), true));
            // anything else there is not a signature's
            assert!(without_signature(&padded(&signed, 72, 1)).unwrap_err().contains("other than zeros"));
            assert!(without_signature(&padded(&signed, 4096, 0)).unwrap_err().contains("other than zeros"));
        }
        // an APK that is not signed comes back as it is; with something unknown before its directory, not at all
        assert_eq!(without_signature(&unsigned).unwrap(), (unsigned.clone(), false));
        assert!(without_signature(&padded(&unsigned, 8, 0)).unwrap_err().contains("8 unknown bytes"));
        // what comes off is read the same way the signature check reads it, and the result is an APK again
        let signed = apk(&entries, Some((V2, &cert)));
        let (taken_off, _) = without_signature(&signed).unwrap();
        assert!(check(&taken_off, "0.2.1", &[]).unwrap().problems.join(" ").contains("not signed"));
        assert!(check(&signed, "0.2.1", &[]).unwrap().problems.is_empty());
        // a damaged block size
        let mut damaged = signed.clone();
        let directory = Zip::read(&signed).unwrap().central_directory;
        damaged[directory - 24] ^= 1;
        assert!(without_signature(&damaged).unwrap_err().contains("damaged"));
    }

    #[test]
    fn data_descriptors_and_a_comment_are_kept_in_place() {
        // one stored file whose sizes follow its data (flag 8, a data descriptor with its signature), a ZIP comment
        let data = b"hello, data descriptor";
        let mut zip = Vec::new();
        zip.extend(0x0403_4b50u32.to_le_bytes());
        zip.extend([20, 0, 8, 0, 0, 0]); // version, flags (8: a data descriptor follows), method 0
        zip.extend([0u8; 16]); // time, date, crc and sizes (they are in the descriptor)
        zip.extend(1u16.to_le_bytes());
        zip.extend(0u16.to_le_bytes());
        zip.push(b'a');
        zip.extend(data);
        zip.extend(0x0807_4b50u32.to_le_bytes());
        zip.extend([0u8; 4]);
        zip.extend((data.len() as u32).to_le_bytes());
        zip.extend((data.len() as u32).to_le_bytes());
        let files_end = zip.len();
        let mut directory = Vec::new();
        directory.extend(0x0201_4b50u32.to_le_bytes());
        directory.extend([20, 0, 20, 0, 8, 0, 0, 0]);
        directory.extend([0u8; 8]);
        directory.extend((data.len() as u32).to_le_bytes());
        directory.extend((data.len() as u32).to_le_bytes());
        directory.extend(1u16.to_le_bytes());
        directory.extend([0u8; 12]);
        directory.extend(0u32.to_le_bytes());
        directory.push(b'a');
        let end = |directory_at: usize, directory: &[u8]| -> Vec<u8> {
            let mut e = 0x0605_4b50u32.to_le_bytes().to_vec();
            e.extend([0, 0, 0, 0, 1, 0, 1, 0]);
            e.extend((directory.len() as u32).to_le_bytes());
            e.extend((directory_at as u32).to_le_bytes());
            e.extend(5u16.to_le_bytes());
            e.extend(b"hello");
            e
        };
        let unsigned = [&zip[..], &directory[..], &end(files_end, &directory)[..]].concat();
        assert_eq!(Zip::read(&unsigned).unwrap().files_end().unwrap(), files_end);
        // the same with a signing block (a stand-in one) between the file and the directory
        let pair = [16u64.to_le_bytes().to_vec(), V2.to_le_bytes().to_vec(), vec![7u8; 12]].concat();
        let size = (pair.len() + 24) as u64;
        let block = [size.to_le_bytes().to_vec(), pair, size.to_le_bytes().to_vec(), b"APK Sig Block 42".to_vec()].concat();
        let signed = [&zip[..], &block[..], &directory[..], &end(files_end + block.len(), &directory)[..]].concat();
        assert_eq!(without_signature(&signed).unwrap(), (unsigned, true));
    }

    #[test]
    fn the_app_before_it_is_signed_is_checked_for_the_environments_folders_too() {
        let mut entries = release_entries("0.2.1", 201);
        let mut apk_found = Apk::default();
        check_contents(&Zip::read(&apk(&entries, None)).unwrap(), "0.2.1", &forbidden_folders(), &mut apk_found).unwrap();
        assert!(apk_found.problems.is_empty(), "{:?}", apk_found.problems);
        assert_eq!(apk_found.checked.len(), 7);
        // the app's own packages (org/seedlock/...) are no folder of the environment
        assert!(ENVIRONMENT_FOLDERS.iter().all(|f| !"Lorg/seedlock/app/MainActivity;Lorg/seedlock/ffi/X;".contains(f)));
        entries[4].1 = b"\x7fELF ... /seedlock/target/aarch64-linux-android/phone/build/x.rs".to_vec();
        let mut apk_found = Apk::default();
        check_contents(&Zip::read(&apk(&entries, None)).unwrap(), "0.2.1", &forbidden_folders(), &mut apk_found).unwrap();
        assert!(apk_found.problems.join(" ").contains("a folder of the computer that built it"), "{:?}", apk_found.problems);
        assert_eq!(unsigned_name("0.3.2"), "seedlock-0.3.2-android-unsigned.apk");
    }

    #[test]
    fn the_android_certificate_is_the_same_everywhere_it_is_given() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for doc in ["android/README.md", "docs/DEVELOPMENT.md"] {
            let text = fs::read_to_string(root.join(doc)).unwrap();
            match RELEASE_CERTIFICATE {
                Some(fingerprint) => {
                    assert!(fingerprint.len() == 95 && fingerprint.split(':').count() == 32, "{fingerprint}");
                    assert!(text.contains(fingerprint), "{doc} must give the Android certificate's fingerprint {fingerprint}");
                    assert!(!text.contains("(not made yet)"), "{doc} still says the Android key is not made yet");
                }
                None => assert!(text.contains("(not made yet)"), "{doc} must say that the Android key is not made yet"),
            }
        }
    }

    #[test]
    fn version_codes_follow_build_gradle() {
        assert_eq!(version_code("0.2.1"), Some(201));
        assert_eq!(version_code("1.10.3"), Some(11003));
        assert_eq!(version_code("0.2"), None);
        assert_eq!(version_code("0.100.0"), None);
    }
}
