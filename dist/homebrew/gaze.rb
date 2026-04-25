class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.4.2"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.4.2/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "cf02314d77cff2b6a66c08c46789ec5231df62bfde0d036de26e2bf7d4b530ad"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.4.2", shell_output("#{bin}/gaze --version")
  end
end
