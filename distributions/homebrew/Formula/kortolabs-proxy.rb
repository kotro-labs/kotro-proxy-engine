class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.1.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.1.2/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "3d24b1b4f37b263adc11827a599e9a7a217414ab5bd2be4f1e0e60ead37b664e"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.1.2/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "c1f8e7700e049a18203de39818d2232085e7caed47605bbe113d0c09a85d2e2f"
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
