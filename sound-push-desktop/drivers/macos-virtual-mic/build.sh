#!/bin/sh
# Builds SoundPushMicrophone.driver (universal arm64 + x86_64, ad-hoc signed).
# Usage: build.sh [output-dir]   (default: ./build)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/build"}
bundle="$out/SoundPushMicrophone.driver"

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS"
cp "$here/Info.plist" "$bundle/Contents/Info.plist"

xcrun clang -std=c11 -O2 -Wall -Wextra -Werror \
    -arch arm64 -arch x86_64 -mmacosx-version-min=13.0 \
    -bundle -fvisibility=hidden \
    -framework CoreAudio -framework CoreFoundation \
    -o "$bundle/Contents/MacOS/SoundPushMicrophone" \
    "$here/SoundPushMicrophone.c"

codesign --force --sign - "$bundle"
echo "$bundle"
