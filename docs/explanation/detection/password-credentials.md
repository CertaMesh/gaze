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
its 1,024-byte limit. Django salts use alphanumeric characters, dot, slash,
underscore, or hyphen. Adjacent encoding characters block partial matches. A preceding `=` remains
an assignment delimiter, so `digest=$...` values are protected.

`password.code` and `security_token.code` require a complete, nonempty quoted
literal on one line, after an exact credential key and `:`, `=`, or `=>`.
Matching quotes and the following comma, closing bracket or brace, semicolon,
or line end establish the value boundary. Source escapes remain escaped in
the manifest. Matching uses normalized text, as `password.field` does. Removed
edge joiners remain outside the capture. Code literals have no length cutoff. Bare password keys
require a trailing comma, semicolon, brace, or bracket. Other bare line records
stay with `password.field`, including its existing length and audit contract. Expressions such as
`Hash::make('secret')` do not establish a credential value.

The labelled-identifier scanner is unsuitable here. Its group and next-label
boundaries describe identifiers, while a quoted secret can contain arbitrary
spaces, punctuation, and words that resemble labels. The closing quote owns
the boundary. `password.field` retains its existing line-record grammar.

SQL positions do not establish ownership of arbitrary strings. A recognizable
password hash is protected anywhere in a SQL dump. Arbitrary plaintext SQL
values need a schema-aware caller or a separate ingestion contract.

The generated benchmark adds whole hashes, code literals and repeats, with
incomplete encodings, dollar prices, unrelated keys and expressions as benign
counterweights. Fresh v2 and v1 gain-gate measurements, mechanism measurements,
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
