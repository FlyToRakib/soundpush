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
    npm --prefix sound-push-desktop/ui run check
    npm --prefix sound-push-desktop/ui run build

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
