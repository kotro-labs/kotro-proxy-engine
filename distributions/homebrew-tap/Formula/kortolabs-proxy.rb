class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/kotro-labs/kotro-proxy-engine"
  version "0.2.7"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.2.7/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "730f0dec5d44148422f6d43d3938f14ada5bf08470c85aeb2a373bf7c73fdefd"
    else
      url "https://github.com/kotro-labs/kotro-proxy-engine/releases/download/v0.2.7/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "8ad608e3a53170828dce7e4f9a120a691a976d73e7c4aaa4b05dea0bfcb658e2"
    end
  end

  def install
    asset = Dir["korto-proxy-*"].first
    odie "Expected exactly one korto-proxy binary in the release tarball" if asset.nil?
    bin.install asset => "kortolabs-proxy"
  end

  test do
    assert_match "korto-proxy #{version}", shell_output("#{bin}/kortolabs-proxy --version")
  end
end
