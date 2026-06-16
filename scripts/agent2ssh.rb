class Agent2ssh < Formula
  desc "SSH capability layer for general-purpose agents"
  homepage "https://github.com/lengyuqu/agent2ssh"
  version "0.1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-aarch64-apple-darwin.tar.gz"
      sha256 "aa675e11e2eaf5dfe19f92a2dc6715dd624b9d712ec7317850cade87158391ad"
    else
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-apple-darwin.tar.gz"
      sha256 "052c8e42be19a4980749bcbf166873d6326dff17d5ee34c036a6cdb7d5e8d9b1"
    end
  end

  on_linux do
    url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "3b4d9a72c573ada3edb0070aa8b56fbe8a384f737d19b36c2868c59c094d78e0"
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
