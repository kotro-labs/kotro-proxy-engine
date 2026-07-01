# Distribution channels

Packaging for three install surfaces lives under `distributions/` so the engine source tree stays clean.

```
distributions/
├── shared/binary-target.js     # Platform → release asset name (single source of truth)
├── SECRETS.md                  # NPM_TOKEN + VSCE_PAT setup for CI publish
├── vscode-extension/           # Cursor / VS Code IDE sidecar
├── npm-cli/                    # npm install -g @kortolabs/proxy-engine
├── homebrew/Formula/           # In-repo formula reference
└── homebrew-tap/               # Standalone tap repo scaffold (copy to github.com/ramairwing/homebrew-tap)
```

## Release asset layout

CI should upload cross-compiled binaries into each channel's `bin/` directory using these basenames:

| Platform | Asset |
|----------|-------|
| macOS Apple Silicon | `korto-proxy-aarch64-apple-darwin` |
| macOS Intel | `korto-proxy-x86_64-apple-darwin` |
| Linux x86_64 | `korto-proxy-x86_64-unknown-linux-gnu` |
| Windows x86_64 | `korto-proxy-x86_64-pc-windows-msvc.exe` |

Build example:

```bash
cd rust
cargo build --release -p korto-proxy
cp target/release/korto-proxy ../distributions/npm-cli/bin/korto-proxy-$(rustc -vV | ...)
```

## VS Code / Cursor extension

```bash
cd distributions/vscode-extension
npm install
npm run compile
# Copy release binaries into distributions/vscode-extension/bin/
# Package: npm run package
```

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which cross-compiles all four platform binaries, stages them into `distributions/*/bin/`, builds a `.vsix`, publishes to **npm** and the **VS Code Marketplace** when secrets are set, and creates a GitHub Release. See [SECRETS.md](SECRETS.md).

## NPM global CLI

```bash
cd distributions/npm-cli
npm install -g .
kortolabs-proxy   # forwards to the native binary for this platform
```

## Homebrew tap

Copy `homebrew/Formula/kortolabs-proxy.rb` into a `homebrew-tap` repository, replace `sha256` placeholders after publishing GitHub release tarballs, then:

```bash
brew install kortolabs/tap/kortolabs-proxy
```

After a GitHub release completes, stamp checksums automatically (updates both in-repo and tap scaffold formulas):

```bash
scripts/update-homebrew-shas.sh v0.1.0
# or
make update-homebrew-shas VERSION=v0.1.0
```

Copy the stamped tap formula into `github.com/ramairwing/homebrew-tap` — see [homebrew-tap/README.md](homebrew-tap/README.md).
