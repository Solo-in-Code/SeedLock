//! The license notices of the libraries that SeedLock's phone app is built from, as the app shows them
//! (its first screen: About SeedLock and its licenses). `android/build-rust.sh` writes them into the app's
//! assets right after it built the Rust library, from the very libraries it used, so that they always
//! match the app.
//!
//! Which libraries: the Rust crates compiled into `libseedlock_ffi.so` for the three kinds of phone
//! processor (`cargo tree`), without the crates that only help to compile it, but with the macro crates
//! whose generated code is in it; Rust's standard library; and the app's Java part: JNA (with libffi in
//! its native library), Kotlin's standard library and JetBrains' annotations.
//!
//! What of each: the license it is used under (of the ones a crate offers, the one that asks the least;
//! MIT where several ask the same), its copyright lines, and the license texts: the MIT text once for all
//! (each library's own copyright lines are listed), the Apache 2.0 and MPL 2.0 texts once, and any other
//! license in full, as the library ships it. Texts are reflowed into paragraphs, for a phone's screen.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The phone processors the app is built for (android/build-rust.sh).
pub const ANDROID_TARGETS: [&str; 3] = ["aarch64-linux-android", "armv7-linux-androideabi", "x86_64-linux-android"];

/// The first line of the notices; `check-apk` finds the version in it.
pub fn title(version: &str) -> String {
    format!("SeedLock {version} for Android: licenses")
}

const APACHE_2_0: &str = include_str!("../licenses/Apache-2.0.txt");
const MPL_2_0: &str = include_str!("../licenses/MPL-2.0.txt");
/// JNA's own LICENSE file (version 5.18.1): which licenses it may be used under.
const JNA_LICENSE: &str = include_str!("../licenses/JNA-LICENSE.txt");
/// libffi's LICENSE, as JNA 5.18.1 ships it (native/libffi), which its libjnidispatch.so is built with.
const LIBFFI_LICENSE: &str = include_str!("../licenses/libffi-LICENSE.txt");

/// The MIT license's text, after its copyright lines.
pub const MIT_BODY: &str = "Permission is hereby granted, free of charge, to any person obtaining a copy of this \
software and associated documentation files (the \"Software\"), to deal in the Software without restriction, \
including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell \
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following \
conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of \
the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO \
THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE \
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, \
TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE \
SOFTWARE.";

/// The MITNFA license ("MIT +no-false-attribs", SPDX), after its copyright lines: for a crate under it
/// that ships no license file.
const MITNFA_BODY: &str = "Permission is hereby granted, free of charge, to any person obtaining a copy of this \
software and associated documentation files (the \"Software\"), to deal in the Software without restriction, \
including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell \
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following \
conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of \
the Software.

Distributions of all or part of the Software intended to be used by the recipients as they would use the \
unmodified Software, containing modifications that substantially alter, remove, or disable functionality of the \
Software, outside of the documented configuration mechanisms provided by the Software, shall be modified such that \
the Original Author's bug reporting email addresses and urls are either replaced with the contact information of \
the parties responsible for the changes, or removed entirely.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO \
THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE \
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, \
TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE \
SOFTWARE.";

/// `seedlock-tools notices --android [--out FILE]`
pub fn run(args: &[String]) -> Result<(), String> {
    let mut android = false;
    let mut out = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--android" => android = true,
            "--out" => out = Some(PathBuf::from(rest.next().ok_or("--out needs a file name")?)),
            other => return Err(format!("notices: unknown option {other}")),
        }
    }
    if !android {
        return Err("notices: say which app the notices are for: --android".into());
    }
    let text = android_notices(Path::new("."), env!("CARGO_PKG_VERSION"))?;
    match out {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            }
            fs::write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            eprintln!("The license notices ({} KB) are in {}", text.len() / 1024, path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// One library, as `cargo metadata` describes it.
#[derive(Debug, Clone)]
struct Package {
    name: String,
    version: String,
    license: Option<String>,
    authors: Vec<String>,
    repository: Option<String>,
    dir: PathBuf,
    proc_macro: bool,
    /// One of SeedLock's own crates (no registry source): covered by SeedLock's own license.
    own: bool,
    /// The ids of the packages it depends on as a normal (not build or dev) dependency.
    deps: Vec<String>,
}

impl Package {
    fn title(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn run_cargo(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(cargo())
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo ({e}); run this in SeedLock's folder, with Rust installed"))?;
    if !output.status.success() {
        return Err(format!("cargo {} failed:\n{}", args.join(" "), String::from_utf8_lossy(&output.stderr)));
    }
    String::from_utf8(output.stdout).map_err(|_| "cargo wrote something that is not text".into())
}

/// The crates (name, version) of `package`'s dependency tree for `target`, with the given kinds of edge.
fn tree(root: &Path, package: &str, target: &str, edges: &str) -> Result<BTreeSet<(String, String)>, String> {
    let out = run_cargo(root, &["tree", "--locked", "-p", package, "--edges", edges, "--target", target, "--prefix", "none", "--format", "{p}"])?;
    let mut crates = BTreeSet::new();
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let mut words = line.split_whitespace();
        let (Some(name), Some(version)) = (words.next(), words.next()) else { continue };
        let version = version.strip_prefix('v').ok_or_else(|| format!("unexpected line from cargo tree: {line}"))?;
        crates.insert((name.to_string(), version.to_string()));
    }
    if crates.is_empty() {
        return Err(format!("cargo tree found no crates for {package} ({target})"));
    }
    Ok(crates)
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Every package of the workspace's dependency graph for these targets, by id.
fn metadata(root: &Path, targets: &[&str]) -> Result<HashMap<String, Package>, String> {
    let mut args = vec!["metadata", "--format-version", "1", "--locked"];
    for target in targets {
        args.extend(["--filter-platform", target]);
    }
    let json: Value = serde_json::from_str(&run_cargo(root, &args)?).map_err(|e| format!("cargo metadata: {e}"))?;
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for node in json["resolve"]["nodes"].as_array().ok_or("cargo metadata: no resolve")? {
        let id = text_field(node, "id").unwrap_or_default();
        let mut normal = Vec::new();
        for dep in node["deps"].as_array().into_iter().flatten() {
            let kinds = dep["dep_kinds"].as_array().cloned().unwrap_or_default();
            if kinds.iter().any(|k| k["kind"].is_null()) {
                normal.extend(text_field(dep, "pkg"));
            }
        }
        deps.insert(id, normal);
    }
    let mut packages = HashMap::new();
    for p in json["packages"].as_array().ok_or("cargo metadata: no packages")? {
        let id = text_field(p, "id").unwrap_or_default();
        let manifest = PathBuf::from(text_field(p, "manifest_path").unwrap_or_default());
        let proc_macro = p["targets"].as_array().into_iter().flatten().any(|t| {
            t["kind"].as_array().into_iter().flatten().any(|k| k.as_str() == Some("proc-macro"))
        });
        let authors = p["authors"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)).collect();
        packages.insert(
            id.clone(),
            Package {
                name: text_field(p, "name").unwrap_or_default(),
                version: text_field(p, "version").unwrap_or_default(),
                license: text_field(p, "license"),
                authors,
                repository: text_field(p, "repository"),
                dir: manifest.parent().map(Path::to_path_buf).unwrap_or_default(),
                proc_macro,
                own: p["source"].is_null(),
                deps: deps.remove(&id).unwrap_or_default(),
            },
        );
    }
    Ok(packages)
}

/// The crates in the phone library: those compiled into it for each Android target, and the macro crates
/// they use directly (the code those generate is in the library; what the macro crates themselves use is
/// not). Sorted by name and version.
fn phone_library_crates(root: &Path) -> Result<Vec<Package>, String> {
    let mut linked = BTreeSet::new();
    let mut built = BTreeSet::new();
    for target in ANDROID_TARGETS {
        linked.extend(tree(root, "seedlock-ffi", target, "normal,no-proc-macro")?);
        built.extend(tree(root, "seedlock-ffi", target, "normal")?);
    }
    let packages = metadata(root, &ANDROID_TARGETS)?;
    let by_name: HashMap<(String, String), &Package> =
        packages.values().map(|p| ((p.name.clone(), p.version.clone()), p)).collect();
    let mut chosen: BTreeMap<(String, String), Package> = BTreeMap::new();
    for key in &linked {
        let package = by_name.get(key).ok_or_else(|| format!("cargo metadata does not know {} {}", key.0, key.1))?;
        chosen.insert(key.clone(), (*package).clone());
        for dep in &package.deps {
            let Some(dep) = packages.get(dep) else { continue };
            let dep_key = (dep.name.clone(), dep.version.clone());
            if dep.proc_macro && built.contains(&dep_key) {
                chosen.insert(dep_key, dep.clone());
            }
        }
    }
    Ok(chosen.into_values().filter(|p| !p.own).collect())
}

// ---------------------------------------------------------------------------------------------------
// License expressions and texts
// ---------------------------------------------------------------------------------------------------

/// A license expression's alternatives, each a list of licenses that apply together:
/// "(MIT OR Apache-2.0) AND ISC" gives [MIT, ISC] and [Apache-2.0, ISC]; the old "MIT/Apache-2.0" means OR.
pub fn alternatives(expression: &str) -> Result<Vec<Vec<String>>, String> {
    let spaced = expression.replace('(', " ( ").replace(')', " ) ").replace('/', " OR ");
    let tokens: Vec<&str> = spaced.split_whitespace().collect();
    let mut at = 0;
    let result = or_expression(&tokens, &mut at)?;
    if at != tokens.len() || result.is_empty() {
        return Err(format!("cannot read the license expression \"{expression}\""));
    }
    Ok(result)
}

fn or_expression(tokens: &[&str], at: &mut usize) -> Result<Vec<Vec<String>>, String> {
    let mut result = and_expression(tokens, at)?;
    while tokens.get(*at).is_some_and(|t| t.eq_ignore_ascii_case("OR")) {
        *at += 1;
        result.extend(and_expression(tokens, at)?);
    }
    Ok(result)
}

fn and_expression(tokens: &[&str], at: &mut usize) -> Result<Vec<Vec<String>>, String> {
    let mut result = atom(tokens, at)?;
    while tokens.get(*at).is_some_and(|t| t.eq_ignore_ascii_case("AND")) {
        *at += 1;
        let right = atom(tokens, at)?;
        result = result
            .iter()
            .flat_map(|left| right.iter().map(move |r| left.iter().chain(r).cloned().collect::<Vec<_>>()))
            .collect();
    }
    Ok(result)
}

fn atom(tokens: &[&str], at: &mut usize) -> Result<Vec<Vec<String>>, String> {
    match tokens.get(*at) {
        Some(&"(") => {
            *at += 1;
            let inner = or_expression(tokens, at)?;
            if tokens.get(*at) != Some(&")") {
                return Err("a license expression without its closing parenthesis".into());
            }
            *at += 1;
            Ok(inner)
        }
        Some(id) if !matches!(id.to_ascii_uppercase().as_str(), "AND" | "OR" | "WITH" | ")") => {
            *at += 1;
            let mut license = id.to_string();
            if tokens.get(*at).is_some_and(|t| t.eq_ignore_ascii_case("WITH")) {
                let exception = tokens.get(*at + 1).ok_or("a license expression ends after WITH")?;
                license = format!("{license} WITH {exception}");
                *at += 2;
            }
            Ok(vec![vec![license]])
        }
        _ => Err("a license expression with a missing license".into()),
    }
}

/// How much a license asks of an app that includes the library: nothing (public domain and the like),
/// its notice, its notice and more text, or its source code to be findable.
fn cost(license: &str) -> u32 {
    match license {
        "0BSD" | "Unlicense" | "CC0-1.0" => 0,
        "MIT" => 1,
        "MITNFA" | "ISC" | "Zlib" | "BSD-2-Clause" | "BSD-3-Clause" | "BlueOak-1.0.0" | "Unicode-3.0" | "Unicode-DFS-2016" => 2,
        _ if license.starts_with("Apache-2.0") => 3,
        "MPL-2.0" => 4,
        _ => 10,
    }
}

/// Of a crate's license alternatives, the one that asks the least (the first of equals).
pub fn choose(alternatives: &[Vec<String>]) -> Vec<String> {
    alternatives.iter().min_by_key(|a| a.iter().map(|l| cost(l)).sum::<u32>()).cloned().unwrap_or_default()
}

/// Letters and digits only, lower case: texts compared this way differ in more than spacing, line breaks,
/// quotes and punctuation.
fn squash(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// Which license a file's text is, from its wording.
pub fn kind_of(text: &str) -> Option<&'static str> {
    let t = squash(text);
    if t.contains("termsandconditionsforusereproductionanddistribution") && t.contains("apachelicense") {
        Some("Apache-2.0")
    } else if t.contains("mozillapubliclicense") && t.contains("version20") {
        Some("MPL-2.0")
    } else if t.contains("unicode") && t.contains("datafiles") && t.contains("permissionisherebygranted") {
        Some("Unicode")
    } else if t.contains("permissionisherebygrantedfreeofcharge") {
        Some("MIT")
    } else if t.contains("thissoftwareisprovidedasiswithoutanyexpressorimpliedwarranty") && t.contains("alteredsourceversionsmustbeplainlymarked") {
        Some("Zlib")
    } else if t.contains("redistributionanduseinsourceandbinaryforms") {
        Some(if t.contains("neitherthenameof") || t.contains("thenamesofitscontributors") { "BSD-3-Clause" } else { "BSD-2-Clause" })
    } else if t.contains("permissiontousecopymodifyandordistributethissoftwareforanypurposewithorwithoutfeeisherebygranted") {
        Some(if t.contains("providedthattheabovecopyrightnoticeandthispermissionnoticeappearinallcopies") { "ISC" } else { "0BSD" })
    } else if t.contains("blueoakmodellicense") {
        Some("BlueOak-1.0.0")
    } else if t.contains("thisisfreeandunencumberedsoftwarereleasedintothepublicdomain") {
        Some("Unlicense")
    } else if t.contains("creativecommons") && t.contains("cc0") {
        Some("CC0-1.0")
    } else {
        None
    }
}

/// The kind of text (see [`kind_of`]) that a license id is found in.
fn kind_for(license: &str) -> &str {
    if license.starts_with("Apache-2.0") {
        "Apache-2.0"
    } else if license == "MITNFA" {
        "MIT"
    } else if license.starts_with("Unicode") {
        "Unicode"
    } else {
        license
    }
}

/// An MIT license text's copyright lines (what comes before "Permission is hereby granted", without a title
/// such as "MIT License"), and whether the rest is the MIT license's usual text, word for word.
pub fn split_mit(text: &str) -> (Vec<String>, bool) {
    let Some(start) = text.to_ascii_lowercase().find("permission is hereby granted") else {
        return (Vec::new(), false);
    };
    let titles = ["mitlicense", "themitlicense", "themitlicensemit", "mitlicensemit", "mit"];
    let copyright = text[..start]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !titles.contains(&squash(l).as_str()))
        .map(str::to_string)
        .collect();
    (copyright, squash(&text[start..]) == squash(MIT_BODY))
}

/// A license file of a crate: its path inside the crate and its text.
#[derive(Debug, Clone)]
struct LicenseFile {
    path: String,
    text: String,
    kind: Option<&'static str>,
}

fn license_like(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["license", "licence", "copying", "unlicense", "notice", "copyright"].iter().any(|p| lower.starts_with(p))
}

/// The license files at the top of a crate (and in a LICENSES folder), and those deeper inside it, which
/// belong to code it contains (the C library inside secp256k1-sys, for example). Tests, examples and
/// benchmarks are left out.
fn license_files(dir: &Path) -> (Vec<LicenseFile>, Vec<LicenseFile>) {
    let mut top = Vec::new();
    let mut inner = Vec::new();
    walk(dir, dir, 0, &mut top, &mut inner);
    top.sort_by(|a, b| a.path.cmp(&b.path));
    inner.sort_by(|a, b| a.path.cmp(&b.path));
    (top, inner)
}

fn walk(root: &Path, dir: &Path, depth: usize, top: &mut Vec<LicenseFile>, inner: &mut Vec<LicenseFile>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let skip = ["tests", "test", "benches", "bench", "examples", "example", "target", "fuzz", "doc", "docs"];
            if name.starts_with('.') || skip.contains(&name.to_ascii_lowercase().as_str()) || depth >= 4 {
                continue;
            }
            // a LICENSES folder (REUSE style) holds the crate's own license texts
            if depth == 0 && name.eq_ignore_ascii_case("LICENSES") {
                for file in fs::read_dir(&path).into_iter().flatten().flatten() {
                    if let Some(found) = read_license(root, &file.path()) {
                        top.push(found);
                    }
                }
                continue;
            }
            walk(root, &path, depth + 1, top, inner);
        } else if license_like(&name) {
            if let Some(found) = read_license(root, &path) {
                if depth == 0 { top.push(found) } else { inner.push(found) }
            }
        }
    }
}

fn read_license(root: &Path, path: &Path) -> Option<LicenseFile> {
    let text = fs::read_to_string(path).ok()?;
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    Some(LicenseFile { kind: kind_of(&text), path: relative, text })
}

/// Of a crate's top-level license files, the one with this license's text (preferring a file named after it).
fn file_for<'a>(files: &'a [LicenseFile], license: &str) -> Option<&'a LicenseFile> {
    let kind = kind_for(license);
    let hint = match kind {
        "Apache-2.0" => "apache",
        "MIT" => "mit",
        "Zlib" => "zlib",
        "ISC" => "isc",
        _ => "",
    };
    let matching: Vec<&LicenseFile> = files.iter().filter(|f| f.kind == Some(kind)).collect();
    let plain = |f: &&&LicenseFile| !f.path.to_ascii_lowercase().contains("exception") || license.contains("exception");
    matching
        .iter()
        .filter(plain)
        .find(|f| !hint.is_empty() && f.path.to_ascii_lowercase().contains(hint))
        .or_else(|| matching.iter().find(plain))
        .copied()
}

// ---------------------------------------------------------------------------------------------------
// The notices
// ---------------------------------------------------------------------------------------------------

/// What the notices say, collected library by library.
#[derive(Default)]
struct Notices {
    /// One paragraph per library: its name, version, license and copyright lines.
    entries: Vec<String>,
    /// The libraries under the MIT license's usual text.
    mit: Vec<String>,
    apache: Vec<String>,
    /// NOTICE files of libraries used under the Apache license (its section 4 (d)).
    apache_notices: Vec<(String, String)>,
    mpl: Vec<String>,
    /// Any other license text, in full: (whose, text).
    other: Vec<(String, String)>,
}

impl Notices {
    /// A crate: the license it is used under, and its texts.
    fn add_crate(&mut self, package: &Package) -> Result<(), String> {
        let expression = package.license.clone().ok_or_else(|| format!("{} declares no license", package.title()))?;
        let alternatives = alternatives(&expression)?;
        let chosen = choose(&alternatives);
        let (top, inner) = license_files(&package.dir);
        let offered = if alternatives.len() > 1 { format!(" (of {expression})") } else { String::new() };
        // its first line says the license; its copyright lines and the rest follow, one per line
        let mut entry = vec![format!("{}: {}{offered}.", package.title(), chosen.join(" and "))];
        for license in &chosen {
            let kind = kind_for(license);
            match (cost(license), kind) {
                (0, _) => entry[0].push_str(" Needs no notice."),
                (_, "MIT") => {
                    let file = file_for(&top, license);
                    match file {
                        Some(file) if license == "MITNFA" || !split_mit(&file.text).1 => {
                            self.other.push((format!("{} ({license})", package.title()), file.text.clone()));
                        }
                        Some(file) => {
                            entry.extend(split_mit(&file.text).0);
                            self.mit.push(package.title());
                        }
                        None => {
                            // no license file in the crate: the license's standard text, with its authors
                            let holders = authors_line(package);
                            entry.push(holders.clone());
                            if license == "MITNFA" {
                                self.other.push((format!("{} ({license})", package.title()), format!("{holders}\n\n{MITNFA_BODY}")));
                            } else {
                                self.mit.push(package.title());
                            }
                        }
                    }
                }
                (_, "Apache-2.0") => {
                    self.apache.push(package.title());
                    for notice in top.iter().filter(|f| f.path.to_ascii_lowercase().starts_with("notice")) {
                        self.apache_notices.push((package.title(), notice.text.clone()));
                    }
                }
                (_, "MPL-2.0") => {
                    let source = format!("https://crates.io/crates/{}/{}", package.name, package.version);
                    let repository = package.repository.as_deref().map(|r| format!(", {r}")).unwrap_or_default();
                    entry.push(format!("Its source code: {source}{repository}."));
                    self.mpl.push(package.title());
                }
                _ => {
                    let file = file_for(&top, license)
                        .ok_or_else(|| format!("{} is under {license}, but its license text was not found in {}", package.title(), package.dir.display()))?;
                    self.other.push((format!("{} ({license})", package.title()), file.text.clone()));
                }
            }
        }
        // code inside the crate that has its own license (libsecp256k1 inside secp256k1-sys, for example)
        for file in inner.iter().filter(|f| !f.path.to_ascii_lowercase().contains("/notice")) {
            let folder = file.path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(&file.path);
            let part = folder.rsplit('/').next().unwrap_or(folder);
            let whose = format!("{part} (code inside {})", package.title());
            match file.kind {
                Some("MIT") if split_mit(&file.text).1 => {
                    entry.push(format!("It contains {part}, under the MIT license:"));
                    entry.extend(split_mit(&file.text).0);
                    self.mit.push(whose);
                }
                Some(kind) => {
                    entry.push(format!("It contains {part}, under {kind} (below)."));
                    self.other.push((format!("{whose} ({kind})"), file.text.clone()));
                }
                None => return Err(format!("{}: cannot tell the license of {}", package.title(), file.path)),
            }
        }
        self.entries.push(entry.join("\n"));
        Ok(())
    }
}

fn authors_line(package: &Package) -> String {
    let names: Vec<String> = package
        .authors
        .iter()
        .map(|a| a.split('<').next().unwrap_or(a).trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if names.is_empty() {
        format!("Copyright (c) the authors of {}.", package.name)
    } else {
        format!("Copyright (c) {}.", names.join(", "))
    }
}

/// A version in a build file: the text right after `marker` on the line that has it, up to `end`.
fn version_in(file: &Path, marker: &str, end: char) -> Result<String, String> {
    let text = fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    text.lines()
        .find_map(|line| line.split_once(marker).map(|(_, rest)| rest.split(end).next().unwrap_or("").to_string()))
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .ok_or_else(|| format!("the version after \"{marker}\" was not found in {}", file.display()))
}

/// The notices, for SeedLock `version`, built in the workspace at `root`.
pub fn android_notices(root: &Path, version: &str) -> Result<String, String> {
    let own_license = fs::read_to_string(root.join("LICENSE")).map_err(|e| format!("cannot read SeedLock's LICENSE: {e}"))?;
    let jna = version_in(&root.join("android/app/build.gradle.kts"), "net.java.dev.jna:jna:", '@')?;
    let kotlin = version_in(&root.join("android/build.gradle.kts"), "id(\"org.jetbrains.kotlin.android\") version \"", '"')?;

    let mut notices = Notices::default();
    for package in phone_library_crates(root)? {
        notices.add_crate(&package)?;
    }
    notices.entries.push(
        "Rust's standard library (std, core, alloc and the parts they include): MIT (of MIT OR Apache-2.0). Copyrights in \
         the Rust project are retained by their contributors. The parts of LLVM that the Android NDK adds to it \
         (libunwind, compiler-rt) are under Apache-2.0 WITH LLVM-exception, which asks for no notice in a compiled \
         program."
            .into(),
    );
    notices.mit.push("Rust's standard library".into());

    let mut java = [
        format!(
            "JNA {jna} (Java Native Access): Apache-2.0 (of Apache-2.0 OR LGPL-2.1-or-later). Its source code: \
             https://github.com/java-native-access/jna. What its LICENSE file says:\n\n{JNA_LICENSE}"
        ),
        format!("libffi (C code inside JNA's libjnidispatch.so): MIT.\n{}", split_mit(LIBFFI_LICENSE).0.join("\n")),
        format!("Kotlin's standard library {kotlin}: Apache-2.0.\nCopyright 2010-2024 JetBrains s.r.o. and Kotlin Programming Language contributors."),
        "JetBrains Java Annotations 13.0 (org.jetbrains:annotations, which Kotlin's standard library uses): Apache-2.0.\nCopyright 2000-2013 JetBrains s.r.o."
            .to_string(),
    ];
    if !split_mit(LIBFFI_LICENSE).1 {
        return Err("libffi's license is no longer the MIT text".into());
    }
    notices.mit.push("libffi".into());
    notices.apache.extend([format!("JNA {jna}"), format!("Kotlin's standard library {kotlin}"), "JetBrains Java Annotations 13.0".into()]);

    let mut out = String::new();
    // a heading: its title, underlined, as a paragraph of its own (the app shows it as a heading)
    let heading = |out: &mut String, text: &str| out.push_str(&format!("\n\n{text}\n{}\n\n", "=".repeat(text.chars().count())));
    out.push_str(&title(version));
    out.push_str(
        "\n\nSeedLock is free software under the MIT license. It is built from libraries by other authors, each under \
         its own license, and those licenses ask that you get their notices with the app: here they are. They were \
         written from the very libraries this app was built with.",
    );
    heading(&mut out, "SeedLock");
    out.push_str(&reflow(&own_license));
    heading(&mut out, "The Rust libraries in SeedLock's library (libseedlock_ffi.so)");
    out.push_str(&notices.entries.join("\n\n"));
    heading(&mut out, "The app's Java part");
    // JNA's LICENSE file is wrapped like a text file: reflowed; the other entries are one line each
    java[0] = reflow(&java[0]);
    out.push_str(&java.join("\n\n"));

    heading(&mut out, "The MIT License");
    out.push_str(&reflow(&format!(
        "The text of the MIT license, for each library above that is used under it, with that library's own \
         copyright lines given above ({}):\n\n{MIT_BODY}",
        notices.mit.join(", ")
    )));
    heading(&mut out, "Apache License, Version 2.0");
    out.push_str(&reflow(&format!("For {}.\n\n{APACHE_2_0}", notices.apache.join(", "))));
    for (whose, notice) in &notices.apache_notices {
        out.push_str(&format!("\n\nThe NOTICE file of {whose}:\n\n{}", reflow(notice)));
    }
    if !notices.mpl.is_empty() {
        heading(&mut out, "Mozilla Public License, Version 2.0");
        out.push_str(&reflow(&format!(
            "For {}, which SeedLock uses unchanged (where to find their source code is said above). The Kotlin code \
             that UniFFI writes for this app (org.seedlock.ffi) comes from UniFFI's templates.\n\n{MPL_2_0}",
            notices.mpl.join(", ")
        )));
    }
    if !notices.other.is_empty() {
        heading(&mut out, "Other licenses");
        let texts: Vec<String> = notices.other.iter().map(|(whose, text)| format!("{whose}:\n\n{}", reflow(text))).collect();
        out.push_str(&texts.join("\n\n"));
    }
    out.push('\n');
    Ok(out)
}

/// A text's paragraphs, each on one line (a phone's screen wraps them at its own width), with a blank line
/// between them. Lines that only underline a title (==== or ----) are left out; a title stays a paragraph.
pub fn reflow(text: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let flush = |current: &mut Vec<&str>, paragraphs: &mut Vec<String>| {
        if !current.is_empty() {
            paragraphs.push(current.join(" "));
            current.clear();
        }
    };
    for line in text.lines() {
        let line = line.trim();
        let underline = line.len() >= 3 && line.chars().all(|c| c == '=' || c == '-');
        if line.is_empty() {
            flush(&mut current, &mut paragraphs);
        } else if underline {
            // the line above was a title: a paragraph of its own
            flush(&mut current, &mut paragraphs);
        } else {
            if starts_item(line) && !current.is_empty() {
                flush(&mut current, &mut paragraphs);
            }
            current.push(line);
        }
    }
    flush(&mut current, &mut paragraphs);
    paragraphs.join("\n\n")
}

/// Whether a line starts an item of a list ("1.", "1.2.", "4)", "(a)", "*", "-"): it starts a paragraph.
fn starts_item(line: &str) -> bool {
    let first = line.split_whitespace().next().unwrap_or("");
    let numbered = first.len() <= 8
        && first.starts_with(|c: char| c.is_ascii_digit())
        && first.ends_with(['.', ')'])
        && first.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ')');
    let lettered = (3..=5).contains(&first.len()) && first.starts_with('(') && first.ends_with(')');
    numbered || lettered || matches!(first, "*" | "-" | "•")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alt(expression: &str) -> Vec<Vec<String>> {
        alternatives(expression).unwrap()
    }

    #[test]
    fn license_expressions_are_read_and_the_one_asking_least_is_chosen() {
        assert_eq!(alt("MIT OR Apache-2.0"), [vec!["MIT"], vec!["Apache-2.0"]]);
        assert_eq!(alt("MIT/Apache-2.0"), [vec!["MIT"], vec!["Apache-2.0"]]);
        assert_eq!(alt("(MIT OR Apache-2.0) AND ISC"), [vec!["MIT", "ISC"], vec!["Apache-2.0", "ISC"]]);
        assert_eq!(alt("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT")[0], ["Apache-2.0 WITH LLVM-exception"]);
        assert!(alternatives("MIT OR").is_err() && alternatives("(MIT").is_err() && alternatives("").is_err());
        assert_eq!(choose(&alt("Apache-2.0 OR MIT")), ["MIT"]);
        assert_eq!(choose(&alt("(MIT OR Apache-2.0) AND ISC")), ["MIT", "ISC"]);
        assert_eq!(choose(&alt("Unlicense OR MIT")), ["Unlicense"]);
        assert_eq!(choose(&alt("Zlib OR Apache-2.0 OR MIT")), ["MIT"], "MIT before other licenses that ask as little");
        assert_eq!(choose(&alt("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT")), ["MIT"]);
        assert_eq!(choose(&alt("MPL-2.0")), ["MPL-2.0"]);
    }

    #[test]
    fn license_texts_are_recognised_by_their_wording() {
        assert_eq!(kind_of(&format!("Copyright (c) 2020 Someone\n\n{MIT_BODY}")), Some("MIT"));
        assert_eq!(kind_of(APACHE_2_0), Some("Apache-2.0"));
        assert_eq!(kind_of(MPL_2_0), Some("MPL-2.0"));
        assert_eq!(kind_of(LIBFFI_LICENSE), Some("MIT"));
        assert_eq!(kind_of("MIT OR Apache-2.0"), None, "a file that only names the licenses is no license text");
        assert_eq!(kind_of("This software is provided 'as-is', without any express or implied warranty. ... 2. Altered source versions must be plainly marked as such"), Some("Zlib"));
        assert_eq!(kind_of(JNA_LICENSE), None);
    }

    #[test]
    fn the_mit_text_is_split_into_its_copyright_lines_and_its_usual_body() {
        let text = format!("The MIT License (MIT)\n\nCopyright (c) 2014 Alex\nCopyright (c) 2016 Bob\n\n{MIT_BODY}");
        let (copyright, usual) = split_mit(&text);
        assert_eq!(copyright, ["Copyright (c) 2014 Alex", "Copyright (c) 2016 Bob"]);
        assert!(usual);
        // libffi writes its quotes the old way, and wraps its lines differently: still the usual text
        let (copyright, usual) = split_mit(LIBFFI_LICENSE);
        assert!(usual && copyright[0].starts_with("libffi - Copyright (c) 1996-2022"), "{copyright:?}");
        // an extra condition makes it another license, kept in full
        let (_, usual) = split_mit(&format!("Copyright x\n\n{MITNFA_BODY}"));
        assert!(!usual);
    }

    #[test]
    fn texts_are_reflowed_into_paragraphs_for_the_phone() {
        let text = "Title\n=====\n\nFirst line\nof a paragraph.\n\n1. One\n   continued.\n2. Two\n\n- a\n- b\n";
        assert_eq!(reflow(text), "Title\n\nFirst line of a paragraph.\n\n1. One continued.\n\n2. Two\n\n- a\n\n- b");
        assert!(reflow(APACHE_2_0).contains("\"License\" shall mean the terms and conditions for use, reproduction, and distribution as defined by Sections 1 through 9 of this document."));
    }
}
