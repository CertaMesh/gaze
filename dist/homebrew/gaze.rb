class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.4.4"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.4.4/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "e96d23685719bf1b5364b83b64c37733a2c2d7e6c086fe066528b30c5c3d0d0e"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.4.4", shell_output("#{bin}/gaze --version")
  end
end
