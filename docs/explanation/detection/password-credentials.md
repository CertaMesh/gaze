# Password hashes and code credentials

The `secrets` bundle protects stored password encodings and literal credential
fields. `gaze setup` already includes this bundle. Keeping these recognizers in
`secrets` gives setup adopters the protection while preserving explicit
core-only library policies.

`password.hash` captures the complete encoding without a field cue. The
existing resolver handles containment of inner matches, including the postal
code inside Argon2's memory parameter. No Argon2-specific suppression changes
postal detection. Strict restore retains every source byte.

The formats follow [libxcrypt crypt(5)](https://github.com/besser82/libxcrypt/blob/develop/doc/crypt.5),
[Passlib scrypt](https://passlib.readthedocs.io/en/stable/lib/passlib.hash.scrypt.html),
the [PHC format](https://github.com/P-H-C/phc-string-format/blob/master/phc-sf-spec.md),
and [Django hashers](https://github.com/django/django/blob/main/django/contrib/auth/hashers.py).
Matching establishes a stored-credential shape, not cryptographic validity.
Fixed-width checksums require their complete encoded length. Variable-length
Argon2 salts and digests require at least four base64 characters each, with no
upper cutoff. Passlib scrypt salts allow up to 1,366 base64 characters, matching
its 1,024-byte limit. SHA-crypt and MD5-crypt salt components accept characters
other than dollar, colon, newline and NUL, following libxcrypt's parser.
The matcher bounds these salts at 16 and 8 decoded characters respectively.
SQL doubled quotes (`''`) and MySQL backslash quotes (`\'`) count as one
character. JSON escaped slashes (`\/`) count as one alphabet character in
hash components, including fixed-width checksums. The full serialized source
is one manifest span; strict restore returns the original escape spelling.
A trailing backslash cannot terminate a hash match. These are supported source
escapes, not a general source-language decoder. Other escapes are outside this
shape contract. Libxcrypt truncates at that many bytes. A Unicode salt over the producer's byte limit
can therefore still match the stored-encoding shape. Salt and checksum
alphabets are distinct; checksums retain exact crypt-base64 widths. Django salts are nonempty and exclude
only dollar, as required by its hasher API; plus, at-sign, spaces and Unicode
are accepted. Adjacent encoding characters block partial matches. A preceding
`=` remains an assignment delimiter, so `digest=$...` values are protected.

`password.code` and `security_token.code` require a complete, nonempty quoted
literal on one line, after an exact credential key and `:`, `=`, or `=>`.
Matching quotes and the following comma, closing parenthesis, bracket or brace, semicolon,
or line end establish the value boundary. Source escapes remain escaped in
the manifest. Matching uses normalized text, as `password.field` does. Removed
edge joiners remain outside the capture. Code literals have no length cutoff. Bare password keys
require a trailing comma, semicolon, parenthesis, brace, or bracket. Other bare line records
stay with `password.field`, including its existing length and audit contract. Compact and nested PHP `array(...)` syntax is supported. Expressions such as
`Hash::make('secret')` do not establish a credential value.

The code token keys are `api_token`, `remember_token`, `security_token`,
`access_token`, `auth_token`, `authorization_token` and `api_key`. Generic
`token` is excluded because logprob output, tokenizer vocabulary and schema
examples use it for ordinary text. This costs recall: a non-JWT secret stored
under a quoted generic `token` key may remain clear. The existing anchored
token recognizer and JWT detection retain their own contracts; they do not
guarantee protection of every such literal.

Password keys remain fail-closed. Validation rules such as
`required|string|min:8|confirmed`, translation messages, schema exemplars
such as `string`, and masked `********` values can be tokenized as passwords.
This is a known false-positive class: agents lose useful code or documentation
content even though restore works. Spaces, pipes and these exact values can
also be real passwords, so excluding them globally would create leaks.
Context-aware metadata exclusions require a separate design. The benchmark
keeps complete benign contexts as layer D counterweights and actual spaced,
pipe-containing and weak passwords as layer A/R gold. The completed gain gate
retains these costs: layer D adds 930 false-positive bytes, layer A adds 1,950
and layer R removes 6. The A/R change is Nym labelling the SQL keyword `VALUES`
as a name; this can hide SQL syntax from an agent until restoration. The D
change comes from the new password-code recognizer. Across all layers,
442,135 leaked bytes are removed for 2,874 additional false-positive bytes
under both required contracts.

The labelled-identifier scanner is unsuitable here. Its group and next-label
boundaries describe identifiers, while a quoted secret can contain arbitrary
spaces, punctuation, and words that resemble labels. The closing quote owns
the boundary. `password.field` retains its existing line-record grammar.

SQL positions do not establish ownership of arbitrary strings. A recognizable
password hash is protected anywhere in a SQL dump. Arbitrary plaintext SQL
values need a schema-aware caller or a separate ingestion contract.

The generated benchmark adds whole hashes, code literals and repeats, with
incomplete encodings, dollar prices, unrelated keys, expressions and full
logprob/tokenizer/schema/validation/language/masked contexts as benign
counterweights. Alternative salts and PHP arrays are included. Generator v22
preserves every v21 document byte-for-byte. Its added contexts use separate
wording, structures and seeded value pools in each partition; tests reject
shared rendered text or value-normalised templates. Generator v23 adds SQL
quote escapes at the salt limit, JSON escaped slashes and malformed/over-limit
counterweights while preserving every v22 document. Generator v24 adds complete
literal and escaped-slash hashes immediately followed by JSON newline, tab, CR,
quote, backslash and Unicode separator escapes. Exact source spans restore
without consuming those separators; malformed tails remain benign. It preserves
every v23 document. The 120 compact and
nested PHP rows per partition share expression shapes, so these measurements
do not establish generalization to unseen PHP syntax.

Fresh base/candidate and isolated-mechanism measurements pass the v2 and v1
gain gates and the current v4 headline contract. Each side's single observation
record supplies every contract by re-scoring. No independent contract capture
is needed. All 25,270 documents per
side restore exactly and have valid manifests; none is refused. The
[evidence report](../../reference/benchmarks/password-code-credentials/json-separators/README.md)
records per-layer leaked and false-positive bytes, source and binary identities,
commands and limits. Historical release backfill is a separate job: published
history remains bound to generator v20, while this measurement uses v24.
No latency or throughput result is claimed.

The mechanism preparation command produces two policies for the same binary:

```bash
uv run --project scripts/bench python scripts/bench/credential_code_arm.py \
  --setup-policy target/gate/policy.toml --output-dir target/credential-code-arm
```

Both retain the same prior secrets definitions and all other setup settings.
The candidate appends the new hash and code-field definitions. Use the emitted
`delta.toml` with `agentic_layers.py gate --policy-delta` and
`mechanism_arms.py record --policy-delta`. The emitted paths are local artifacts.
The committed evidence records the policy delta and retains both value-free
observation archives. Historical release rows are not re-labelled as v24.
