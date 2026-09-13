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
