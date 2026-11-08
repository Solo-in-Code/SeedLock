// Used only when tools/reproducible/build-app.sh --gradle-checksums writes Gradle's list of the files it downloads for
// the app, each with its checksum (android/gradle/verification-metadata.xml). Android's build plugin downloads aapt2,
// the program that compiles the app's resources, for the system that builds: in the fixed build environment, for
// Linux. This makes Gradle download it for macOS and Windows too, so that the list holds their checksums as well, and
// Android Studio on a Mac or on Windows builds the test app with every file checked. build-app.sh gives the version,
// the one the build plugin downloaded for Linux.
gradle.rootProject {
    val version = property("seedlock.aapt2") as String
    val aapt2 = configurations.create("seedlockAapt2AllSystems") { isCanBeConsumed = false }
    for (system in listOf("osx", "windows")) {
        dependencies.add(aapt2.name, "com.android.tools.build:aapt2:$version:$system@jar")
    }
}
