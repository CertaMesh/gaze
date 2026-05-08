class Gaze < Formula
  desc "Channel-agnostic PII redaction CLI for AI pipelines"
  homepage "https://github.com/EmpireTwo/gaze"
  version "0.6.4"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/EmpireTwo/gaze/releases/download/v0.6.4/gaze-aarch64-apple-darwin",
          using: :nounzip
      sha256 "dad01b196eeea445291e7c47ddecf61d1143c3a3e4a5b731fc8b647dfc884a07"
    end
  end

  def install
    bin.install "gaze-aarch64-apple-darwin" => "gaze"
    chmod 0755, bin/"gaze"
  end

  test do
    assert_match "gaze 0.6.4", shell_output("#{bin}/gaze --version")
  end
end
