class Agent2ssh < Formula
  desc "SSH capability layer for general-purpose agents"
  homepage "https://github.com/lengyuqu/agent2ssh"
  version "0.5.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-aarch64-apple-darwin.tar.gz"
      sha256 "2250fa22c2a2c0c73d2ba9196f18cfd29885e1bdb55d1de06bbe4f9b263656a7"
    else
      # Intel macOS support was dropped, so no x86_64-apple-darwin archive is
      # published any more. Fail loudly here rather than handing Homebrew a URL
      # that 404s, which would surface as an opaque download error.
      odie "agent2ssh no longer ships a prebuilt binary for Intel Macs. " \
           "Build it from source with `cargo build --release`, or use an Apple Silicon Mac."
    end
  end

  on_linux do
    url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "9603fc173cb1407058ccc4ce0ac125667bba4f6895315cad4764169ae73d124a"
  end

  def install
    bin.install "agent2ssh"
    bin.install "agent2ssh-mcp"
    bin.install "agent2ssh-daemon"
  end

  test do
    assert_match "SSH capability layer", shell_output("#{bin}/agent2ssh --help")
  end
end
