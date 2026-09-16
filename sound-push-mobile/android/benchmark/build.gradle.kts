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
    }

    buildTypes {
        // Matches the app's "benchmark" build type: release code, profileable, debug-signed.
        create("benchmark") {
            isDebuggable = false
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
