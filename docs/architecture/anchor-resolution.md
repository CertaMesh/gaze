# Mandatory Anchor Resolution

Mandatory anchors are a fail-closed guard for structural recognizers whose shape
alone is not enough to safely emit a precise variant token. A recognizer declares
the requirement in its collision metadata:

```toml
[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
mandatory_anchor = "iban"
```

Locale rulepacks provide cue bundles under `[locale.cues.<key>]`:

```toml
[locale.cues.iban]
names = ["IBAN", "IBAN:", "Account No."]
window_chars = 64
```

At runtime, Gaze runs validator veto first, then anchor resolution, then normal
conflict resolution. `AnchorResolver` looks up the candidate's recognizer id in
`FamilyPolicyTable`; when `mandatory_anchor` is present, it scans the active
locale chain for a matching cue bundle and searches a bounded window around the
candidate span. Missing cue bundles are treated as missing anchors, not as
permission to emit the narrower variant.

When a mandatory anchor is found, the candidate flows normally and can emit its
variant class, for example `custom:iban`. When the anchor is missing, Gaze emits
one family-level token with class `PiiClass::Custom("family:<family>")`, marks
the decision as `ConflictTier::AnchoredContext`, and attaches an
`AmbiguityRecord` with `AmbiguityReason::NoAnchor`.

This is HYBRID output, not multiple redactions. The cleaned text receives one
token and the manifest keeps one restore mapping. Audit receives the redaction
entry plus the ambiguity sidecar so adopters can tune cues without weakening
restore semantics.

The bundled coherence gate:

```bash
cargo run -p xtask -- locale-cue-bundle-coherence
```

fails if a bundled recognizer declares `mandatory_anchor` without at least one
bundled locale cue block for that key.
