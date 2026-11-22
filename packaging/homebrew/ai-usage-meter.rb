cask "ai-usage-meter" do

  version "0.7.1"
  sha256 "fbb8398b42c6cb8a06e97a73a5b3dd168b248c1933987e90cb73af745aa13755"

  url "https://github.com/TTNAN/ai-usage-meter/releases/download/v#{version}/AI.Usage.Meter_#{version}_universal.dmg",
      verified: "github.com/TTNAN/ai-usage-meter"
  name "AI Usage Meter"
  desc "Claude, ChatGPT (Codex), Grok, Gemini and Copilot quota in the menu bar"
  homepage "https://github.com/TTNAN/ai-usage-meter"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :catalina"

  app "AI Usage Meter.app"

  # The app is not notarized: macOS attaches a quarantine attribute to the
  # downloaded DMG's payload, and a plain `brew install` copy would be refused
  # on first launch ("cannot be opened because Apple cannot check it"). Strip
  # the attribute right after the copy, which is exactly what the README's
  # `xattr -cr` one-liner does by hand.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-cr", "#{appdir}/AI Usage Meter.app"]
  end

  caveats <<~EOS
    AI Usage Meter is not notarized: the cask strips macOS's quarantine
    attribute after installing, so the app launches directly. If macOS still
    asks, right-click the app and choose Open.

    The app reads the logins the official CLIs already saved on this machine
    (Claude Code, Codex, Grok Build, Gemini CLI, GitHub Copilot) and keeps
    everything on-device: no servers, no analytics.
  EOS

  uninstall quit: "com.ttnan.ai-usage-meter"

  zap trash: [
    "~/Library/Application Support/com.ttnan.ai-usage-meter",
    "~/Library/Caches/com.ttnan.ai-usage-meter",
    "~/Library/Preferences/com.ttnan.ai-usage-meter.plist",
    "~/Library/Saved Application State/com.ttnan.ai-usage-meter.savedState",
    "~/Library/WebKit/com.ttnan.ai-usage-meter",
  ]
end
