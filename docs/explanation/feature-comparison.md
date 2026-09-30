# Feature comparison: Gaze and other PII tools

This page compares **documented capabilities**, not detection quality. It uses
Gaze at this repository revision; Presidio 2.2.364; DataFog Core 0.3.0;
DataFog Python 4.8.1; scrubadub 2.0.0; GLiNER 0.2.29 with the
`urchade/gliner_multi_pii-v1` model; OpenAI Privacy Filter (OPF) 0.1.0; Perplexity
PII-Tracer at model revision `d25c16f`; and
LLM Guard 0.3.16 (its repository was [archived on 9 July
2026](https://github.com/protectai/llm-guard)). The two DataFog packages have
different APIs and contracts. A model or detector library is not a complete
owner-side restore runtime. **“Not verified” means this source review did not
establish the capability; it does not assert absence.**

Each Gaze cell links to a repository file read for this comparison. Each other
cell links to a primary source at the named version. OPF has no 0.1.0 Git tag,
so its links pin source commit `f7f00ca`; the Gaze benchmark used the same
commit. GLiNER's PII-language claim additionally uses model snapshot
`1fcf13e`.

## Restore and agent workflow

**Runtimes and agent tools**

| Capability | Gaze | Presidio | DataFog Core | DataFog Python |
| --- | --- | --- | --- | --- |
| Reversible restore and state | Owner-held, signed session snapshot and manifest; unknown tokens refused [session](core/session-contract.md), [signature](../../crates/gaze/src/session.rs), [boundary](core/restore-boundary.md) | Encrypt/decrypt operators; separate mapping-based pseudonymization **sample**, explicitly not thread-safe. No equivalent session contract verified [2.2.364][p-anon], [2.2.364][p-pseudo] | Provider-backed, scope-bound tokenization and atomic restore; provider owns storage and authorization [0.3.0][c-token] | `redact()` returns a mapping, but a public `restore()` is a **v5 plan**, not a 4.8 contract [4.8.1][y-engine], [4.8.1][y-roadmap] |
| Placeholder or token format | `<session:Class_N>` with owner manifest [walkthrough](how-gaze-works.md) | Mapping sample uses `<ENTITY_TYPE_n>`; other anonymizer operators have other outputs [2.2.364][p-pseudo] | Canonical `DFTOKENv1(...)` envelope for provider tokens; HMAC pseudonyms are a separate, non-restorable strategy [0.3.0][c-token] | Document-order `[EMAIL_1]` style redaction tokens; mapping is returned [4.8.1][y-roadmap] |
| Repeated values | Same rule-found value is swept through a document and later session turns; case or spacing variants may receive sibling tokens. Learned spans do not propagate [session](core/session-contract.md), [sweep](detection/manifest-sweep.md) | Mapping sample reuses an identifier for the same value; this is sample logic [2.2.364][p-pseudo] | Repeats are separate provider items and **may receive different tokens** [0.3.0][c-token] | Document-order numbering; same-value reuse across turns not verified [4.8.1][y-roadmap] |
| Streaming | Proxy handles provider SSE response deltas and JSON tool arguments; incoming request bodies are complete, and a prior field can precede the value that teaches the repeat sweep [proxy](proxy/proxy-runtime.md), [sweep](detection/manifest-sweep.md) | Streaming anonymize/restore contract not verified [2.2.364][p-core] | Streaming scan/restore contract not verified [0.3.0][c-readme] | Streaming output filtering is listed for **v5**, not 4.8 [4.8.1][y-v5] |
| Agent integration | Stdio MCP server and bridge, OpenAI/Anthropic/Gemini API-key proxy, JSONL daemon; Laravel adapter is a separate repository [MCP](mcp/mcp-runtime.md), [bridge](mcp/mcp-bridge.md), [proxy](proxy/proxy-runtime.md), [daemon](daemon/daemon-mode.md), [Laravel](support-drafts-in-production.md) | LiteLLM proxy sample; first-party MCP/daemon contract not verified [2.2.364][p-litellm] | Rust/Python/Node APIs; first-party MCP, proxy, or Claude Code hook not verified [0.3.0][c-readme] | Claude Code hook and LiteLLM request/response guardrail [4.8.1][y-readme] |

**Other detection tools**

| Capability | scrubadub | GLiNER | OPF | LLM Guard | PII-Tracer |
| --- | --- | --- | --- | --- | --- |
| Reversible restore and state | Replacement post-processors; restore contract not verified [2.0.0][s-usage] | Span detector; restore contract not verified [0.2.29][g-usage] | Span detector and masking CLI; restore contract not verified [0.1.0][o-readme] | `Anonymize`/`Deanonymize` share a `Vault`; session durability and unknown-token handling not verified [0.3.16][l-restore] | Span detector; `mask()` replaces spans with placeholders; restore contract not verified [d25c16f][t-card] |
| Placeholder or token format | Default `{{EMAIL}}` style; post-processors can add an index or hash [2.0.0][s-usage] | Returns labelled spans, not replacements [0.2.29][g-usage] | Typed spans or generic `redacted` labels; masking is not a documented restore token [0.1.0][o-output] | `[REDACTED_PERSON_1]` style placeholders [0.3.16][l-restore] | `[PRIVATE_EMAIL]` style via `mask()`; the format is configurable, not a documented restore token [d25c16f][t-card] |
| Repeated values | Repeat identity guarantee not verified [2.0.0][s-usage] | Not verified for a detector [0.2.29][g-usage] | Not verified for a detector [0.1.0][o-output] | Vault repeat behavior not verified [0.3.16][l-restore] | Trained and benchmarked to cover every mention of a recurring identifier across turns; this is detection coverage, not a stable replacement token [paper][t-paper] |
| Streaming | Streaming contract not verified [2.0.0][s-usage] | Incremental streaming NER is documented; this is detection, not restore [0.2.29][g-readme] | Long-context batch inference is documented; incremental stream contract not verified [0.1.0][o-readme] | Streaming anonymize/deanonymize contract not verified [0.3.16][l-restore] | Streaming contract not verified; input is truncated at 4096 tokens, so longer documents need caller-side chunking [d25c16f][t-card] |
| Agent integration | Python scrubber API; agent hook not verified [2.0.0][s-readme] | Python model API and optional serving; agent hook not verified [0.2.29][g-readme] | Local CLI/API; agent hook not verified [0.1.0][o-readme] | Input/output scanners and optional API deployment [0.3.16][l-readme] | Python `transformers` model API plus vLLM, GGUF (llama-server) and MLX packagings; agent hook not verified [d25c16f][t-card] |

## Detection, documents, and operations

**Runtimes and agent tools**

| Capability | Gaze | Presidio | DataFog Core | DataFog Python |
| --- | --- | --- | --- | --- |
| Document/OCR ingestion | PNG, JPG, PDF to `SafeBundle`; Tesseract OCR; owner manifest separated from agent files [document](document/document-extension.md) | Tesseract image/DICOM redactor, optional Azure Document Intelligence OCR, and PDF sample [2.2.364][p-image], [2.2.364][p-samples] | OCR ingestion not verified [0.3.0][c-readme] | Optional local image OCR; Donut needs a locally available model [4.8.1][y-readme] |
| Audit log | Optional metadata-only SQLite sink and queries; raw PII stays out of audit rows [audit](../how-to/audit-and-restore.md) | Analyzer returns scored findings; persistent audit sink not verified [2.2.364][p-analyzer] | Transformation records carry ranges and metadata; provider owns token audit logging [0.3.0][c-token] | Guardrail counts detections/redactions; durable audit sink not verified [4.8.1][y-agent] |
| Policy and rules | TOML action policy, bundled and custom rulepacks, recognizers, dictionaries, validators, locale cues [policy](../reference/policy.md) | Configurable recognizer registry, regex, deny lists, context, validators, and operators [2.2.364][p-analyzer] | Regex/configured detectors and separate transform strategies; no ML engine selector in Core [0.3.0][c-matrix] | Regex engine, allowlists, entity filters and optional NLP engines [4.8.1][y-readme] |
| Locales and country IDs | Locale chain and bundled country formats, with some packs needing explicit activation; no claim of complete national coverage [policy](../reference/policy.md) | **Broader country-ID catalog**, including 13 German entity types; the German recipe requires explicit bilingual setup [2.2.364][p-entities], [2.2.364][p-german] | Locale can constrain detectors; full country-ID catalog not verified [0.3.0][c-matrix] | Opt-in German IDs alongside core formats [4.8.1][y-readme] |
| NER and safety layers | Rule/dictionary floor; `gaze setup` enables Davlan NER and Nym by default; OPF is opt-in [setup](../../README.md), [benchmark](../reference/benchmarks/README.md), [safety nets](safety-net/safety-nets.md) | Rules plus configurable spaCy/Stanza/Hugging Face NER and context enhancer [2.2.364][p-analyzer] | Built-in pattern detectors; spaCy/GLiNER engine selection is out of Core scope [0.3.0][c-matrix] | Regex core plus optional spaCy or GLiNER [4.8.1][y-readme] |
| Structured data | Tool-call JSON strings; library-only `column` action rules (the CLI rejects them); CLI `clean` takes text, not a general table loader [proxy](proxy/proxy-runtime.md), [rule](../../crates/gaze/src/rule.rs), [policy](../../crates/gaze/src/policy.rs) | Dedicated tabular and JSON package [2.2.364][p-structured] | JSON leaf scanning, field paths and structured transformations in its published bindings [0.3.0][c-readme] | Text API and Spark UDF helpers; general nested-JSON transform contract not verified [4.8.1][y-readme] |

**Other detection tools**

| Capability | scrubadub | GLiNER | OPF | LLM Guard | PII-Tracer |
| --- | --- | --- | --- | --- | --- |
| Document/OCR ingestion | OCR ingestion not verified [2.0.0][s-readme] | OCR ingestion not verified [0.2.29][g-readme] | OCR ingestion not verified [0.1.0][o-readme] | OCR ingestion not verified [0.3.16][l-readme] | OCR ingestion not verified [d25c16f][t-card] |
| Audit log | Detector records and document names; durable audit sink not verified [2.0.0][s-usage] | Labelled spans and scores; durable audit sink not verified [0.2.29][g-usage] | Typed predictions can be exported; durable audit sink not verified [0.1.0][o-output] | Scanner validity/risk scores; durable audit sink not verified [0.3.16][l-restore] | Labelled spans and a document sensitivity score; durable audit sink not verified [d25c16f][t-card] |
| Policy and rules | Configurable detectors and post-processors; user-supplied values [2.0.0][s-usage] | Runtime label list, threshold, model selection and fine-tuning [0.2.29][g-usage] | Eight fixed output categories; runtime operating points; changing label policy requires fine-tuning [0.1.0][o-readme] | Presidio-backed recognizers, custom patterns and threshold [0.3.16][l-anon] | Nine fixed categories; no runtime label list or threshold documented [d25c16f][t-card] |
| Locales and country IDs | US/GB/CA postal and GB national identifiers, plus locale-specific detectors [2.0.0][s-readme] | Model `gliner_multi_pii-v1` card reports six languages; ID coverage depends on requested labels and model [0.2.29 + model `1fcf13e`][g-model] | Primarily English; eight broad categories rather than a country-ID catalog [0.1.0][o-readme] | English and Chinese support is documented, with the entity detection caveat that it is English-specific [0.3.16][l-anon] | Card lists `en` and `multilingual`; the paper's benchmark covers 13 languages and its account type names national ID, SSN, IBAN, card and sort code [d25c16f][t-card], [paper][t-paper] |
| NER and safety layers | Pattern detectors plus external spaCy/Stanford plugins [2.0.0][s-usage] | Learned NER itself; no independent rule floor verified [0.2.29][g-readme] | Bidirectional token classifier with constrained Viterbi span decoding; no rule floor verified [0.1.0][o-readme] | Presidio analyzer plus configurable NER and custom regex [0.3.16][l-anon] | 0.6B bidirectional Qwen3 token classifier with constrained BIOES Viterbi decoding and a document sensitivity head; no rule floor verified [d25c16f][t-card] |
| Structured data | Free-text scrubber; structured traversal not verified [2.0.0][s-readme] | Text/batch span inference; structured traversal not verified [0.2.29][g-usage] | Text span inference; structured traversal not verified [0.1.0][o-readme] | Prompt/output scanners; structured traversal not verified [0.3.16][l-readme] | Text input; structured traversal not verified [d25c16f][t-card] |

## Deployment and evaluation

**Runtimes and agent tools**

| Capability | Gaze | Presidio | DataFog Core | DataFog Python |
| --- | --- | --- | --- | --- |
| Offline / CPU / GPU | Local CPU rules and ONNX NER/net; models are installed by setup; no remote PII call required [safety nets](safety-net/safety-nets.md), [setup](../../crates/gaze-cli/README.md) | Local Python rules and NLP; CPU/GPU depends on chosen NLP engine [2.2.364][p-analyzer] | Local Rust built-ins; no model download for built-in detectors [0.3.0][c-readme] | Offline regex core; optional local models for spaCy/GLiNER/OCR [4.8.1][y-readme] |
| Platforms and SDKs | Rust crates and CLI; release binaries for Apple Silicon macOS and Linux x86_64, other targets build from source; Laravel adapter separate [README](../../README.md) | Python packages, CLI, HTTP services and containers [2.2.364][p-core], [2.2.364][p-cli], [2.2.364][p-anon] | **Rust, Python, Node.js, browser/WASM**; release binaries for macOS Intel/ARM, Linux x64/ARM64 and Windows x64. Browser cannot run provider-backed restore [0.3.0][c-readme] | Python package and CLI [4.8.1][y-readme] |
| Licence | Apache-2.0 **or** MIT [licences](../../README.md#contributing) | MIT [2.2.364][p-license] | MIT [0.3.0][c-license] | MIT [4.8.1][y-license] |
| Evaluation tooling | Versioned prose holdout, generated agentic identifiers, benign lookalikes and repeat-value benchmarks; byte leaks, false positives, refusals and exact restore checks [benchmark](../reference/benchmarks/README.md) | Presidio Research evaluation workflow and F-score guidance [2.2.364][p-eval] | Scan microbenchmark; annotated-corpus evaluator not verified [0.3.0][c-bench] | Reproducible performance suite with pinned payloads [4.8.1][y-bench] |

**Other detection tools**

| Capability | scrubadub | GLiNER | OPF | LLM Guard | PII-Tracer |
| --- | --- | --- | --- | --- | --- |
| Offline / CPU / GPU | Local Python detectors; optional NLP plugins [2.0.0][s-readme] | Local CPU/GPU model inference [0.2.29][g-readme] | Local CPU or GPU; browser/laptop use documented [0.1.0][o-readme] | Local scanners and optional API; CPU/GPU depends on chosen NER model [0.3.16][l-anon] | Local model; loading runs the repository's own Python code (`trust_remote_code`), so pin the revision; CPU is what this comparison used [d25c16f][t-card] |
| Platforms and SDKs | Python package [2.0.0][s-readme] | Python package, model ecosystem and optional serving [0.2.29][g-readme] | Python package and CLI, CPU/GPU execution [0.1.0][o-readme] | Python package and API deployment [0.3.16][l-readme] | Python package (`transformers>=5.2`) with vLLM, GGUF and MLX packagings [d25c16f][t-card] |
| Licence | Apache-2.0 [2.0.0][s-license] | Apache-2.0 library; cited PII model also Apache-2.0 [0.2.29][g-license], [model `1fcf13e`][g-model] | Apache-2.0 [0.1.0][o-license] | MIT; archived [0.3.16][l-license] | MIT [d25c16f][t-license] |
| Evaluation tooling | Accuracy and timing benchmark scripts [2.0.0][s-bench] | Model evaluation script and benchmark examples [0.2.29][g-eval] | `opf eval` supports typed/untyped annotated datasets; own-data fine-tuning [0.1.0][o-eval] | Scanner timing benchmark script [0.3.16][l-bench] | Introduced with the PII-TRACE benchmark (13,148 synthetic multi-turn dialogues in 13 languages in the paper). Only a 500-conversation English subset (MIT) is public, with a documented exact typed-span P/R/F1 protocol; the full set and the paper's scorer were not verified as public [paper][t-paper], [subset `1c3eb67`][t-data] |


Presidio has more published country recognizers and a dedicated structured-data
package. DataFog Core has broader SDK and prebuilt-platform coverage. DataFog
Python has Claude Code and LiteLLM hooks. GLiNER has incremental streaming NER;
OPF has a long-context neural detector and its own annotated-data evaluator.
Presidio offers an adopter evaluator through Presidio Research [2.2.364][p-eval];
Gaze's `gaze eval` command is in review in [PR #703](https://github.com/CertaMesh/gaze/pull/703).
These are real advantages for those use cases, not Gaze features under another
name. Conversely, a reversible operator, a token-shaped replacement, and a
documented agent integration do not by themselves establish the same
manifest/session/restore contract.

**Measured detection results:** [Gaze's competitor scorecards](../reference/benchmarks/competitors.md)
report Presidio, GLiNER and OPF on the same Gaze benchmark corpus and scorer,
with configurations, skipped-document accounting, false positives and latency.
Perplexity PII-Tracer's scores on the same corpus are on [their own page](../reference/benchmarks/competitors-pii-tracer.md), and its rows on the [competitors' own benchmarks](../reference/benchmarks/README.md#competitors-own-benchmarks) sit beside the other tools'.
They do **not** test competitor restore contracts and do not include DataFog,
scrubadub or LLM Guard. The [benchmark methods](../reference/benchmarks/README.md)
define the scored labels and generated layers. No general “best detector” claim
follows from that corpus. A separate “their benchmarks” page was not present at
this revision, so no result from it is linked here.

[p-pseudo]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/samples/python/pseudonymization.ipynb
[p-core]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/presidio/README.md
[p-anon]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/anonymizer/index.md
[p-cli]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/presidio-cli/README.md
[p-samples]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/samples/index.md
[p-litellm]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/samples/docker/litellm.md
[p-image]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/image-redactor/index.md
[p-analyzer]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/analyzer/index.md
[p-entities]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/supported_entities.md
[p-german]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/recipes/german-language-support/README.md
[p-structured]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/structured/index.md
[p-eval]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/evaluation/index.md
[p-license]: https://github.com/data-privacy-stack/presidio/blob/2.2.364/LICENSE
[c-token]: https://github.com/DataFog/datafog-core/blob/v0.3.0/docs/guides/tokenization-and-restoration.mdx
[c-readme]: https://github.com/DataFog/datafog-core/blob/v0.3.0/README.md
[c-matrix]: https://github.com/DataFog/datafog-core/blob/v0.3.0/docs/privacy-capability-matrix.md
[c-bench]: https://github.com/DataFog/datafog-core/blob/v0.3.0/crates/core/examples/scan_benchmark.rs
[c-license]: https://github.com/DataFog/datafog-core/blob/v0.3.0/LICENSE
[y-roadmap]: https://github.com/DataFog/datafog-python/blob/v4.8.1/docs/roadmap.rst
[y-engine]: https://github.com/DataFog/datafog-python/blob/v4.8.1/datafog/engine.py
[y-v5]: https://github.com/DataFog/datafog-python/blob/v4.8.1/docs/v5-cut-line.rst
[y-readme]: https://github.com/DataFog/datafog-python/blob/v4.8.1/README.md
[y-agent]: https://github.com/DataFog/datafog-python/blob/v4.8.1/datafog/agent.py
[y-bench]: https://github.com/DataFog/datafog-python/blob/v4.8.1/benchmarks/README.md
[y-license]: https://github.com/DataFog/datafog-python/blob/v4.8.1/LICENSE
[s-usage]: https://github.com/LeapBeyond/scrubadub/blob/v2.0.0/docs/usage.rst
[s-readme]: https://github.com/LeapBeyond/scrubadub/blob/v2.0.0/README.rst
[s-bench]: https://github.com/LeapBeyond/scrubadub/blob/v2.0.0/tests/benchmark_accuracy.py
[s-license]: https://github.com/LeapBeyond/scrubadub/blob/v2.0.0/LICENSE
[g-readme]: https://github.com/urchade/GLiNER/blob/v0.2.29/README.md
[g-usage]: https://github.com/urchade/GLiNER/blob/v0.2.29/docs/usage.md
[g-eval]: https://github.com/urchade/GLiNER/blob/v0.2.29/eval.py
[g-license]: https://github.com/urchade/GLiNER/blob/v0.2.29/LICENSE
[g-model]: https://huggingface.co/urchade/gliner_multi_pii-v1/tree/1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d
[o-readme]: https://github.com/openai/privacy-filter/blob/f7f00ca7fb869683eb732c010299d901457f19c3/README.md
[o-output]: https://github.com/openai/privacy-filter/blob/f7f00ca7fb869683eb732c010299d901457f19c3/OUTPUT_SCHEMAS.md
[o-eval]: https://github.com/openai/privacy-filter/blob/f7f00ca7fb869683eb732c010299d901457f19c3/EVAL_AND_OUTPUT_MODES.md
[o-license]: https://github.com/openai/privacy-filter/blob/f7f00ca7fb869683eb732c010299d901457f19c3/LICENSE
[l-readme]: https://github.com/protectai/llm-guard/blob/v0.3.16/README.md
[l-anon]: https://github.com/protectai/llm-guard/blob/v0.3.16/docs/input_scanners/anonymize.md
[l-restore]: https://github.com/protectai/llm-guard/blob/v0.3.16/docs/output_scanners/deanonymize.md
[l-bench]: https://github.com/protectai/llm-guard/blob/v0.3.16/benchmarks/run.py
[l-license]: https://github.com/protectai/llm-guard/blob/v0.3.16/LICENSE
[t-card]: https://huggingface.co/perplexity-ai/PII-Tracer/blob/d25c16f2e57e321f6d2527715c01df9112f956f5/README.md
[t-license]: https://huggingface.co/perplexity-ai/PII-Tracer/blob/d25c16f2e57e321f6d2527715c01df9112f956f5/LICENSE
[t-paper]: https://arxiv.org/abs/2609.22200v1
[t-data]: https://huggingface.co/datasets/perplexity-ai/PII-TRACE/blob/1c3eb67bbd43e8571b4433a9d3d76271f9970eb9/README.md
