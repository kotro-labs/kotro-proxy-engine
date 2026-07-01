# Distribution channels

Packaging for three install surfaces lives under `distributions/` so the engine source tree stays clean.

```
distributions/
├── shared/binary-target.js     # Platform → release asset name (single source of truth)
├── vscode-extension/           # Cursor / VS Code IDE sidecar
├── npm-cli/                    # npm install -g @kortolabs/proxy-engine
└── homebrew/Formula/           # brew install kortolabs/tap/kortolabs-proxy
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

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which cross-compiles all four platform binaries, stages them into `distributions/*/bin/`, builds a `.vsix`, and publishes a GitHub release. Set repository secret `NPM_TOKEN` to enable automated npm publish.

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
