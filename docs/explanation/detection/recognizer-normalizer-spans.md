# Recognizer normalizers preserve the original span

## Original-span-preserved invariant

Normalizers may strip spaces, dashes, or full-width variants for validators,
checksums, and parsers. They must not change the original byte span emitted to
the manifest. Store the exact matched input bytes so session restore returns
the owner-side text byte for byte.
