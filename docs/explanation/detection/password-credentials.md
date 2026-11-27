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
Complete JSON separator escapes establish both hash boundaries. A hash after
`heading\n` or another hash followed by `\n` keeps its own complete span.
An incomplete or malformed escape cannot establish a hash boundary. These are
supported source escapes, not a general source-language decoder. Other escapes are outside this
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
855,588 leaked bytes are removed for 2,874 additional false-positive bytes
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
every v23 document. Generator v25 adds hashes after serialized line prefixes and
two distinct consecutive hashes repeated twice inside one JSON string. Complete separator
escapes work on both sides; malformed prefixes and identifier-glued prefixes
remain benign. Every v24 document reconstructs byte-for-byte. The 120 compact and
nested PHP rows per partition share expression shapes, so these measurements
do not establish generalization to unseen PHP syntax.

Fresh base/candidate and isolated-mechanism measurements pass the v2 and v1
gain gates and the current v4 headline contract. Each side's single observation
record supplies every contract by re-scoring. No independent contract capture
is needed. All 26,950 documents per
side restore exactly and have valid manifests; none is refused. The
[evidence report](../../reference/benchmarks/password-code-credentials/json-prefixes/README.md)
records per-layer leaked and false-positive bytes, source and binary identities,
commands and limits. Historical release backfill is a separate job: published
history remains bound to generator v20, while this measurement uses v25.
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
observation archives. Historical release rows are not re-labelled as v25.

## Credentials named in prose

Chat, email and tool-call text often gives a credential in a sentence:
`I reset my password to …`, `Das Passwort für das Portal lautet „…“`,
`Ihr Sicherheitstoken, der Code …, wurde bestätigt`. Three `secrets` rules
cover these, and the private keys that deployment files carry base64-encoded.

`password.prose` starts at a password, passphrase, Passwort or Kennwort cue,
including German compounds such as `WLAN-Passwort`. The value follows the cue
after `:` or `=`, or after a copula or preposition (`is`, `was`, `to`, `as`,
`from`, `ist`, `lautet`, `sei`, `auf`, `zu`, `von`) that may come behind up to
eight connector words; there a quoted value may be any phrase of six or more
characters, so `The passphrase is "correct horse battery staple"` is protected.
Directly after the cue, or quoted behind connector words alone, the value needs
a digit or a symbol. An unquoted value always needs a digit or a symbol other
than word punctuation, so `password to log in` and `password reset-link` are
not credentials; `_` does not count, so identifiers such as `DB_PASSWORD` or
`bcrypt_hash` are not values either. Quotes and brackets inside a value stay
in it (`Fj0rd'Tal99`, `gr8-Pa55w0rd(old)`), but a quote, comma, colon or
semicolon followed by anything other than another value character ends it,
so a password at the end of a JSON or code string (`…Tal99"}`, `…Tal99";`)
never takes the closing quote or the next key. JSON-escaped quotes
(`\"…\"`) delimit a value like plain ones. A trailing `!` stays in the value;
sentence punctuation does not. The context guard refuses values under six
characters, one or two plain words, symbol-only placeholders such as
`********`, URLs, email addresses, paths without digits, Windows paths,
environment references (`${NAME}` or `$ALL_CAPS`; a `$`-led password such as
`$Tal99abc` stays protected), ALL_CAPS identifiers, hyphenated adjectives
(`2fa-enabled`), and quoted UI text: a quoted phrase with a function word and
no digit (`"Please enter it again"`) or one that names the password itself
(`"Incorrect password, try again"`). It also refuses matches whose
cue part names credential metadata (name, label, hint, policy, length,
rotation, template, field, hash, file, path, model, version, region, endpoint,
table, service, commit, column, and UI nouns such as error, message, prompt,
subject, title, text, button, placeholder, banner and dialog, with their German
equivalents): a password's hint or
policy is not the password. Metadata words inside a quoted value do not count.

`security_token.prose` starts at an explicit security cue: German
`Sicherheits-`, `Zugangs-`, `Anmelde-`, `Verifizierungs-` and
`Bestätigungs-` tokens, codes and keys, English security, access, auth,
verification, session, API, bearer and one-time tokens, codes and keys, and
`Token-ID`. The value follows the cue directly, or after `:`, `=`, `,` or a
copula (`is`, `ist`, `lautet`, `der Code`, …) that may come behind up to eight
connector words. Connector words alone never introduce a value, so in
`Rotate the API key before deploying release v2.14.0-rc1` the version stays
text. The value has at least eight characters and both a letter and a digit,
or is an all-digit one-time code of six to eight digits. Three- and four-digit
card security codes stay raw here. The same metadata guard applies, and
versions, model names (`gpt-4o-mini`, `claude-3-opus`), CVE identifiers, file
names, numbered snake_case identifiers (`auth_service_v2`) and hyphenated
adjectives (`SHA256-basiert`) are refused. The bare word `token` is not a cue, because agent text uses it for
model tokens and tokenizer names; a value introduced only by `token` therefore
stays raw unless the anchored rule owns it.

`security_token.pem_base64` finds the base64 spelling of a private-key
`-----BEGIN … PRIVATE KEY-----` line at each of the three byte alignments and
protects the whole contiguous base64 run, so `tls.key:` values in Kubernetes
secrets, `KEY_B64=` environment lines and keys inside a larger encoded JSON
file are one restorable span. Wrapped `base64` output continues through lines
of at least 16 characters and a final padded line, including indented YAML
block scalars and JSON-escaped `\n` and `\r\n` line breaks. A line that is a YAML
key (it ends at a colon) never continues the run. A 16-character or longer word
directly after an escaped line break inside the same JSON string does, and is
then protected with the key; an unpadded short final line
stays raw, and it encodes only the public END armour. Encoded certificates and
public keys stay raw.

Known residuals: a value placed before its cue (`I entered … as the password`),
a value after `like`/`wie` without quotes, an unquoted spaced passphrase, a
value that ends in `?`, whose last character is read as sentence punctuation, a
value that starts with a bracket or ends with an unpaired closing bracket, a
value in a parenthesised aside, and a security value behind connector words
without a copula (`Sicherheitscode für den Zugriff auf unser System …`).
The older `security_token.anchored` rule still tokenizes a hyphenated adjective
directly after `API key` (`base64-encoded`). Grouped one-time codes (`482 913`, `4829-13`) and spaced tokens
(`"k3j9 ww2p x7a9"`) stay raw; a later change owns them together with card
security codes.
Generator v31 measures these phrasings with the rest; see the
[benchmark contract](../../reference/benchmarks/README.md).
