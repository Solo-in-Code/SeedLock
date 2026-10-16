#!/bin/sh
# Builds the Rust part of SeedLock's Android app, before Android Studio builds the app itself:
#   - libseedlock_ffi.so for the three kinds of processor phones have, into app/src/main/jniLibs/
#   - the Kotlin code that calls it (package org.seedlock.ffi), into app/src/main/java/
#
#   sh android/build-rust.sh        (Linux and macOS; on Windows: android\build-rust.ps1)
#
# Needs Rust installed with rustup (README, Part 1, Way B), cargo-ndk (cargo install cargo-ndk), the
# Android NDK (Android Studio: Settings > Languages & Frameworks > Android SDK > SDK Tools > NDK (Side by
# side)) and an internet connection. Run it again whenever the Rust code changed. android/README.md says more.
set -eu
cd "$(dirname "$0")/.."

if ! command -v rustup >/dev/null 2>&1; then
    echo "This script needs Rust installed with rustup (README, Part 1, Way B)." >&2
    exit 1
fi
if ! command -v cargo-ndk >/dev/null 2>&1; then
    echo "This script needs cargo-ndk. Install it once with:  cargo install cargo-ndk" >&2
    exit 1
fi
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# Rust writes the full path of each library's source files into the program, and that path holds this
# computer's user name: --remap-path-prefix writes ~ in its place (as tools/build-release.sh does).
unset RUSTFLAGS
flags="--remap-path-prefix=$HOME=~"
if [ -n "${CARGO_HOME:-}" ]; then
    flags="$flags$(printf '\037')--remap-path-prefix=$CARGO_HOME=~/.cargo"
fi
jnilibs=android/app/src/main/jniLibs
rm -rf "$jnilibs"
echo "Building libseedlock_ffi.so for 64-bit phones (arm64-v8a), 32-bit phones (armeabi-v7a) and the emulator (x86_64) ..."
CARGO_ENCODED_RUSTFLAGS="$flags" cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -P 24 -o "$jnilibs" build --profile phone --locked -p seedlock-ffi

# The Kotlin code is written from a build for this computer: a debug build keeps the description that
# uniffi-bindgen reads (the build for phones has it stripped), and the code is the same for
# every kind of processor.
echo "Writing the Kotlin code that calls it ..."
cargo build --locked -p seedlock-ffi -p uniffi-bindgen
case "$(uname -s)" in
    Darwin) host_lib=target/debug/libseedlock_ffi.dylib ;;
    *) host_lib=target/debug/libseedlock_ffi.so ;;
esac
rm -rf android/app/src/main/java/org/seedlock/ffi
target/debug/uniffi-bindgen generate --no-format --language kotlin --out-dir android/app/src/main/java "$host_lib"

for lib in "$jnilibs"/*/libseedlock_ffi.so; do
    for folder in "$HOME" ${CARGO_HOME:+"$CARGO_HOME"}; do
        if grep -qaF "$folder/" "$lib"; then
            echo "$lib still contains the path $folder (and with it your user name): something went wrong." >&2
            exit 1
        fi
    done
done
echo
echo "Ready:"
ls -l "$jnilibs"/*/libseedlock_ffi.so android/app/src/main/java/org/seedlock/ffi/*.kt
echo "Now open the folder \"android\" in Android Studio and build the app (android/README.md)."
