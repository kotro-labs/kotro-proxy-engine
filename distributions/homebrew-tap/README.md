# homebrew-tap

Public Homebrew tap for installing the Korto proxy on macOS.

This directory is a **standalone tap repository scaffold**. Publish it as:

`https://github.com/kotro-labs/homebrew-tap`

## One-time setup

```bash
# Create the tap repo on GitHub, then:
git clone git@github.com:kotro-labs/homebrew-tap.git
cp distributions/homebrew-tap/Formula/kortolabs-proxy.rb homebrew-tap/Formula/
cd homebrew-tap && git add Formula && git commit -m "Add kortolabs-proxy formula" && git push
```

## Install for end users

```bash
brew tap kotro-labs/tap
brew install kortolabs-proxy
kortolabs-proxy --version
```

## Updating checksums after a release

From the main `kotro-proxy-engine` repository (after CI publishes `v*` assets):

```bash
scripts/update-homebrew-shas.sh v0.1.0
```

This updates both:

- `distributions/homebrew/Formula/kortolabs-proxy.rb` (in-engine reference)
- `distributions/homebrew-tap/Formula/kortolabs-proxy.rb` (tap scaffold)

Then copy or sync the stamped formula into your `homebrew-tap` repo and push.

```bash
cp distributions/homebrew-tap/Formula/kortolabs-proxy.rb ../homebrew-tap/Formula/
cd ../homebrew-tap && git commit -am "Bump kortolabs-proxy to v0.1.0" && git push
```

## Repository layout

```
homebrew-tap/
└── Formula/
    └── kortolabs-proxy.rb
```

Homebrew expects the repo name `homebrew-tap` (or `homebrew-*`) so `brew tap kotro-labs/tap` resolves to `github.com/kotro-labs/homebrew-tap`.
