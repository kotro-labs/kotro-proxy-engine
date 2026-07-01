class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.1.0/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "39f7934181d0aea6992efdf56a0bf22408803d44ed7bd71a8d10378a9f851bc3"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.1.0/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "3dd2891a8cd6c9a0ee54250d8152aa673d9c40e7a9518d59cfd907c33675f13c"
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
