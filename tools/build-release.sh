#!/bin/sh
# Builds the ready-made SeedLock program that a release offers for download, on Linux or on a Mac, and
# names it after the version (from Cargo.toml) and the system. The file goes into the folder "dist":
#
#   Linux (x86-64): dist/seedlock-VERSION-linux-x86_64  a static program: it runs on any 64-bit Intel or AMD
#                                                       Linux, old or new, because it needs no system library
#   macOS:          dist/seedlock-VERSION-macos         one file for Macs with Apple chips and with Intel chips
#
# Windows is built with two commands, see "Making a release" in docs/DEVELOPMENT.md.
#
#   sh tools/build-release.sh
#
# Needs Rust installed with rustup (README, Part 1, Way B), which adds what is missing here, and an internet
# connection. On Linux also musl, a small C library that the static program is built with:
#   Debian, Ubuntu: sudo apt install musl-tools     Arch: sudo pacman -S musl     Fedora: sudo dnf install musl-gcc
set -eu
cd "$(dirname "$0")/.."
# The release programs are built the same way everywhere: into this folder's "target", and without extra compiler
# flags that this computer's settings might add (for example a target-cpu that other computers do not have).
export CARGO_TARGET_DIR="$PWD/target"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS

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
        echo "This script runs on Linux and macOS. On Windows, see \"Making a release\" in docs/DEVELOPMENT.md." >&2
        exit 1
        ;;
esac

chmod 755 "$out"
shown=$("$out" --version)
if [ "$shown" != "seedlock $version" ]; then
    echo "The new program says \"$shown\" instead of \"seedlock $version\": something went wrong." >&2
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
