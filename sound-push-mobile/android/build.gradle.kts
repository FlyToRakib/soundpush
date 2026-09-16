import org.jlleitschuh.gradle.ktlint.KtlintExtension

plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.android.test) apply false
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.aboutlibraries) apply false
    alias(libs.plugins.ktlint) apply false
}

// Kotlin style on every module (plan §29.3: ktlint on every PR). The rules come from .editorconfig,
// so IntelliJ and `./gradlew ktlintCheck` agree. `ktlintFormat` fixes what can be fixed.
val ktlintVersion = libs.versions.ktlint.get()
subprojects {
    apply(plugin = "org.jlleitschuh.gradle.ktlint")
    extensions.configure<KtlintExtension> {
        version.set(ktlintVersion)
        // Machine-written UniFFI bindings: regenerating them would undo any reformatting.
        // (The generated Compose theme is handled by its own .editorconfig section.)
        filter { exclude { "/uniffi/" in it.file.invariantSeparatorsPath } }
    }
}
