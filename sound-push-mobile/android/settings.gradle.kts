pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

plugins {
    // Fetches the JDK the modules ask for (jvmToolchain) when it is not installed on this machine.
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "SoundPush"

include(
    ":app",
    ":core-engine",
    ":core-ui",
    ":feature-home",
    ":feature-devices",
    ":feature-audio",
    ":feature-settings",
    ":platform-service",
)
