#!/bin/sh
# Checks the Android app's way into Rust on a computer: builds seedlock-ffi for this computer, writes
# its Kotlin code with uniffi-bindgen, and runs JvmCheck.kt in the JVM through JNA, the way the app
# calls the same code on a phone. embit (Python) makes the transaction and checks the signature, and
# urtypes the wallet's QR code for Sparrow.
#
#   KOTLINC=/path/to/kotlinc/bin/kotlinc JNA_JAR=/path/to/jna.jar sh android/jvm-check/run.sh
#
# Needs: Rust, a JDK (java), the Kotlin compiler, JNA's jar (Debian, Ubuntu: apt install libjna-java,
# then /usr/share/java/jna.jar), and Python 3 with embit, mnemonic and urtypes (pip install embit
# mnemonic urtypes).
# Linux and macOS.
set -eu
cd "$(dirname "$0")/../.."
: "${KOTLINC:=kotlinc}"
: "${JNA_JAR:=/usr/share/java/jna.jar}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo build --locked -p seedlock-ffi -p uniffi-bindgen
case "$(uname -s)" in
    Darwin) lib=target/debug/libseedlock_ffi.dylib ;;
    *) lib=target/debug/libseedlock_ffi.so ;;
esac
# the bindings are read from a library that is not stripped (a debug build): they are the same for
# every processor, the phone's included
target/debug/uniffi-bindgen generate --no-format --language kotlin --out-dir "$work/gen" "$lib"
python3 android/jvm-check/psbt_check.py make "$work"
"$KOTLINC" -nowarn -include-runtime -cp "$JNA_JAR" -d "$work/check.jar" "$work/gen/org/seedlock/ffi/seedlock_ffi.kt" android/jvm-check/JvmCheck.kt
java -Djna.library.path=target/debug -cp "$work/check.jar:$JNA_JAR" JvmCheckKt "$work"
python3 android/jvm-check/psbt_check.py verify "$work"
echo "ALL JVM CHECKS PASSED"
