#!/bin/sh
# Builds the ready-made SeedLock program that a release offers for download, on Linux or on a Mac, and
# names it after the version (from Cargo.toml) and the system. The file goes into the folder "dist":
#
#   Linux (x86-64): dist/seedlock-VERSION-linux-x86_64  a static program: it runs on any 64-bit Intel or AMD
#                                                       Linux, old or new, because it needs no system library
#   macOS:          dist/seedlock-VERSION-macos         one file for Macs with Apple chips and with Intel chips
#
# Neither contains the user name of the computer that built it (see below). Windows: tools/build-release.ps1.
#
#   sh tools/build-release.sh
#
# Needs Rust installed with rustup (README, Part 1, Way B), which adds what is missing here, and an internet
# connection. On Linux also musl, a small C library that the static program is built with:
#   Debian, Ubuntu: sudo apt install musl-tools     Arch: sudo pacman -S musl     Fedora: sudo dnf install musl-gcc
set -eu
cd "$(dirname "$0")/.."
# The release programs are built the same way everywhere: into this folder's "target", and with only the compiler
# flags below. CARGO_ENCODED_RUSTFLAGS replaces every other source of flags (RUSTFLAGS, Cargo's settings files), so
# nothing from this computer's settings gets in (for example a target-cpu that other computers do not have).
#
# Rust writes the full path of each library's source files into the program, for its error messages, and that path
# holds this computer's user name: /home/NAME/.cargo/... on Linux, /Users/NAME/.cargo/... on a Mac.
# --remap-path-prefix makes it write ~ in place of the home folder; the end of this script checks that it did.
export CARGO_TARGET_DIR="$PWD/target"
unset RUSTFLAGS
flags="--remap-path-prefix=$HOME=~"
if [ -n "${CARGO_HOME:-}" ]; then
    # Cargo keeps the libraries' source files in another folder: that one too (\037 separates the flags)
    flags="$flags$(printf '\037')--remap-path-prefix=$CARGO_HOME=~/.cargo"
fi
export CARGO_ENCODED_RUSTFLAGS="$flags"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "The version was not found in Cargo.toml." >&2
    exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
    cat >&2 <<'MSG'
This script needs Rust installed with rustup, the official installer (README, Part 1, Way B):
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
then open a new terminal and run this script again.
MSG
    exit 1
fi
mkdir -p dist

case "$(uname -s)" in
    Linux)
        if [ "$(uname -m)" != x86_64 ]; then
            echo "The ready-made Linux program is made for x86-64 (Intel and AMD) processors only, and this computer has $(uname -m)." >&2
            echo "On this computer, build SeedLock as the README explains (Part 1, Way B)." >&2
            exit 1
        fi
        if ! command -v musl-gcc >/dev/null 2>&1; then
            cat >&2 <<'MSG'
musl-gcc was not found. Install musl first:
  Debian, Ubuntu : sudo apt install musl-tools
  Arch           : sudo pacman -S musl
  Fedora         : sudo dnf install musl-gcc
then run this script again.
MSG
            exit 1
        fi
        target=x86_64-unknown-linux-musl
        rustup target add "$target"
        echo "Building SeedLock $version for Linux (static, $target) ..."
        CC_x86_64_unknown_linux_musl=musl-gcc cargo build --release --locked --target "$target"
        out="dist/seedlock-$version-linux-x86_64"
        cp "target/$target/release/seedlock" "$out"
        ;;
    Darwin)
        rustup target add aarch64-apple-darwin x86_64-apple-darwin
        # The oldest macOS versions the program starts on: 11 (Big Sur) on Apple chips, 10.12 (Sierra) on Intel.
        echo "Building SeedLock $version for Macs with Apple chips ..."
        MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked --target aarch64-apple-darwin
        echo "Building SeedLock $version for Macs with Intel chips ..."
        MACOSX_DEPLOYMENT_TARGET=10.12 cargo build --release --locked --target x86_64-apple-darwin
        out="dist/seedlock-$version-macos"
        rm -f "$out"
        lipo -create -output "$out" target/aarch64-apple-darwin/release/seedlock target/x86_64-apple-darwin/release/seedlock
        codesign --force --sign - "$out"     # an ad-hoc signature, which Macs with Apple chips need to start a program
        lipo -info "$out"
        ;;
    *)
        printf '%s\n' 'This script runs on Linux and macOS. On Windows: powershell -ExecutionPolicy Bypass -File tools\build-release.ps1' >&2
        exit 1
        ;;
esac

chmod 755 "$out"
shown=$("$out" --version)
if [ "$shown" != "seedlock $version" ]; then
    echo "The new program says \"$shown\" instead of \"seedlock $version\": something went wrong." >&2
    exit 1
fi
for folder in "$HOME" ${CARGO_HOME:+"$CARGO_HOME"}; do
    if grep -qaF "$folder/" "$out"; then
        echo "The new program still contains the path $folder (and with it your user name): something went wrong." >&2
        exit 1
    fi
done
# A released program works only offline, with no way around it: the stand-in network that the test builds read
# (crates/seedlock-cli, feature test-hooks) must not be in it.
if grep -qaF SEEDLOCK_TEST_NET_DIR "$out"; then
    echo "The new program can be made to believe that the computer is offline (it was built for the tests): something went wrong." >&2
    exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum "$out" | cut -d' ' -f1)
else
    sum=$(shasum -a 256 "$out" | cut -d' ' -f1)
fi
echo
echo "Ready: $out"
echo "SHA-256: $sum"
echo "Bring this file (and the other systems' files) to one computer to make SHA256SUMS.txt: see \"Making a release\" in docs/DEVELOPMENT.md."
