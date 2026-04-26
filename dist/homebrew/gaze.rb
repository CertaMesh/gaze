class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/Naoray/gaze"
  version "0.4.3"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Naoray/gaze/releases/download/v0.4.3/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "df9ae86153e3b55f676bcfa39acd1867c786a8096f8ce0363d6d614bf950b6df"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.4.3", shell_output("#{bin}/gaze --version")
  end
end
