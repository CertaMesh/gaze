class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/piinuts/gaze"
  version "0.4.5"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/piinuts/gaze/releases/download/v0.4.5/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "c739869f3d4d21936872fc92785dbd5be2e794d9bed9e4684805c1f0da054b4f"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.4.5", shell_output("#{bin}/gaze --version")
  end
end
