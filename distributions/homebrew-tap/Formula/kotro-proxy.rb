class KotroProxy < Formula
  desc "Local LLM streaming proxy with semantic SSE cache, PII redaction, and context compression"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.3.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.3.0/kotro-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_AARCH64_APPLE_DARWIN"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.3.0/kotro-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_X86_64_APPLE_DARWIN"
    end
  end

  def install
    asset = Dir["kotro-proxy-*"].first
    odie "Expected exactly one kotro-proxy binary in the release tarball" if asset.nil?
    bin.install asset => "kotro-proxy"
  end

  test do
    assert_match "kotro-proxy #{version}", shell_output("#{bin}/kotro-proxy --version")
  end
end
