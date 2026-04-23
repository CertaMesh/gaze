class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.3.0-rc.3"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.3.0-rc.3/gaze-aarch64-apple-darwin"
      sha256 "baa7edb79d84fea5d74377f82877c5069d861381a9f6012aa55af2264a8287f4"
    end
  end

  def install
    bin.install Dir["gaze-*"].first => "gaze"
  end

  test do
    assert_match "gaze 0.3.0-rc.3", shell_output("#{bin}/gaze --version")
  end
end
