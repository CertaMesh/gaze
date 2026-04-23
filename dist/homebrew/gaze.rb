class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.3.0-rc.1"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.3.0-rc.1/gaze-aarch64-apple-darwin"
      sha256 "PLACEHOLDER_ARM64_SHA"
    end
    on_intel do
      url "https://github.com/Naoray/gaze/releases/download/v0.3.0-rc.1/gaze-x86_64-apple-darwin"
      sha256 "PLACEHOLDER_X86_64_SHA"
    end
  end

  def install
    bin.install Dir["gaze-*"].first => "gaze"
  end

  test do
    assert_match "gaze 0.3.0-rc.1", shell_output("#{bin}/gaze --version")
  end
end
