- The `secrets` bundle (included by `gaze setup`) protects credentials named
  in prose: `password.prose` for passwords and passphrases given after an EN/DE
  password cue, `security_token.prose` for security codes, tokens and API keys
  behind an explicit security cue, and `security_token.pem_base64` for
  base64-encoded private keys such as Kubernetes `tls.key` values, wrapped,
  indented or JSON-escaped. Credential metadata (names, hints, policies,
  lengths), card verification digits, encoded public certificates, and code
  and agent text beside a credential word (model names, versions, file names,
  identifiers, paths, quoted UI strings) stay raw.
- `driver_license.cue_anchored` also protects licence numbers with a second
  letter group after the first, such as `B-XY123456` and `NZDL1234567`, behind
  a driver cue; the generic `license number` cue keeps one letter group.
