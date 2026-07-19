class KotroProxy < Formula
  desc "Local security and efficiency layer for MCP-native agentic AI — injection scanning, secret redaction, semantic cache, agent loop protection"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.5.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.5.2/kotro-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "51bd7c1e5e10869b0d82dff2ae5e871f439f19085adbbc669d3b8fc472b2586a"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.5.2/kotro-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "c69a2383b261760ab66bb08a6faebc75de708ae4b6367af4571df0e2de77bbe0"
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
