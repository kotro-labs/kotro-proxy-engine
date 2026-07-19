class KotroProxy < Formula
  desc "Local security and efficiency layer for MCP-native agentic AI — injection scanning, secret redaction, semantic cache, agent loop protection"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.6.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.6.0/kotro-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "6cddff6e4d626cd901be49f39bfd29f27b0e81f745fd8de32b357bd7a28e9f8f"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.6.0/kotro-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "d7419f6ec37dec1e192e579ebc5d5c542f8454996fc6eb88d614d03e1df2603d"
    end
  end

  def install
    asset = Dir["kotro-proxy-*"].first
    odie "Expected exactly one kotro-proxy binary in the release tarball" if asset.nil?
    bin.install asset => "kotro-proxy"
  end

  test do
    # Binary --version tracks the crate (may differ from formula/tag, e.g. 1.0.0 vs 0.5.2)
    assert_match(/kotro-proxy/, shell_output("#{bin}/kotro-proxy --version"))
  end
end
