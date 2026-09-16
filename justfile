# SoundPush task runner — https://github.com/casey/just

ndk := env_var_or_default("ANDROID_NDK_HOME", env_var("HOME") + "/Library/Android/sdk/ndk/27.2.12479018")

default:
    @just --list

# Shared core ---------------------------------------------------------------

core-test:
    cargo test --workspace --exclude sound-push-desktop

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

tokens:
    node design/scripts/generate.mjs

# Desktop ------------------------------------------------------------------

desktop-deps:
    npm --prefix sound-push-desktop/ui ci

desktop-dev:
    cd sound-push-desktop && ./ui/node_modules/.bin/tauri dev

desktop-build:
    cd sound-push-desktop && ./ui/node_modules/.bin/tauri build

desktop-check:
    npm --prefix sound-push-desktop/ui run lint
    npm --prefix sound-push-desktop/ui run check
    npm --prefix sound-push-desktop/ui run build

# Rewrite the formatting differences `desktop-check` reports.
desktop-format:
    npm --prefix sound-push-desktop/ui run format

# Mobile -------------------------------------------------------------------

mobile-bindings:
    cargo build -p sp-ffi
    rm -rf sound-push-mobile/android/core-engine/src/main/java/uniffi
    cargo run -q -p sp-ffi --bin uniffi-bindgen -- generate --library target/debug/libsoundpush_ffi.{{ if os() == "macos" { "dylib" } else { "so" } }} --language kotlin --out-dir sound-push-mobile/android/core-engine/src/main/java --no-format

mobile-native:
    ANDROID_NDK_HOME={{ndk}} cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 --platform 26 -o sound-push-mobile/android/core-engine/src/main/jniLibs build -p sp-ffi --release

mobile-build: mobile-bindings mobile-native
    cd sound-push-mobile/android && ./gradlew assembleDebug

mobile-install: mobile-build
    adb install -r sound-push-mobile/android/app/build/outputs/apk/debug/app-debug.apk

# The Android quality gate CI runs: build, App Bundle, unit tests, Android lint, Kotlin style.
mobile-check:
    cd sound-push-mobile/android && ./gradlew assembleDebug bundleDebug testDebugUnitTest lint ktlintCheck --console=plain

# Fix the Kotlin style violations ktlint can fix by itself.
mobile-format:
    cd sound-push-mobile/android && ./gradlew ktlintFormat

# Rewrite gradle/verification-metadata.xml after a dependency changes (§31). It downloads everything
# again into a throwaway Gradle home, which is slow but the only way to get a complete file: Gradle
# writes a checksum only for what it actually fetches, so a warm cache silently leaves out the POM
# and .module files it had already parsed, and the build then fails on a cold machine — CI — with
# "checksums are missing from verification metadata". Afterwards, add back the aapt2 entries for the
# operating systems this machine is not: AGP picks aapt2 by OS, so a file generated on one machine
# misses the others. The entries in the file say where they came from.
mobile-verification:
    rm -rf sound-push-mobile/android/.gradle-cold-home
    cd sound-push-mobile/android && ./gradlew -g .gradle-cold-home --write-verification-metadata sha256 assembleDebug bundleDebug assembleRelease bundleRelease testDebugUnitTest lint ktlintCheck :benchmark:assembleBenchmark
    cd sound-push-mobile/android && ./gradlew -g .gradle-cold-home --stop
    rm -rf sound-push-mobile/android/.gradle-cold-home

# Startup and jank measurements against a connected phone (adb devices) or a running emulator.
mobile-benchmark:
    cd sound-push-mobile/android && ./gradlew :benchmark:connectedBenchmarkAndroidTest --console=plain

# Tools --------------------------------------------------------------------

# End-to-end pipeline latency over a simulated link; `just latency --help` for the options.
latency *args:
    cargo run -q -p soundpush-latency-probe --bin latency-probe -- {{args}}

# Network impairment profiles, simulated and applied for real; `just netsim list` shows them.
netsim *args:
    cargo run -q -p soundpush-netsim --bin netsim -- {{args}}
