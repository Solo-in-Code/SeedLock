#!/bin/sh
# Runs inside the fixed build environment (tools/reproducible/Dockerfile), never on its own: it builds SeedLock's phone
# app, not signed yet, into dist/ and checks it. Everything here is the same for everyone who builds it (the folder,
# the tools, the settings), so the app comes out the same, byte for byte. tools/sign-apk.sh signs it afterwards.
#
# If the release's app (seedlock-VERSION-android.apk) is in SeedLock's folder, it is compared with this build too:
# its signature is checked, and it is written without it into dist/, for tools/build-release.sh to compare.
set -eu

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "The version was not found in Cargo.toml." >&2
    exit 1
fi

# The same file paths in the Rust library as in the programs (tools/reproducible/build-programs.sh).
sep=$(printf '\037')
paths="--remap-path-prefix=$CARGO_HOME=~/.cargo$sep--remap-path-prefix=$PWD=."
unset RUSTFLAGS CARGO_BUILD_RUSTFLAGS
export CARGO_TARGET_DIR="$PWD/target"
# Gradle keeps what it downloads and makes in target too, so that it goes with it at the end
export GRADLE_USER_HOME="$PWD/target/gradle"

cargo fetch --locked
export CARGO_NET_OFFLINE=true

# 1. libseedlock_ffi.so for the phone's three kinds of processor, with the NDK's compiler and linker, set up as
#    cargo-ndk does (android/build-rust.sh, which builds it for Android Studio): clang for Android 7.0 (API 24) and
#    newer. NDK r28 lays out the 64-bit libraries for phones with 16 KB memory pages by itself.
ndk_bin="$ANDROID_HOME/ndk/$ANDROID_NDK_VERSION/toolchains/llvm/prebuilt/linux-x86_64/bin"
jnilibs=android/app/src/main/jniLibs
rm -rf "$jnilibs"
for build in aarch64-linux-android:arm64-v8a:aarch64-linux-android24 \
             armv7-linux-androideabi:armeabi-v7a:armv7a-linux-androideabi24 \
             x86_64-linux-android:x86_64:x86_64-linux-android24; do
    target=${build%%:*}
    rest=${build#*:}
    abi=${rest%%:*}
    clang_target=${rest#*:}
    lower=$(echo "$target" | tr '-' '_')
    upper=$(echo "$lower" | tr '[:lower:]' '[:upper:]')
    echo "Building libseedlock_ffi.so $version for $abi ..."
    env "CC_$lower=$ndk_bin/clang" "CFLAGS_$lower=--target=$clang_target" "AR_$lower=$ndk_bin/llvm-ar" \
        "CARGO_TARGET_${upper}_LINKER=$ndk_bin/clang" \
        CARGO_ENCODED_RUSTFLAGS="$paths$sep-Clink-arg=--target=$clang_target" \
        cargo build --profile phone --locked -p seedlock-ffi --target "$target"
    mkdir -p "$jnilibs/$abi"
    cp "target/$target/phone/libseedlock_ffi.so" "$jnilibs/$abi/"
done

# 2. The Kotlin code that calls it, and the license notices the app shows, as android/build-rust.sh writes them.
echo "Writing the Kotlin code that calls it, and the license notices ..."
cargo build --locked -p seedlock-ffi -p uniffi-bindgen -p seedlock-tools
rm -rf android/app/src/main/java/org/seedlock/ffi
target/debug/uniffi-bindgen generate --no-format --language kotlin --out-dir android/app/src/main/java \
    target/debug/libseedlock_ffi.so
target/debug/seedlock-tools notices --android --out android/app/src/main/assets/notices.txt

# 3. The app, with Gradle and Android's build tools: its release build, which Gradle leaves unsigned here. Without
#    Gradle's background processes and caches, from nothing.
echo "Building the app $version ..."
gradle --no-daemon --no-build-cache --no-configuration-cache --console=plain -p android \
    -Pkotlin.compiler.execution.strategy=in-process assembleRelease
mkdir -p dist
cp android/app/build/outputs/apk/release/app-release-unsigned.apk "dist/seedlock-$version-android-unsigned.apk"
chmod 644 "dist/seedlock-$version-android-unsigned.apk"

# 4. What the app must be (the version, a release build, its permissions, libraries, code and notices, none of this
#    environment's folders), before it is signed: seedlock-tools.
target/debug/seedlock-tools check-apk --unsigned "dist/seedlock-$version-android-unsigned.apk"

# 5. The release's app, if it is here: its signature must be valid, and without it, it goes into dist/ (the build
#    script compares the two).
release="seedlock-$version-android.apk"
if [ -f "$release" ]; then
    echo "Checking the release's app, $release ..."
    apksigner verify --print-certs "$release"
    target/debug/seedlock-tools unsigned-apk "$release" "dist/seedlock-$version-android-release-without-signature.apk"
fi

# What the builds left behind (some GB) need not be kept with the app.
rm -rf target android/app/build android/build android/.gradle
