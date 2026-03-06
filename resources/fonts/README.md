# Fonts

Marauder uses JetBrains Mono as the default terminal font.

## Installation

Fonts are downloaded automatically during `bin/install.sh`. To install manually:

```bash
# Download JetBrains Mono v2.304
curl -fsSL https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip -o /tmp/jbmono.zip
unzip -o /tmp/jbmono.zip -d ~/.config/marauder/fonts/
rm /tmp/jbmono.zip
```

## License

JetBrains Mono is licensed under the [SIL Open Font License 1.1](https://github.com/JetBrains/JetBrainsMono/blob/master/OFL.txt).

## Bundled Fonts

No font files are committed to this repository. The install script downloads them at install time to keep the repo lightweight.

## Fallback Chain

Marauder uses this font fallback chain:
1. JetBrains Mono (downloaded)
2. Fira Code (system)
3. Cascadia Code (system)
4. SF Mono (macOS system)
5. Menlo (macOS system)
6. monospace (generic)
