class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.4.0-rc.1"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.4.0-rc.1/gaze-v0.4.0-rc.1-aarch64-apple-darwin.tar.gz"
      sha256 "4ccdce9cccd3c9777fb6983de3d0c884568f7147a98cde01fafc298013df592b"
    end
  end

  def install
    bin.install "gaze"
  end

  test do
    assert_match "gaze 0.4.0-rc.1", shell_output("#{bin}/gaze --version")
  end
end
