class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.4.1"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.4.1/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "dab8d21297c7e7de1df97dc371291f2abb3a2a66f1a234d8522aaf777d847c39"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.4.1", shell_output("#{bin}/gaze --version")
  end
end
