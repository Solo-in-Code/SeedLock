//! Helpers for building SeedLock's phone app, run on the computer that builds it; never part of SeedLock
//! itself. In SeedLock's folder:
//!
//! ```text
//! seedlock-tools notices --android [--out FILE]   the license notices the app shows (android/build-rust.sh)
//! seedlock-tools check-apk FILE                   checks a release APK and puts it into dist/ (DEVELOPMENT.md)
//! ```
//!
//! or with `cargo run --locked -p seedlock-tools -- ...` in place of `seedlock-tools`.

mod apk;
mod notices;

const USAGE: &str = "SeedLock's tools for the phone app (run them in SeedLock's folder):
  cargo run --locked -p seedlock-tools -- notices --android --out FILE   write the app's license notices
  cargo run --locked -p seedlock-tools -- check-apk FILE                 check a release APK, put it into dist/";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("notices") => notices::run(&args[1..]),
        Some("check-apk") => apk::run(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
