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
    }

    buildTypes {
        release {
            // no shrinking or renaming: the app stays exactly as its source reads
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

// Only the app itself is ever built, org.seedlock.app (app-debug.apk). The app has no instrumented tests
// of its own (SeedLock's tests are in Rust, and in android/jvm-check), so the separate test app that
// Android Studio would otherwise build too (org.seedlock.app.test, app-debug-androidTest.apk: no icon,
// nothing to open) is switched off, and cannot be installed by mistake.
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
