class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.1.1/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "25cb4b33eadf877daee4e31373e31838a554c0d1dba5fc9b10125777e9794c98"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.1.1/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "867f772b56d0c52beda990495ef24542c55496c8a0760ff56cdeac92a954debe"
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
