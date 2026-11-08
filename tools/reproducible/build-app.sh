#!/bin/sh
# Runs inside the fixed build environment (tools/reproducible/Dockerfile), never on its own: it builds SeedLock's phone
# app, not signed yet, into dist/ and checks it. Everything here is the same for everyone who builds it (the folder,
# the tools, the settings), so the app comes out the same, byte for byte. tools/sign-apk.sh signs it afterwards.
#
# If the release's app (seedlock-VERSION-android.apk) is in SeedLock's folder, it is compared with this build too:
# its signature is checked, and it is written without it into dist/, for tools/build-release.sh to compare.
#
# With --gradle-checksums (from tools/build-release.sh --gradle-checksums), it writes Gradle's list of the files it
# downloads for the app, each with its checksum, into dist/ instead: see step 3.
set -eu

mode=${1:-}
if [ -n "$mode" ] && [ "$mode" != --gradle-checksums ]; then
    echo "Unknown option: $mode (the only one is --gradle-checksums)." >&2
    exit 1
fi
# Gradle's list of the files it downloads for the app, with their checksums (step 3)
metadata=android/gradle/verification-metadata.xml
if [ -z "$mode" ] && [ ! -f "$metadata" ]; then
    echo "$metadata is missing: without it, Gradle would not check the files it" >&2
    echo "downloads for the app. Write it with: sh tools/build-release.sh --gradle-checksums (docs/DEVELOPMENT.md," >&2
    echo "\"Checksums of the app's libraries\")." >&2
    exit 1
fi

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
#    Gradle's background processes and caches, from nothing. Gradle checks every file it downloads for it (Android's
#    build plugin, Kotlin, JNA, and all that they use) against the list android/gradle/verification-metadata.xml,
#    which gives each file's SHA-256, and stops if a file is not in the list or has another checksum: its dependency
#    verification, "strict" here, whatever a gradle.properties file may say.
gradle_build() {
    gradle --no-daemon --no-build-cache --no-configuration-cache --console=plain -p android \
        -Pkotlin.compiler.execution.strategy=in-process "$@"
}
# The files in such a list, one line each: group:name:version/file and its SHA-256, sorted
listed_files() {
    awk '
        /<component / {
            match($0, /group="[^"]*"/); group = substr($0, RSTART + 7, RLENGTH - 8)
            match($0, /name="[^"]*"/); name = substr($0, RSTART + 6, RLENGTH - 7)
            match($0, /version="[^"]*"/); version = substr($0, RSTART + 9, RLENGTH - 10)
        }
        /<artifact / { match($0, /name="[^"]*"/); file = substr($0, RSTART + 6, RLENGTH - 7) }
        /<sha256 / {
            match($0, /value="[^"]*"/)
            print group ":" name ":" version "/" file " " substr($0, RSTART + 7, RLENGTH - 8)
        }
    ' "$1" | LC_ALL=C sort
}

if [ "$mode" = --gradle-checksums ]; then
    # 3, with --gradle-checksums: the list, written anew by the same builds (the release build, and the test build
    #    that Android Studio makes), so that it holds every file they download, and nothing else. Asked to add to a
    #    list, Gradle would quietly trust a file whose checksum changed as well; so the list before is put aside, and
    #    compared with the new one at the end.
    before=target/verification-metadata-before.xml
    rm -f "$before"
    if [ -f "$metadata" ]; then
        mv "$metadata" "$before"
    fi
    # What Gradle checks: each file's SHA-256, the libraries' descriptions (.pom, .module) included; not signatures,
    # which most of Google's files do not have. The sources and documentation of a library, which Android Studio
    # downloads to show them, are never part of a build, so they need none.
    cat > "$metadata" <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<verification-metadata xmlns="https://schema.gradle.org/dependency-verification" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="https://schema.gradle.org/dependency-verification https://schema.gradle.org/dependency-verification/dependency-verification-1.3.xsd">
   <configuration>
      <verify-metadata>true</verify-metadata>
      <verify-signatures>false</verify-signatures>
      <trusted-artifacts>
         <trust file=".*-javadoc[.]jar" regex="true"/>
         <trust file=".*-sources[.]jar" regex="true"/>
      </trusted-artifacts>
   </configuration>
</verification-metadata>
XML
    echo "Writing Gradle's list of the files it downloads for the app, each with its checksum ..."
    gradle_build --write-verification-metadata sha256 assembleRelease assembleDebug
    # aapt2 (which compiles the app's resources) is downloaded for the system that builds: here, Linux. For macOS and
    # Windows too, so that Android Studio there checks it as well: aapt2-all-systems.init.gradle.kts, here.
    pattern='s/^ *<component group="com\.android\.tools\.build" name="aapt2" version="\([^"]*\)">$/\1/p'
    aapt2=$(sed -n "$pattern" "$metadata")
    if [ -z "$aapt2" ] || [ "$(echo "$aapt2" | awk 'END { print NR }')" -ne 1 ]; then
        echo "Gradle's list does not hold one version of aapt2: did Android's build plugin change how it gets it?" >&2
        exit 1
    fi
    gradle_build --write-verification-metadata sha256 \
        --init-script "$PWD/tools/reproducible/aapt2-all-systems.init.gradle.kts" -Pseedlock.aapt2="$aapt2" help

    listed_files "$metadata" > target/listed-after.txt
    files=$(awk 'END { print NR }' target/listed-after.txt)
    components=$(awk '/<component / { n++ } END { print n + 0 }' "$metadata")
    if [ "$files" -eq 0 ]; then
        echo "Gradle wrote no file into its list: something went wrong (see the messages above)." >&2
        exit 1
    fi
    report=target/report.txt
    echo "Gradle's list of the files it downloads for the app: $files files, of $components libraries and plugins." \
        > "$report"
    if [ -f "$before" ]; then
        listed_files "$before" > target/listed-before.txt
        # A file of a version never changes: the same file with another checksum than before is a warning sign.
        changed=$(LC_ALL=C join target/listed-before.txt target/listed-after.txt | awk '$2 != $3 { print "  " $1 }')
        if [ -n "$changed" ]; then
            {
                echo
                echo "STOP: these files have another checksum now than in the list before:"
                echo "$changed"
                echo "A file of a version never changes, so do not use the new list: first find out why these did"
                echo "(docs/DEVELOPMENT.md, \"Checksums of the app's libraries\")."
            } >&2
            exit 1
        fi
        cut -d' ' -f1 target/listed-before.txt > target/files-before.txt
        cut -d' ' -f1 target/listed-after.txt > target/files-after.txt
        LC_ALL=C comm -13 target/files-before.txt target/files-after.txt > target/files-new.txt
        LC_ALL=C comm -23 target/files-before.txt target/files-after.txt > target/files-gone.txt
        new=$(awk 'END { print NR }' target/files-new.txt)
        gone=$(awk 'END { print NR }' target/files-gone.txt)
        {
            if [ "$new" -eq 0 ] && [ "$gone" -eq 0 ]; then
                echo "The same files as in the list before, each with the same checksum."
            else
                echo "Compared with the list before: $new files are new, $gone are no longer needed, and all the"
                echo "others have the same checksums."
                if [ "$new" -gt 0 ]; then
                    echo "New:"
                    sed 's/^/  /' target/files-new.txt
                fi
                if [ "$gone" -gt 0 ]; then
                    echo "No longer needed:"
                    sed 's/^/  /' target/files-gone.txt
                fi
            fi
        } >> "$report"
    else
        echo "There was no list before, so there is nothing to compare this one with." >> "$report"
    fi
    mkdir -p dist
    cp "$metadata" dist/verification-metadata.xml
    cp "$report" dist/report.txt
    chmod 644 dist/verification-metadata.xml dist/report.txt
    # What the builds left behind (some GB) need not be kept with the list.
    rm -rf target android/app/build android/build android/.gradle
    exit 0
fi

echo "Building the app $version ..."
if ! gradle_build --dependency-verification strict assembleRelease; then
    # said again at the end, where Docker shows it when the build stops (Gradle writes this report then)
    if [ -d android/build/reports/dependency-verification ]; then
        echo >&2
        echo "STOP: Gradle's dependency verification failed. A file that Gradle downloaded for the app is not the" >&2
        echo "one in $metadata (Gradle names it above): do not build the app with it," >&2
        echo "see \"Checksums of the app's libraries\" in docs/DEVELOPMENT.md." >&2
    fi
    exit 1
fi
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
