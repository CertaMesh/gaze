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
The matcher bounds these salts at 16 and 8 characters respectively; libxcrypt
truncates at that many bytes. A Unicode salt over the producer's byte limit
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
pipe-containing and weak passwords as layer A/R gold; the pending gain gate
must report the resulting false-positive bytes.

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
shared rendered text or value-normalised templates. Fresh v2 and v1 gain-gate
measurements, mechanism measurements,
and historical-release remeasurement remain pending until the release
benchmark machine is available. No performance or byte-gain result is claimed.

The mechanism preparation command produces two policies for the same binary:

```bash
uv run --project scripts/bench python scripts/bench/credential_code_arm.py \
  --setup-policy target/gate/policy.toml --output-dir target/credential-code-arm
```

Both retain the same prior secrets definitions and all other setup settings.
The candidate appends the new hash and code-field definitions. Use the emitted
`delta.toml` with `agentic_layers.py gate --policy-delta` and
`mechanism_arms.py record --policy-delta`. The emitted paths are local artifacts.
Measurements and historical rows must be committed before this change can merge.
