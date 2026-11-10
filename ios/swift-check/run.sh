#!/bin/sh
# Calls SeedLock's library the way the iPhone app does, on a computer: through the Swift code that uniffi-bindgen
# writes, with the app's own storage, helpers, new backups, wallet for Sparrow as text and reader of the license notices
# (ios/SeedLock/Backups.swift, Helpers.swift, NewSeeds.swift, SparrowText.swift and Notices.swift), and the notices that
# ios/build-rust.sh writes (main.swift says what it checks).
# The rest of the app needs iOS. Needs Rust and a Swift compiler: on a Mac, Xcode's; on Linux, Swift (swift.org, or a
# package such as Debian's and Ubuntu's swiftlang). In SeedLock's folder:
#
#   sh ios/swift-check/run.sh            (SWIFTC=/path/to/swiftc sh ios/swift-check/run.sh for another compiler)
set -eu
cd "$(dirname "$0")/../.."
swiftc=${SWIFTC:-swiftc}
if ! command -v "$swiftc" >/dev/null 2>&1; then
    echo "This check needs a Swift compiler (swiftc): Xcode on a Mac, or Swift on Linux." >&2
    exit 1
fi

cargo build --locked -p seedlock-ffi -p uniffi-bindgen -p seedlock-tools
case "$(uname -s)" in
    Darwin)
        lib=target/debug/libseedlock_ffi.dylib
        # Backups.swift uses what macOS 13 and later have, as iOS 16 and later do
        platform="-target $(uname -m)-apple-macos13"
        ;;
    *)
        lib=target/debug/libseedlock_ffi.so
        platform=""
        ;;
esac
out=target/swift-check
rm -rf "$out"
mkdir -p "$out/headers/seedlock_ffiFFI"
target/debug/uniffi-bindgen generate --no-format --language swift --out-dir "$out" "$lib"
# the C part as the module seedlock_ffiFFI, laid out as ios/build-rust.sh lays it out for Xcode
mv "$out/seedlock_ffiFFI.h" "$out/headers/seedlock_ffiFFI/"
mv "$out/seedlock_ffiFFI.modulemap" "$out/headers/seedlock_ffiFFI/module.modulemap"
target/debug/seedlock-tools notices --ios --out "$out/notices.txt"

echo "Compiling the check ..."
# shellcheck disable=SC2086 # $platform is empty, or two words
"$swiftc" $platform -swift-version 5 -I "$out/headers" -L target/debug -lseedlock_ffi \
    -Xlinker -rpath -Xlinker "$PWD/target/debug" \
    ios/swift-check/main.swift ios/SeedLock/Backups.swift ios/SeedLock/Helpers.swift ios/SeedLock/Notices.swift \
    ios/SeedLock/NewSeeds.swift ios/SeedLock/SparrowText.swift \
    "$out/seedlock_ffi.swift" -o "$out/swift-check"
"$out/swift-check" "$out/notices.txt"
