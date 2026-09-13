#!/bin/sh
# Downloads the official VB-CABLE driver pack that the SoundPush Windows installer bundles
# (plan stage 1, see docs/virtual-microphone.md).
#
# - VB-CABLE is donationware by VB-Audio (https://vb-audio.com/Cable/).
# - NOT WIRED INTO THE BUILD YET. The licence inside the package (readme.txt) says:
#   "It is not allowed to integrate the VB-CABLE package in another software installation
#   procedure without Author agreement." Bundling is enabled only after VB-Audio's written
#   agreement (see docs/virtual-microphone.md §4).
# - The package is never committed to git. It is pinned by SHA-256, so every build ships
#   exactly the package that was reviewed.
# - To update: change VERSION, run this script once, and put the new checksum in SHA256.
#
# Works in Git Bash (Windows, including GitHub Actions) and on macOS/Linux.
# Output: drivers/vbcable/package/ (bundled into the installer as "vbcable").
set -eu

VERSION=45
URL="https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack${VERSION}.zip"
SHA256="b950e39f01af1d04ea623c8f6d8eb9b6ea5c477c637295fabf20631c85116bfb"

here=$(cd "$(dirname "$0")" && pwd)
out="$here/package"
zip="$here/VBCABLE_Driver_Pack${VERSION}.zip"

if [ -f "$out/VBCABLE_Setup_x64.exe" ]; then
    echo "VB-CABLE package already present: $out"
    exit 0
fi

curl -fsSL -o "$zip" "$URL"

# sha256sum on Linux/Git Bash, shasum on macOS.
actual=$( (sha256sum "$zip" 2>/dev/null || shasum -a 256 "$zip") | cut -d' ' -f1)
if [ "$actual" != "$SHA256" ]; then
    rm -f "$zip"
    echo "VB-CABLE download checksum mismatch (got $actual). Refusing to bundle it." >&2
    exit 1
fi

rm -rf "$out"
mkdir -p "$out"
unzip -q "$zip" -d "$out"
rm -f "$zip"
echo "VB-CABLE package ready: $out"
