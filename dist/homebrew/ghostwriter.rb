# frozen_string_literal: true

# Ghostwriter — deterministic text sanitization + exact restoration for LLM prompts.
# Source-build variant. Once cargo-dist is wired up, replace this with
# prebuilt binaries per the scribe.rb pattern.
class Ghostwriter < Formula
  desc "Deterministic PII sanitization + exact restoration for LLM prompts"
  homepage "https://github.com/naoray/gaze"
  url "https://github.com/naoray/gaze/archive/refs/heads/ghostwriter-v0.1.tar.gz"
  version "0.1.0"
  license "Apache-2.0"
  head "https://github.com/naoray/gaze.git", branch: "ghostwriter-v0.1"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/ghostwriter")
  end

  test do
    assert_match "ghostwriter 0.1.0", shell_output("#{bin}/ghostwriter --version")
    out = pipe_output(
      "#{bin}/ghostwriter sanitize",
      '{"text":"Hi Markus","context":{"customer_name":"Markus"}}'
    )
    assert_match "<CUSTOMER_NAME>", out
  end
end
