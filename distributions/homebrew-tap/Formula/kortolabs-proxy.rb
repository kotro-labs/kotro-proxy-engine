class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.0/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "33851405962c2e0bd6223f3773c08bec80db690f487bdabedf87b11d140c74a5"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.0/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "358066d812f35d6957f9de462a3b2119d6f419de4f8f6ace7b04cb2c24b45a21"
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
