# Release Guide

## Build Workflow

### macOS (Universal Binary)

```bash
# Install Rust targets for universal binary
rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Build release
cargo tauri build --target universal-apple-darwin

# Output: apps/marauder/src-tauri/target/universal-apple-darwin/release/bundle/
#   - dmg/Marauder_<version>_universal.dmg
#   - macos/Marauder.app
```

### Linux

```bash
# Install system dependencies (Debian/Ubuntu)
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev

# Build release
cargo tauri build

# Output: apps/marauder/src-tauri/target/release/bundle/
#   - deb/marauder_<version>_amd64.deb
#   - appimage/marauder_<version>_amd64.AppImage
```

### Windows

```bash
cargo tauri build

# Output: apps/marauder/src-tauri/target/release/bundle/
#   - msi/Marauder_<version>_x64_en-US.msi
#   - nsis/Marauder_<version>_x64-setup.exe
```

## Code Signing

### macOS

1. Set environment variables:
   ```bash
   export APPLE_CERTIFICATE="<base64-encoded .p12>"
   export APPLE_CERTIFICATE_PASSWORD="<password>"
   export APPLE_SIGNING_IDENTITY="Developer ID Application: <name> (<team-id>)"
   export APPLE_ID="<apple-id-email>"
   export APPLE_PASSWORD="<app-specific-password>"
   export APPLE_TEAM_ID="<team-id>"
   ```
2. Tauri automatically signs and notarizes when these variables are set during `cargo tauri build`.

### Windows

1. **Authenticode code signing**: Obtain an EV code signing certificate and configure `signtool.exe` in your CI. Tauri invokes the system code signing tool automatically when configured via `tauri.conf.json > bundle > windows > certificateThumbprint`.
2. **Tauri update signing** (separate from Authenticode): Set `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` for Tauri's built-in updater signature verification.

## Homebrew Tap Setup

1. Create a GitHub repo: `LayerDynamics/homebrew-tap`
2. Copy `dist/homebrew/marauder.rb` into the tap repo as `Casks/marauder.rb`
3. Update the `sha256` in the formula after uploading the DMG to GitHub Releases
4. Users install with:
   ```bash
   brew tap LayerDynamics/tap
   brew install --cask marauder
   ```

## GitHub Release Checklist

1. Update version in `apps/marauder/src-tauri/tauri.conf.json` and `Cargo.toml`
2. Create a git tag: `git tag v<version> && git push --tags`
3. Build for each platform (or use CI)
4. Create GitHub Release with the tag
5. Upload artifacts: `.dmg`, `.deb`, `.AppImage`, `.msi`, `.exe`
6. Update Homebrew cask formula with new version and SHA256

## CI/CD

Recommended: Use GitHub Actions with `tauri-apps/tauri-action` for automated builds on tag push. See [Tauri GitHub Action docs](https://github.com/tauri-apps/tauri-action).
