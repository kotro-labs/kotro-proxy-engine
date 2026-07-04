class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.0/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "81e7ae10806fc996a93f0a36b525cddf6cdfc58d420be139bad23ad7bf4d1986"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.0/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "36806083949fa243359483882f6370e5fec6e325f63997c8d6efdf62c98ca3f7"
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
