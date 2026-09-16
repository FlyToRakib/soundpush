plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    // Open-source licences screen, generated from the resolved dependencies at build time.
    alias(libs.plugins.aboutlibraries)
}

/** A release signing value from the environment (CI) or from gradle.properties (a maintainer). */
fun signingValue(env: String, property: String): String? = System.getenv(env)?.takeIf { it.isNotBlank() }
    ?: providers.gradleProperty(property).orNull?.takeIf { it.isNotBlank() }

val releaseKeystore: String? = signingValue("ANDROID_KEYSTORE_FILE", "soundpush.keystore.file")

android {
    namespace = "net.soundpush.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "net.soundpush.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }

    // One debug key for every machine and CI run, so a build from anywhere updates an installed
    // debug build instead of failing with "signatures do not match". Not a secret: debug only.
    signingConfigs {
        getByName("debug") {
            storeFile = rootProject.file("debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
        // The release key (plan §32) never lives in the repository: CI decodes it from a secret and
        // points these variables at it, and a maintainer can set them in ~/.gradle/gradle.properties.
        // Without them there is no "release" config at all and the release build keeps the public
        // debug key, which is what every build on this branch does today.
        releaseKeystore?.let { keystore ->
            create("release") {
                storeFile = file(keystore)
                storePassword = signingValue("ANDROID_KEYSTORE_PASSWORD", "soundpush.keystore.password")
                keyAlias = signingValue("ANDROID_KEY_ALIAS", "soundpush.key.alias")
                keyPassword = signingValue("ANDROID_KEY_PASSWORD", "soundpush.key.password")
                enableV1Signing = true
                enableV2Signing = true
                enableV3Signing = true
            }
        }
    }

    buildTypes {
        debug {
            // en-XA (long text) and ar-XB (right to left) pseudo-locales, offered in the language picker.
            isPseudoLocalesEnabled = true
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
        // What the Macrobenchmark module measures (plan §29.1): the release build, minified and not
        // debuggable, but profileable so the benchmark can read frame and startup timings.
        create("benchmark") {
            initWith(getByName("release"))
            signingConfig = signingConfigs.getByName("debug")
            isProfileable = true
            matchingFallbacks += "release"
        }
    }

    // Play builds one download per device from the bundle (plan §32: AAB with per-ABI splits). The
    // language split stays off: the in-app picker offers every translation this build carries, so
    // they must all be installed, not just the ones matching the system language.
    bundle {
        abi { enableSplit = true }
        density { enableSplit = true }
        language { enableSplit = false }
    }

    buildFeatures { compose = true }

    // The app's languages (Android 13+ system settings and the in-app picker) are generated from the
    // values-<lang> folders, so a new translation needs no code change. See docs/translating.md.
    androidResources { generateLocaleConfig = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs { useLegacyPackaging = false }
    }

    // Screenshot tests render every screen (Light + Dark) on the JVM with Robolectric.
    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            all { it.systemProperty("roborazzi.test.record", "true") }
        }
    }
}

kotlin { jvmToolchain(17) }

aboutLibraries {
    // Licence data from the dependencies' POM files only, never fetched from the network: same inputs, same list.
    offlineMode = true
}

dependencies {
    implementation(project(":core-ui"))
    implementation(project(":feature-home"))
    implementation(project(":feature-devices"))
    implementation(project(":feature-audio"))
    implementation(project(":feature-settings"))
    implementation(project(":platform-service"))
    implementation(libs.androidx.activity.compose)
    // Per-app language on Android 8–12 (AppCompatDelegate.setApplicationLocales).
    implementation(libs.androidx.appcompat)
    implementation(libs.aboutlibraries.compose.m3)
    implementation(libs.androidx.core.ktx)
    implementation(libs.navigation.compose)
    implementation(libs.androidx.lifecycle.process)

    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.compose.ui.test.junit4.accessibility)
    testImplementation(libs.androidx.test.ext.junit)
    debugImplementation(platform(libs.compose.bom))
    debugImplementation(libs.compose.ui.test.manifest)
}
