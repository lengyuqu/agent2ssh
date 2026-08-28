class Agent2ssh < Formula
  desc "SSH capability layer for general-purpose agents"
  homepage "https://github.com/lengyuqu/agent2ssh"
  version "0.3.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-aarch64-apple-darwin.tar.gz"
      sha256 "c5a6ab3d192f5171fe8522a6c063b7737c393ebe1f92114e475dcf9f3f0c2a41"
    else
      url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-apple-darwin.tar.gz"
      sha256 "8bfd6be7063c459e055caa3721944200384112adc3af784f6f8e006d9f3907c0"
    end
  end

  on_linux do
    url "https://github.com/lengyuqu/agent2ssh/releases/download/v#{version}/agent2ssh-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "e10fa4310a4dc772c967b318ab26c4fab3ba85932244563fc992bb392fe1af89"
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
