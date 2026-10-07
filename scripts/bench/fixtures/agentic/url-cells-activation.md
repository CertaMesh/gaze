# Inactive URL benchmark cells

`scripts/bench/url_cells.py` prepares additive A/D/R records. These cells were
prepared against the committed generator-v9 label contract, which has no `URL`
ruling and deliberately fails closed on their URL gold. The active
`agentic_layers.generate()` does not import this module. No shipping generator
version is allocated by it.

The composite source also contains the public generator-v10 profile, which
explicitly scores `URL` for its own URL population. The source controls retain
the historical v9 rejection and separately check that applying the current
profile preserves this module's complete original and extended URL gold.
Those application checks do not accept the public profile's final allocation,
runtime, gain or history evidence. Root allocation after the pending tax and
phone additions and the activation checklist below remain required.

The module emits 760 distinct source documents per partition: 520 A positives,
168 D counterweights, and 72 R repeats. Its seeded dev/test source templates,
record IDs, groups, source texts and gold values are disjoint. All domains are
reserved `.invalid` examples. The source-byte gold is recorded while inserting
raw values, before any detection or decoding. Plain and slash-escaped repeats
are separate raw evidence; no URL normalizer or shared token identity is assumed.

The six surfaces are compact JSON, HTML double and single attributes, Markdown,
prose and log fields. Values cover HTTP/HTTPS and case, scheme-only and path-only
slash escaping, mixed scheme slashes, `www.`, host-only values, terminal escaped
slashes, query/fragment, apostrophes and percent-encoded delimiters. Reference
URLs remain A gold under the existing URL contract. Surrounding quotes, keys,
numbers, punctuation and markup are non-gold, so over-spans cost false-positive
bytes. D prices bare hosts and paths (plain and escaped), incomplete schemes and
`www` prefixes, filenames, decimals and versions on each surface. R has exact
and mixed-spelling repeats plus bare-path near misses.

Literal apostrophes are not put inside single-quoted HTML attributes, where
ownership is ambiguous. They remain gold on other surfaces; percent-encoded
apostrophes have valid single-quoted coverage. URL preserve with Email tokenize
and residual behavior, manifest identity, export/import and strict restore stay
in the native URL tests. These Python cells do not prove runtime behavior.

The frozen 760-row population and both original hash pins remain unchanged.
An additional inactive entry point, `url_cells.generate_extended(partition)`,
appends 828 distinct serialization records: A292, D280, R256, for 1,588 total.
`url_cells.generate()` still returns the original population. Neither entry point
is called by the active agentic generator.

The addition covers escaped ASCII account characters at multiple positions,
non-ASCII BMP units, valid surrogate-pair spellings, mixed literal Unicode,
escaped path/query separators and structural data, terminal Unicode-escaped
slashes, and all 16 combinations of literal, slash-escaped and either-case
Unicode-escaped scheme slashes. Source values vary by seeded account and index.
Reference hosts stay positive gold. D uses unanchored counterparts and malformed
scheme units; R repeats the complete raw spelling with a recorded bare near miss.
Malformed tails are explicitly raw-text precision fixtures: the valid anchored
prefix is gold, the unsupported tail is a decoy. They are not valid JSON claims.

Single-quoted self-closing `img` and `link` records separately price the closing
quote and slash as non-gold. They expose an inherited runtime overspan; the
serialization repair does not claim general HTML boundary proof or repair it.
The inherited balanced-parenthesis path limitation also remains unresolved.

## Activation checklist

1. Allocate the final generator version after the pending tax and phone
   additions. Preserve every frozen historical record byte-for-byte. Import and
   append `url_cells.generate_extended(partition)` only in that reviewed integration.
   Import the module inside `agentic_layers.generate()` after initialization;
   a top-level import would create a cycle through the shared Record types.
   Register the allocated version with surface prefix `url_` in
   `GENERATOR_ADDITIONS` so old-version filters exclude these new cells.
2. Add the existing layer-C `URL` label to that version's explicit agentic
   contract. Score the entire original serialized URL range. No credentials
   relabeling, scorer/schema change, checksum credit or counterweight exemption
   belongs to this addition. Regenerate corpus pins and version provenance.
3. Review coverage independently. Run the model-free tests with the locked bench
   environment, or use Python 3.11, 3.12 or 3.13:

   ```sh
   python3 -m unittest discover -s scripts/bench -p 'test_url*.py' -v
   ```

4. Root must authorize the separate fresh runtime and benchmark runs. Measure
   both sides on the same final pinned harness and each side's own setup policy,
   under both gate contracts. Re-measure every displayed past release using
   this final harness against that tag's detection code, or document the exact
   reason a row is unavailable. Existing release evidence grants no exemption.

Until those steps finish, these committed cells are preparation only, with no
runtime, gain-gate, history or shipping credit.
