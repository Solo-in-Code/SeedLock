import com.android.build.api.variant.DeviceTestBuilder
import com.android.build.api.variant.HasDeviceTestsBuilder

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The version comes from SeedLock's Cargo.toml ([workspace.package] version), the one place it is set,
// so that the app and the command-line program always carry the same number.
val seedlockVersion: String = rootDir.resolve("../Cargo.toml").readLines()
    .first { it.startsWith("version = ") }
    .substringAfter('"').substringBefore('"')
val versionParts: List<Int> = seedlockVersion.split('.').map { it.toInt() }

android {
    namespace = "org.seedlock.app"
    compileSdk = 35
    // The versions of the build tools and the NDK (whose llvm-strip Android's build runs over the native libraries)
    // that the fixed build environment holds (tools/reproducible/Dockerfile), so that a build here uses the same.
    buildToolsVersion = "34.0.0"
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "org.seedlock.app"
        // Android 7.0 and newer
        minSdk = 24
        // 34: the pages keep Android's usual room for the status bar (35 would draw under it)
        targetSdk = 34
        versionCode = versionParts[0] * 10000 + versionParts[1] * 100 + versionParts[2]
        versionName = seedlockVersion
        // 64-bit phones, older 32-bit phones, and the emulator on a PC
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
        // the app's name under its icon (AndroidManifest.xml); a test build is named otherwise, below
        manifestPlaceholders["appLabel"] = "SeedLock"
    }

    buildTypes {
        // A test build (what Android Studio's Run and Generate APK(s) make) is another app beside the
        // released one: "SeedLock test", org.seedlock.app.debug, with storage of its own. Trying a build never
        // touches the released app or its backups, and Android, which keeps an app to the key it was first
        // signed with, never has to choose between Android Studio's test key and SeedLock's own.
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-test"
            manifestPlaceholders["appLabel"] = "SeedLock test"
        }
        // The release: built in the fixed build environment (tools/build-release.sh), signed with SeedLock's own key
        // (tools/sign-apk.sh) and checked with seedlock-tools check-apk (docs/DEVELOPMENT.md, Making a release).
        release {
            // no shrinking or renaming: the app stays exactly as its source reads
            isMinifyEnabled = false
            // never a release that can be debugged (Android's default for a release, said here on purpose)
            isDebuggable = false
            // nothing about the folder it was built in (whether it is a git checkout, and which commit), so that the
            // app comes out the same from the release's Source code (zip) and from a git checkout
            vcsInfo.include = false
        }
    }

    // No list of the libraries the app is built from in its signature block (only Google Play reads it, and the
    // app is not on Google Play): the release's app is the fixed environment's build with nothing but a signature.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    lint {
        // A release build stops when the targetSdk is lower than the one Google Play asks of the apps it publishes
        // (ExpiredTargetSdkVersion). SeedLock's app is not on Google Play, and its targetSdk of 34 is on purpose
        // (above). Every other problem that Android's checks count as fatal still stops a release build.
        disable += "ExpiredTargetSdkVersion"
    }
}

// Only the app itself is ever built (app-debug.apk, org.seedlock.app.debug; app-release.apk, org.seedlock.app).
// The app has no instrumented tests of its own (SeedLock's tests are in Rust, and in android/jvm-check), so
// the separate test app that Android Studio would otherwise build too (org.seedlock.app.debug.test,
// app-debug-androidTest.apk: no icon, nothing to open) is switched off, and cannot be installed by mistake.
androidComponents {
    // (the test app is only ever made for the debug build, Android's "test build type")
    beforeVariants(selector().withBuildType("debug")) { variant ->
        (variant as? HasDeviceTestsBuilder)?.deviceTests?.get(DeviceTestBuilder.ANDROID_TEST_TYPE)?.enable = false
    }
}

dependencies {
    // JNA: how the Kotlin code that uniffi-bindgen writes (org.seedlock.ffi) calls libseedlock_ffi.so.
    // 5.18.1 runs on phones with 16 KB memory pages (fixed in 5.16 and 5.17), without the deadlock that
    // 5.16 brought (fixed in 5.18); Mozilla's UniFFI components for Firefox use the same version.
    implementation("net.java.dev.jna:jna:5.18.1@aar")
}
