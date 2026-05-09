# Recognizer Policy

## Original-span-preserved invariant

Recognizer normalizers MUST NOT mutate the original byte span emitted to the
manifest. Normalizers operate on the value passed to validators, checksums, and
parsers; the original span is preserved for restore.

Research-855 source line 33 is the governing rule:

> "Strip spaces, dashes, and full-width variants before checksum validation; **keep original span for redaction**. Most standards define logical value first and presentation second."

This is an axis-2 reversibility invariant. If a recognizer validates a
normalized canonical value, it must still persist the exact input bytes covered
by the match so `Session` restore can reconstruct the owner-side text
byte-for-byte.
