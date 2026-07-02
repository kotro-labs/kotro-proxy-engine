class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.2.3"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.3/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "ce0429b294117ab3a2bb50515acef5bc35127d66fbac191a7b8184eedc8e2489"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.3/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "1bac754f4cb2ba4a2e4f68995c35dacd261afbdb3c690e1a24d8a15443b07cce"
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
