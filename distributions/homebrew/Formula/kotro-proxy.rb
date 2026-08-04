class KotroProxy < Formula
  desc "Local security and efficiency layer for MCP-native agentic AI — injection scanning, secret redaction, semantic cache, agent loop protection"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.6.3"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.6.3/kotro-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "9f75d44f21e38cf9965321c2d7cdd12e31c4850a94de007b98e7d63a68b9ca05"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.6.3/kotro-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "1b864a6fa502dcb165da0760ffdd48e6e0e37cd5925bf59d3a2bf473455957fe"
    end
  end

  def install
    asset = Dir["kotro-proxy-*"].first
    odie "Expected exactly one kotro-proxy binary in the release tarball" if asset.nil?
    bin.install asset => "kotro-proxy"
  end

  test do
    # Binary --version tracks the crate version embedded in the release.
    assert_match(/kotro-proxy/, shell_output("#{bin}/kotro-proxy --version"))
  end
end
