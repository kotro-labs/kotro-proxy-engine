class KortolabsProxy < Formula
  desc "Statically linked edge AI streaming proxy gateway with Zstd cache compression"
  homepage "https://github.com/ramairwing/kotro-proxy-engine"
  version "0.2.5"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.5/korto-proxy-aarch64-apple-darwin.tar.gz"
      sha256 "7202690a258df98d61dc4e924602490a0eed13333b4f369551b64f16472ec132"
    else
      url "https://github.com/ramairwing/kotro-proxy-engine/releases/download/v0.2.5/korto-proxy-x86_64-apple-darwin.tar.gz"
      sha256 "65d482241596cf04a5f526402ee3a20aa70a4170e89f1ef020d9b5763fc33907"
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
