class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/EmpireTwo/gaze"
  version "0.5.1"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/EmpireTwo/gaze/releases/download/v0.5.1/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "161c43e50d838935aeefb02c764240792942e5226e63a18207294a02d4262671"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.5.1", shell_output("#{bin}/gaze --version")
  end
end
