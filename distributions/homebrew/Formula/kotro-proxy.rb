class KotroProxy < Formula
  desc "Local security and efficiency layer for MCP-native agentic AI — injection scanning, secret redaction, semantic cache, agent loop protection"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.6.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.6.2/kotro-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "987b69f89c07bdcf76e1c37b160c82f43548d1d83321a429f560a8cf90519793"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.6.2/kotro-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "754eb8c69d4253ae2233be078923ceff05b2522165eafeb219a649138bf92e4b"
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
