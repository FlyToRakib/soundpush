plugins {
    alias(libs.plugins.android.test)
    alias(libs.plugins.kotlin.android)
}

// Startup and jank measurements (plan §29.1 "Android UI"). This module is not part of the app: it
// drives an installed release-like build from a second process on a device or emulator, which is why
// it cannot run on the JVM alongside the other tests. See docs/testing-guide.md for how to run it.
android {
    namespace = "net.soundpush.benchmark"
    compileSdk = 36

    defaultConfig {
        // Macrobenchmark needs the tracing APIs added in Android 7.
        minSdk = 26
        targetSdk = 36
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // CI runs on an emulator, and a phone on a desk is rarely charged and locked: the numbers are read
        // as a trend, so these conditions do not abort the run. Set here rather than on the command line,
        // which the configuration cache does not support.
        testInstrumentationRunnerArguments["androidx.benchmark.suppressErrors"] = "EMULATOR,LOW-BATTERY,UNLOCKED"
    }

    buildTypes {
        // Matches the app's "benchmark" build type: release code, profileable, debug-signed.
        create("benchmark") {
            isDebuggable = false
            // The app's libraries only have debug and release. The tested app's dependencies are resolved
            // from this module as well, so it needs the same fallback as the app's own benchmark type.
            matchingFallbacks += "release"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    targetProjectPath = ":app"
    // The benchmark drives the app from its own process, so the runner must not instrument it.
    experimentalProperties["android.experimental.self-instrumenting"] = true
}

kotlin { jvmToolchain(17) }

dependencies {
    implementation(libs.androidx.test.ext.junit)
    implementation(libs.androidx.test.runner)
    implementation(libs.androidx.test.uiautomator)
    implementation(libs.espresso.core)
    implementation(libs.benchmark.macro.junit4)
    implementation(libs.junit)
}
