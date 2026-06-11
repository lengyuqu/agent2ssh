class Agent2ssh < Formula
  desc "SSH capability layer for general-purpose agents"
  homepage "https://github.com/lengyuqu/agent2ssh"
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER"
    else
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER"
    end
  end

  on_linux do
    url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "SHA256_PLACEHOLDER"
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
