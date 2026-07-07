class KotroProxy < Formula
  desc "Local LLM streaming proxy with semantic SSE cache, PII redaction, and context compression"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.3.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.3.0/kotro-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "aaa6874552fe02e07ac1bfeb6e44a3a4b419c16180e2edc6588d5fae457dc027"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.3.0/kotro-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "738f4e5a735e786b89890266239f94e59f5f61c362a6232a51a72ac5d69a76be"
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
