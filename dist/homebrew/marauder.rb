cask "marauder" do
  version "0.1.0"
  # Replace with: shasum -a 256 Marauder_<version>_universal.dmg
  sha256 :no_check

  url "https://github.com/LayerDynamics/marauder/releases/download/v#{version}/Marauder_#{version}_universal.dmg"
  name "Marauder"
  desc "GPU-accelerated terminal emulator built with Rust, Deno, and Tauri"
  homepage "https://github.com/LayerDynamics/marauder"

  depends_on macos: ">= :monterey"

  app "Marauder.app"

  zap trash: [
    "~/Library/Application Support/com.ryanoboyle.marauder",
    "~/Library/Caches/com.ryanoboyle.marauder",
    "~/Library/Preferences/com.ryanoboyle.marauder.plist",
    "~/.config/marauder",
  ]
end
