# Homebrew cask for Verbatim. Source-of-truth template: the release channel
# workflow (.github/workflows/channels.yml) copies this file into the
# homebrew-verbatim tap and replaces the __VERSION__ / __*_SHA256__ tokens with
# the published release's tag and the SHA-256 sums from SHA256SUMS, then opens a
# PR against the tap. Never hand-edit the tap copy; edit this template.
cask "verbatim" do
  version "__VERSION__"

  on_arm do
    sha256 "__ARM64_SHA256__"
    url "https://github.com/underthinker/verbatim/releases/download/v#{version}/Verbatim_#{version}_aarch64.dmg"
  end
  on_intel do
    sha256 "__X64_SHA256__"
    url "https://github.com/underthinker/verbatim/releases/download/v#{version}/Verbatim_#{version}_x64.dmg"
  end

  name "Verbatim"
  desc "Privacy-first, fully local dictation - hotkey to polished text, no cloud"
  homepage "https://github.com/underthinker/verbatim"

  # Verbatim self-updates through its own hash-verified channel mechanism; keep
  # Homebrew from also nagging about the same upgrade.
  auto_updates true
  depends_on macos: ">= :big_sur" # matches tauri minimumSystemVersion 11.0

  app "Verbatim.app"

  # The daemon stores config/models/history under the platform data dir; leave
  # user data in place on `brew uninstall`, purge it only on `--zap`.
  zap trash: [
    "~/Library/Application Support/app.verbatim.dictation",
    "~/Library/Caches/app.verbatim.dictation",
    "~/Library/Preferences/app.verbatim.dictation.plist",
    "~/Library/Logs/app.verbatim.dictation",
  ]
end
