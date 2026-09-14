# Homebrew cask template: tools/release/fill-manifests.mjs fills in the placeholders. See packaging/README.md.
cask "soundpush" do
  version "@VERSION@"
  sha256 "@SHA256_MACOS_DMG@"

  url "@REPO_URL@/releases/download/v#{version}/SoundPush_#{version}_universal.dmg"
  name "SoundPush"
  desc "Use your phone as a speaker, microphone or headset for your computer"
  homepage "@DOCS_URL@/"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  # Needs macOS 14.2 (Core Audio process taps); Homebrew can only express the major version.
  depends_on macos: ">= :sonoma"

  app "SoundPush.app"

  uninstall quit: "net.soundpush.desktop"

  zap trash: [
    "/Library/Audio/Plug-Ins/HAL/SoundPushMicrophone.driver",
    "~/Library/Application Support/SoundPush",
    "~/Library/Application Support/net.soundpush.desktop",
    "~/Library/Caches/net.soundpush.desktop",
    "~/Library/Preferences/net.soundpush.desktop.plist",
    "~/Library/Saved Application State/net.soundpush.desktop.savedState",
    "~/Library/WebKit/net.soundpush.desktop",
  ]
end
