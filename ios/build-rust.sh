#!/bin/sh
# Builds the Rust part of SeedLock's iPhone app, before Xcode builds the app itself (on a Mac):
#   - libseedlock_ffi.a for iPhones, and for the iPhone simulator on this Mac, put together with the C header and
#     the module map that let Swift call it: ios/SeedLockFFI.xcframework, which the Xcode project links
#   - the Swift code that calls it, into ios/SeedLock/Generated/seedlock_ffi.swift
#   - the license notices of the libraries the app is built from, which the app shows (About SeedLock and its
#     licenses), into ios/SeedLock/Generated/notices.txt
#
#   sh ios/build-rust.sh
#
# Needs Xcode 16 or newer (opened once, so that it finished installing), Rust installed with rustup (README, Part 1,
# Way B) and, the first time, an internet connection (rustup adds the iPhone targets, cargo downloads the libraries).
# Run it again whenever the Rust code changed. ios/README.md says more.
set -eu
cd "$(dirname "$0")/.."

if [ "$(uname -s)" != Darwin ]; then
    echo "The iPhone app is built on a Mac, with Xcode." >&2
    exit 1
fi
if ! command -v rustup >/dev/null 2>&1; then
    echo "This script needs Rust installed with rustup (README, Part 1, Way B)." >&2
    exit 1
fi
if ! xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1 || ! xcrun --sdk iphonesimulator --show-sdk-path >/dev/null 2>&1; then
    echo "This script needs Xcode itself, not only Apple's command line tools. Install Xcode, open it once, then run:" >&2
    echo "  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer" >&2
    exit 1
fi

# iPhones, and the simulator for this Mac's own kind of processor (an Apple chip, or an Intel processor)
case "$(uname -m)" in
    arm64) simulator=aarch64-apple-ios-sim ;;
    *) simulator=x86_64-apple-ios ;;
esac
rustup target add aarch64-apple-ios "$simulator"

# The oldest iOS the app runs on: the same as IPHONEOS_DEPLOYMENT_TARGET in ios/SeedLock.xcodeproj (the tests check).
export IPHONEOS_DEPLOYMENT_TARGET=17.0

# Rust writes the full path of each library's source files into the program, and that path holds this computer's
# user name: --remap-path-prefix writes ~ in its place.
unset RUSTFLAGS
flags="--remap-path-prefix=$HOME=~"
if [ -n "${CARGO_HOME:-}" ]; then
    flags="$flags$(printf '\037')--remap-path-prefix=$CARGO_HOME=~/.cargo"
fi
for target in aarch64-apple-ios "$simulator"; do
    echo "Building libseedlock_ffi.a for $target ..."
    CARGO_ENCODED_RUSTFLAGS="$flags" cargo rustc --locked -p seedlock-ffi --lib --crate-type staticlib --profile phone --target "$target"
done
libs="target/aarch64-apple-ios/phone/libseedlock_ffi.a target/$simulator/phone/libseedlock_ffi.a"
for lib in $libs; do
    for folder in "$HOME" ${CARGO_HOME:+"$CARGO_HOME"}; do
        if grep -qaF "$folder/" "$lib"; then
            echo "$lib still contains the path $folder (and with it your user name): something went wrong." >&2
            exit 1
        fi
    done
done

# The Swift code is written from a build for this Mac: a debug build keeps the description that uniffi-bindgen reads
# (the build for phones has it stripped), and the code is the same for every kind of processor.
echo "Writing the Swift code that calls it, and the license notices ..."
cargo build --locked -p seedlock-ffi -p uniffi-bindgen -p seedlock-tools
generated=ios/SeedLock/Generated
bindings=target/ios-bindings
rm -rf "$generated" "$bindings"
mkdir -p "$generated" "$bindings/headers/seedlock_ffiFFI"
target/debug/uniffi-bindgen generate --no-format --language swift --out-dir "$bindings" target/debug/libseedlock_ffi.dylib
mv "$bindings/seedlock_ffi.swift" "$generated/"
# The C part, which the Swift code imports as the module seedlock_ffiFFI: its header, and its module map under the
# name the compiler looks for, in a folder named after the module (as the Matrix SDK lays out its own).
mv "$bindings/seedlock_ffiFFI.h" "$bindings/headers/seedlock_ffiFFI/"
mv "$bindings/seedlock_ffiFFI.modulemap" "$bindings/headers/seedlock_ffiFFI/module.modulemap"
# the notices of exactly the libraries just built in (crates/seedlock-tools)
target/debug/seedlock-tools notices --ios --out "$generated/notices.txt"

echo "Putting the libraries together for Xcode (ios/SeedLockFFI.xcframework) ..."
rm -rf ios/SeedLockFFI.xcframework
xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/phone/libseedlock_ffi.a -headers "$bindings/headers" \
    -library "target/$simulator/phone/libseedlock_ffi.a" -headers "$bindings/headers" \
    -output ios/SeedLockFFI.xcframework

echo
echo "Ready:"
# shellcheck disable=SC2086 # $libs is two file names, without spaces
ls -l $libs "$generated"/*
echo "Now open ios/SeedLock.xcodeproj in Xcode and build the app (ios/README.md)."
