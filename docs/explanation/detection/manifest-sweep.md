# Repeat-value sweep

Once Gaze tokenizes a value that a rule found, every other copy of that value
in the same document, and in later documents of the same session, is
tokenized too. Before the sweep a copy was protected only when a recognizer
fired at that exact spot, so a name caught in an email header shipped raw in
the body, in a different case, or in the next turn (solo todo 3849).

```text
input   From: Maria Schneider <maria.schneider@example.invalid>
        hi, this is maria schneider again. Thanks, Maria
before  From: <a13e:Name_1> <<a13e:Email_1>>
        hi, this is maria schneider again. Thanks, Maria
after   From: <a13e:Name_1> <<a13e:Email_1>>
        hi, this is <a13e:Name_2> again. Thanks, <a13e:Name_3>
```

## Where it runs

After resolve and before the safety net. The sweep collects its values from
two places: this document's resolved winners and the session manifest. It
looks for copies that no winner covers. Each copy joins the candidate pool,
and the pool is resolved again, so the resolver's containment and priority
rules settle any overlap. A copy that encloses a weaker same-class span, such as an NER
fragment that left `<Name_1>a`, wins and covers the whole copy. A document
with no uncovered copy keeps its first resolution; the only extra cost is one
linear scan.

**Order matters: a copy is swept only after its source was seen.** Within
one text, all rule-found values of that text are sources for every copy in
it, before or after. Across texts, a copy is swept only when its source was
tokenized in an earlier text of the session. The daemon cleans one request
per turn, so a later turn sees every earlier turn. The proxy protects each
JSON string field of a request in order (message contents, tool results,
tool arguments), each as its own text. **Stated gap:** a copy in an earlier
field than its source leaks. Example: a user message `summarise maria
schneider's mail` before the tool result that holds
`From: Maria Schneider <…>`. Base behaviour is the same (not a regression); a
second proxy pass that re-protects earlier fields when the sweep grew during
the request is a tracked follow-up.

The proxy and the daemon receive whole request bodies, so no copy is cut by a
chunk boundary. A future streaming ingress must hold back
(longest value − 1) bytes before it emits.

## What propagates

Only values found by deterministic evidence: a validator, an anchored or
structural cue, or a plain regex or dictionary rule. Values found by the NER
model or by a safety net never propagate. Copying model-found values spreads
the model's mistakes across whole documents: on TAB court cases, 91 % of the
new false positives came from NER-tagged words such as "Government" and
"Court". A class the policy does not protect (`preserve`) never propagates.

Each emitter declares which kind of evidence it produces, as
`gaze::EvidenceKind::Rule` or `gaze::EvidenceKind::Learned`
(`Recognizer::evidence`, `Detector::evidence`). The registry stamps the
declaration on every candidate the recognizer emits, and the sweep and the
resolver read that field. Nothing is inferred from a recognizer id. An emitter
that declares nothing is `Learned` and never propagates, so a new model-backed
or heuristic emitter cannot start spreading its values by accident.

| Emitter | Declares |
|---|---|
| Rulepack regex, dictionary and anchored-match recognizers (bundled and adopter `[[policy.custom_recognizers]]`) | `Rule` |
| `gaze index` field-label detector, TokenBridge synthetic-corpus detector | `Rule` |
| NER (`ner`), GLiNER birth-date judge (`dob.gliner`) | `Learned` |
| House numbers licensed by a NER street (`address.house_number.street_corroborated`) | `Learned` |
| An adopter `Recognizer` / `Detector` that declares nothing | `Learned` |
| A swept copy of a rule value | `Rule` |
| A collision-family tie | `Rule` only if both sides are `Rule` |

The declaration is also the resolver's evidence tier for cross-class
containment: a `Learned` span never swallows an enclosed `Rule` candidate of
another class, so an undeclared emitter's container splits into tokens around
the rule span instead of covering it with one token. Recovery covers the
remainder, so no raw bytes ship.

Safety-net values (Nym, OPF) are not candidates. Their tokens record no
evidence in the session, so they never propagate either.

## Matching rules

Each value has one shape, and each shape owns its precision floor.

| Shape | Example | Matches |
|---|---|---|
| Multi-word, or letters mixed with digits or punctuation | `Maria Kowalski`, `DE44 5001`, `N1234567A` | Any case; any whitespace run (space, tab, line break, NBSP) matches any whitespace run. At least 4 non-space characters. |
| Digit run (digits with spaces, `-`, `.`, `/` only) | `030 1234567`, `10115` | Any whitespace run matches any whitespace run. **At least 6 digits.** A four-digit AT/CH postcode or a bare five-digit postal code is found only through the cue or city next to it; that anchor is its whole precision, so copying the bare digits would turn years and room numbers into postcodes (`2024 Neuchâtel`, then `Im Jahr 2024`). |
| One alphabetic word | `Kowalski`, `KOWALSKI`, `Albrecht-Quaye` | As written (a byte-identical copy carries the source's own evidence, so `KOWALSKI` sweeps `KOWALSKI`), in title case (`Kowalski`) and in upper case (`KOWALSKI`). At least 4 characters. |
| A part of a multi-word `Name` value | `Maria` of `Maria Kowalski` | As one word, but only in a spelling that starts upper-case (as written, title case, upper case), at least 3 letters. |
| A run of two or more adjacent parts of a multi-word `Name` value | `Tobias Brenner` of `Herr Tobias Brenner` | Like a multi-word value: any case, any whitespace run. At least one part of the run must be a name on its own (3+ letters, not on the common-word list), so `grace hall` of `Grace Hall Kowalski` is not swept. Built for values of up to six parts. |
| A collision-family value (`family:<name>`) | | Byte-exact only, at least 4 non-space characters. |

Every copy must stand on word edges under the shared
`gaze_types::is_inside_word` rule, and a copy inside a URL-shaped run
(`://` or a leading `www.`) is skipped. A `Name` copy then grows over name
parts glued to it by a hyphen or apostrophe
(`gaze_types::extend_over_name_joiners`): `jorunn vasquez-ellery` is one copy
of `Jorunn Vasquez`, and `O'Brien` one copy of the part `Brien`. Stopping at
the source's edge would ship `-ellery` raw. A forward continuation needs two
letters, so a possessive `'s` stays outside the token; a continuation that
runs into a digit is not taken. The growth does not ask whether the glued word
is a name, so `Kowalski-follow-up` becomes one token over `-follow-up` too:
fail-closed, a few bytes (review F3 on #691).

Why runs: a cue-found value keeps its cue's honorific
(`From: Herr Tobias Brenner <…>` yields `Herr Tobias Brenner`), so the bare
name `tobias brenner` or `TOBIAS BRENNER` is not a spelling of the whole
value. Before runs, only title-case parts matched, and the all-caps copy
shipped raw (solo todo 3897). A single word on a closed list of
common words that are also names is never swept: months, weekdays, English
names that are everyday words (`Will`, `Mark`, `Rose`, `May`), English
surnames that are everyday verbs or nouns (`Grant`, `Price`, `Banks`,
`Wells`), and German surnames that are everyday nouns (`Richter`, `Bauer`,
`Fischer`, `Müller`, `Schneider`, `Weber`, ...). German capitalises every
noun and English every sentence start, so title case alone cannot tell
`Der Richter hat entschieden` or `Grant access to the repo.` from a name.
The cost is recall: `Herr Richter` is not swept from a `Thomas Richter`
header. The full value `Thomas Richter` still is.

Two more closed lists make a word ordinary, so it is never a part on its own
and never the distinctive part of a run:

- **Particles and articles**, compiled per naming convention for every
  setup-policy locale and the common particle cultures: German (`von`, `zu`,
  `der`, `auf`, ...), Dutch tussenvoegsels (`van`, `de`, `het`, `op`, `ter`,
  `uit`, ...), French (`de`, `du`, `la`, `le`), Spanish and Catalan (`del`,
  `las`, `los`, `y`), Portuguese (`da`, `dos`, `e`), Italian (`di`, `della`,
  `degli`, ...), Scandinavian (`af`, `av`), Arabic and Hebrew (`al`, `bin`,
  `ibn`, `abu`, ...), Gaelic and Welsh (`mac`, `ap`, ...). The full list and
  its sources are on `PARTICLES` in `crates/gaze/src/sweep.rs`; a test
  enumerates every particle and particle pair. `Anna von der Heide` must not
  turn every German `von der`, nor `Carmen de los Rios` every `de los`, into
  a token. `von der Leyen` is still swept in any case through its distinctive
  `leyen`. `ben` and `nic` are left off: `Ben` and `Nic` are common given
  names, and a particle never sweeps alone. The cost: `VAN DER BERG` has no
  distinctive part (`berg` is a common word), so a lower-case or all-caps copy
  relies on NER.
- **Organization and role words** (`support`, `team`, `service`, `info`,
  `paket`, `kundenservice`, `noreply`, `newsletter`, also as a hyphen piece
  as in `IT-Support`).

A value shaped like an organization never sweeps single parts: a CamelCase
part (`GitHub`), or all-caps parts mixed with others (`DHL Paket`). The whole
value and its runs still sweep, so `DHL Paket` is found again but `Paket` is
not. A CamelCase single word keeps its written and title-case spellings but
gets no upper case (`GitHub` does not sweep `GITHUB`). A wholly all-caps value
(`MARIA KOWALSKI`) is a shouted personal name and keeps its parts, so a
wholly all-caps organization of unlisted words (`BLUE HARBOR`) also sweeps
its parts (`Blue`): fail-closed, accepted to keep `Thanks, Maria` protected. The cost:
a CamelCase surname (`Ronald McDonald`) sweeps as a whole and in runs, not
as `McDonald` alone.

Case folding keeps a map from every folded byte back to the character it came
from, and a copy must start and end on whole characters. Turkish `İ` lowers to
two scalars; the copy still covers exactly its own bytes. Default Unicode case
mapping lowers `I` to `i`, so a dotless `ı` in a value does not match an
upper-case `I` in the copy.

**Stated leaks.** A lone lower-case or mixed-case part (`thanks maria`,
`tOBIAS`) stays raw: matching
lower-case single words would tokenize ordinary words that happen to be
names. A surname part on the common-word list stays raw on its own. A digit
run under six digits is not swept. A copy in an earlier proxy field than its
source stays raw (see above). A value no rule found (only NER, or nothing)
seeds no sweep. An honorific plus a name made only of common words stays raw
in lower case: `From: Herr Frank Wolf <…>` then `frank wolf` has no
distinctive part, so no run, and the whole-value match needs `herr`
(same for `Mr Bill Gates`). A French-convention name with an all-caps surname
(`Jean DUPONT`) reads as organization-shaped, so `DUPONT a signé` stays raw
unless NER finds it (as before this rule).

## Token identity and restore

A token restores to exactly one byte string, so:

- A byte-identical copy reuses the source token.
- A different spelling (`maria schneider`, `MARIA SCHNEIDER`) or a name part
  gets its own sibling token of the same class and token family.

Restore stays exact for every copy.

## Session state

Each manifest entry records the evidence tier its value was found with, and
keeps the strongest tier seen. The session caches one multi-pattern matcher
over its sweepable values and rebuilds it only when a value's sweepability
changes. The value list is the manifest the session already holds; the sweep
adds no new store of raw values.

The tier travels in the `session_blob`, which moved to envelope version 6.
Readers up to v0.15 refuse a v6 blob with `InvalidSnapshotVersion(6)` rather
than drop the field. A blob of version 5 or older still imports and restores,
but none of its values seed the sweep, because their tier is unknown.

## Audit

Every swept copy writes one audit row with `recognizer_id` and
`provenance_stage` `manifest_sweep`, `decided_by: manifest_sweep`
(`ConflictTier::ManifestSweep`), and `provenance_merged_from` naming the link:
`manifest_sweep:exact`, `manifest_sweep:variant` or `manifest_sweep:part`
(a part or a run of parts).
The row never carries the source token or value, per the audit contract. The
daemon overwrites `provenance_stage` with `daemon` as it does for every row;
`decided_by` still names the sweep.

## Failure

Fail closed. When the value list passes its size cap (200,000 patterns or
16 MiB of pattern bytes) or the matcher cannot be built, the request fails
with `Error::ManifestSweep`. It never ships unswept text.

## See also

- [Session contract](../core/session-contract.md)
- [How Gaze works](../how-gaze-works.md)
- Tests: `crates/gaze/tests/manifest_sweep.rs`,
  `crates/gaze-cli/tests/manifest_sweep.rs`,
  `crates/gaze-proxy/tests/manifest_sweep.rs`
