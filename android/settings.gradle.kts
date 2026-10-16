// SeedLock's Android app. Open this folder ("android") in Android Studio; android/README.md explains
// how to build it. The Rust part (libseedlock_ffi.so and its Kotlin code) is made first, by build-rust.sh.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "SeedLock"
include(":app")
