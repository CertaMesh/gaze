# Privacy-conformant patterns for AI agents accessing production data

**The central architectural challenge for a GDPR-compliant AI agent data proxy is that agents need rich context to reason effectively while GDPR demands data minimization — and pseudonymized data remains personal data under EU law.** The industry is converging on a layered defense pattern: static policy allowlists → regex fast-pass → optional NER → deterministic tokenization with session-scoped keys, deployed as transparent proxy middleware between agents and data sources. Gaze's existing two-layer architecture (TOML allowlist + runtime PII scanner with HMAC pseudonymization) aligns well with emerging best practices, though several critical gaps remain — particularly around compositional privacy attacks where individually clean agent queries collectively reconstruct PII, and inference-driven re-identification where LLMs deduce identity from anonymized patterns.

This report synthesizes findings across privacy proxy architectures, reversible masking patterns, performance benchmarks, agent-specific risks, real-world products, anti-patterns, and emerging standards to inform Gaze's v0.2 pipe mode and future development.

---

## 1. Executive summary: ten architectural decisions that matter most

These decisions define whether a GDPR-compliant AI agent data proxy provides real protection or compliance theater:

1. **Deterministic tokenization over placeholder masking.** Research from NoPII shows deterministic tokens preserve **91–96% of LLM output quality** versus 54–68% for `[REDACTED]`-style placeholders. Agents reason better over consistent tokens that maintain relational structure. Gaze's HMAC approach is correct.

2. **Session-scoped keys with zeroization, not persistent token vaults.** Destroying the HMAC key on session exit converts pseudonymized data into effectively anonymized data (the "additional information" required for re-identification ceases to exist). This is stronger than persistent vaults under EDPB Guidelines 01/2025, which clarify that the controller's ability to reverse pseudonymization determines GDPR applicability.

3. **Hybrid regex→NER detection, not regex alone.** Production systems achieve **F1 scores of 0.81–0.94** with hybrid pipelines versus ~0.65 recall for regex-only. The GDPR Art. 25 "state of the art" obligation likely means regex-only detection is insufficient in 2026. However, a Rust CLI can offer regex-only as the fast default with optional NER as a flag.

4. **Whitelist-by-default, not blacklist.** New database columns and log fields should be masked by default; only explicitly allowlisted fields pass through unmasked. This prevents human error — the most common PII detection failure is not recognizing that a new field contains PII.

5. **Query-budget or cardinality guards against compositional attacks.** Individual query sanitization is necessary but insufficient. An agent running `SELECT COUNT(*) WHERE age=25 AND city=Berlin AND role=engineer` repeatedly with narrowing predicates can reconstruct individual identity. Minimum result-set cardinality checks (k≥5) or per-session query budgets are essential.

6. **No HTTP surface, ever.** Unix pipes and stdio eliminate entire attack surface categories — no authentication bypass, no SSRF, no credential rotation. Gaze's architecture is correct here and should resist pressure to add a web server.

7. **Structured data needs schema-aware masking, not text scanning.** SQL result sets should be masked by column policy (foreign keys preserved deterministically), not by running text PII detection over serialized output. JSON responses should be walked recursively with per-path policies.

8. **SQL query literals must be scanned.** This is a significant gap in existing tooling. WHERE clause values and INSERT literals can contain PII that bypasses result-set masking. Rust's `sqlparser` crate can extract string literals for scanning.

9. **Audit log everything, retain nothing reversible.** The SQLite audit log should record what entity types were detected and masked, session identifiers, and tool invocations — but never the token-to-value mapping. Under GDPR Art. 32, the audit trail itself must not become a re-identification vector.

10. **Progressive disclosure minimizes context window exposure.** Send agents aggregated summaries first; provide row-level detail only on justified request. This reduces both PII exposure surface and agent context window pollution.

---

## 2. Proven patterns for privacy-preserving agent data access

### The transparent privacy proxy

The dominant architectural pattern across Skyflow, Satori, Protegrity, and the emerging MCP privacy middleware is the **transparent intercepting proxy** — a component that sits between the data consumer (AI agent) and the data source (database, log stream), intercepting all traffic and applying transformations without requiring changes to either endpoint.

Skyflow's architecture operates as an isolated vault where applications interact only via API calls and downstream systems store only opaque tokens. Satori Cyber takes a different approach as a transparent database proxy that applies dynamic masking based on the requester's identity — production engineers see unmasked data while AI agents see masked data. Protegrity distributes lightweight "Protectors" across the data environment with a centralized policy engine, achieving **180 million+ token operations per second** in Redshift benchmarks through vaultless, algorithmic token generation.

For MCP specifically, several proxy patterns have emerged in 2025–2026. **Skyflow MCP Gateway** enforces field-level privacy policies without application changes. **Promptfoo MCP Proxy** provides enterprise-grade traffic interception with PII monitoring and per-user permissions. **MCP Manager** integrates Microsoft Presidio for NLP-based PII detection within the MCP traffic flow. Most directly relevant to Gaze is **MCP Server Conceal** (github.com/gbrigandi/mcp-server-conceal) — a Rust-based MCP privacy proxy that intercepts tool responses, detects PII with two-phase detection (regex then optional LLM analysis), and applies pseudo-anonymization with consistent mapping.

The key architectural insight across all these systems: **privacy enforcement must be fail-closed**. Unknown fields are masked, not passed through. New data types default to maximum protection. The Grab engineering team's Kafka pipeline exemplifies this — their Flink masking application simply drops any unknown protobuf fields rather than risk passing untagged PII.

### Schema-aware versus text-based masking

For structured data (SQL results, JSON API responses), the industry splits into two camps. Text-based scanning runs PII detection across serialized output — simple to implement but prone to false positives on numeric values and unable to maintain referential integrity. Schema-aware masking applies deterministic transformations per-column, preserving foreign key relationships because the same plaintext FK value always maps to the same token.

PostgreSQL Anonymizer implements the schema-aware approach directly in database schema definitions via `SECURITY LABEL`, keeping masking rules with schema developers who understand the data model. Microsoft Presidio's Structured module (beta) maps column/key names to PII entity types, enabling consistent per-field treatment. For Gaze, the TOML policy allowlist is the right abstraction — it enables schema-aware column-level rules while remaining data-source agnostic.

For unstructured data (logs, stack traces), the hybrid regex→NER pipeline is the established pattern. Regex catches structured PII with predictable formats (emails, credit cards, IPs) in sub-millisecond time. NER models then catch contextual PII (names, addresses) that regex misses. The OpenTelemetry community's "Safe Observability" framework implements this as a custom PII-Redaction Processor in the OTel Collector, acting as a centralized "Telemetry Firewall" — architecturally analogous to what Gaze does for AI agent traffic.

### Reversible tokenization with session isolation

Three reversible masking approaches dominate, each with distinct trade-offs:

**HMAC-based session tokenization** (Gaze's current approach) generates tokens via keyed hash. Within a session, the same input always produces the same token, enabling an agent to recognize that "the same user" appears in multiple log lines. When the session key is destroyed, reversal becomes computationally infeasible. The limitation: single-iteration HMAC offers no CPU-hardening, so if key K is compromised during the session, all pseudonyms can be recomputed rapidly.

**Format-preserving encryption (FPE)** using FF1 encrypts data while preserving its format — an encrypted email looks like a valid email, an encrypted phone number looks like a valid phone number. This is superior for agent reasoning because format validation in downstream processing still works. However, **NIST withdrew FF3/FF3-1 in February 2025** due to Beyne's 2021 attack; only FF1 remains approved with a minimum domain size of 1 million. FF1 also has patent claims affecting open-source implementations.

**Vault-based tokenization** stores token-to-value mappings in a secure vault, enabling reversal via lookup. Protegrity's "vaultless" variant uses cryptographic algorithms to generate format-preserving tokens without a vault database, eliminating the scalability bottleneck. For session-scoped use, vault-based approaches add unnecessary complexity — the vault must be secured, and its existence extends GDPR obligations.

For Gaze's use case, session-scoped HMAC is the strongest choice because key destruction on exit provides the cleanest privacy boundary. Adding format-preserving output (making tokens look like valid emails/IPs) would improve agent reasoning quality without changing the underlying security model.

---

## 3. Real-world products solving adjacent problems

**MCP Server Conceal** is the most directly comparable open-source project — a Rust MCP proxy that pseudo-anonymizes PII before data reaches external AI providers. It replaces `john.smith@acme.com` with `mike.wilson@techcorp.com`, preserving structure for AI analysis while protecting real identities. Two-phase detection uses regex first, then optional LLM analysis.

**LLM Guard** (Protect AI, open source, MIT license, 2.5M+ downloads) provides 15 input scanners and 20 output scanners including an `Anonymize` scanner that replaces PII with tokens before sending to an LLM, and a `Deanonymize` scanner that restores values using a vault. The Anonymize/Deanonymize + Vault pattern directly mirrors Gaze's Ghostwriter clean/restore architecture.

**Lakera Guard** handles outbound PII filtering at sub-50ms latency with **98%+ detection** and <0.5% false positive rate across 100+ languages. It screens data going both TO and FROM LLMs, returning flagged locations for application-level masking. Kong Gateway has a native Lakera plugin.

**Nightfall AI** provides an explicit "Firewall for AI" product — an API wrapper intercepting sensitive data before it reaches public LLMs. It achieves **95% precision/recall**, ≤100ms P99 latency, and includes data lineage tracking that traces information from source to destination.

**Satori Cyber** operates as a transparent database proxy with dynamic masking — the architecturally closest commercial product to what Gaze does for MySQL. It auto-classifies sensitive data, applies masking transformations in real-time based on requester identity, and supports MySQL, PostgreSQL, Snowflake, and 10+ other data stores.

**redact-core** (Rust, lib.rs/crates/redact-core) is a high-performance Rust PII detection engine with 36+ pattern-based entity types, ONNX Runtime integration for transformer models, and multiple anonymization strategies. Memory footprint is **~20–50MB versus ~300MB for Presidio**, with 2ms processing time for basic text. Modular crate structure includes CLI, API, and WASM bindings.

**Protecto AI** provides reversible masking specifically designed for LLM use cases — it masks PII before LLM input and unmasks after LLM output, with RBAC so different roles see different masking levels. Unlike simple regex masking, it handles LLM response variations that don't exactly match input text.

**GreenMask** (greenmask.io, open source) handles PostgreSQL-specific batch anonymization with declarative YAML transformation rules, deterministic transformers, and CI/CD pipeline integration — useful for creating sanitized staging databases.

**pganalyze** demonstrates the edge-filtering pattern: its open-source collector filters PII from Postgres logs, `pg_stat_activity`, and `pg_stat_statements` before data leaves customer infrastructure. This architectural pattern — filter at the edge, before data crosses the trust boundary — is exactly Gaze's approach.

---

## 4. Tokenization vs. FPE vs. synthetic data vs. differential privacy

| Dimension | HMAC tokenization | Format-preserving encryption (FF1) | Synthetic data generation | Differential privacy |
|---|---|---|---|---|
| **GDPR status** | Pseudonymized (personal data) | Pseudonymized (personal data) | Contested; likely pseudonymized without DP | Can achieve anonymization (outside GDPR) |
| **Reversibility** | Requires lookup table or key | Reversible with key | Irreversible by design | Irreversible |
| **Format preservation** | No (unless explicitly designed) | Yes, by construction | N/A (new records) | N/A (noise added) |
| **Consistency** | Deterministic (same input → same token) | Deterministic (same key + tweak) | No record correspondence | N/A |
| **Agent reasoning quality** | 91–96% preserved (NoPII benchmark) | Higher (format-valid tokens) | High (realistic data) | Reduced (noise degrades accuracy) |
| **Performance** | Fast (HMAC is ~ns per operation) | Fast for short data (AES-based) | High compute for generation | Minimal overhead for noise injection |
| **Key management** | Single session key | Distributed keys at decryption points | Model management | Privacy budget tracking |
| **Session isolation** | Natural (key per session) | Requires per-session tweak management | N/A (batch generation) | Budget per session possible |
| **Best for** | Real-time proxying, debugging sessions | Systems requiring format validation | Test environments, ML training | Aggregate queries, analytics |
| **Weakness** | Tokens don't validate as original type | FF1 patent claims; min domain 1M | Not anonymous without DP; inference attacks | Utility degrades with query count |

**For Gaze's use case**, HMAC tokenization with session-scoped keys is the optimal primary mechanism. FPE (FF1) could be added as an enhancement for specific high-value data types (emails, phone numbers) where format-valid output significantly improves agent reasoning. Differential privacy should be considered for aggregate query responses (COUNT, AVG) to prevent narrowing attacks. Synthetic data is irrelevant for real-time debugging.

The critical GDPR boundary: **all reversible masking produces pseudonymized data**, which remains personal data requiring full GDPR compliance. Only differential privacy with formal epsilon guarantees, or session-scoped tokenization where the key is provably destroyed, can approach true anonymization. The EDPB's January 2025 guidelines explicitly state that deleting mapping information does not automatically make pseudonymized data anonymous — anonymization conditions must be independently met.

---

## 5. Performance benchmarks and latency expectations

### Throughput by detection approach

| Approach | Throughput | Latency per line | Memory | Source |
|---|---|---|---|---|
| Rust regex literal search (SIMD) | **4.8–9.0 GB/s** | <0.01ms | <10MB | BurntSushi rebar benchmarks |
| Rust regex complex alternation | **800 MB/s** | <0.1ms | <50MB | Rust regex crate lazy DFA |
| ripgrep (multi-pattern, real files) | **80 MB/s** single-core | <0.5ms | <30MB | Production benchmarks |
| Edge Delta regex PII masking (6 patterns) | **50 MB/s** | ~0.01ms | 5–7 cores used | Vendor benchmark, 12-core Apple Silicon |
| Hybrid regex+NER pipeline | **1–5 MB/s** | 10–100ms | 500MB+ | Research benchmarks (F1=0.94) |
| Microsoft Presidio (Python, full pipeline) | **0.01–0.1 MB/s** | 200ms+ per request | ~300MB | Community-reported, GitHub issues |
| spaCy NER alone | **0.04 MB/s** per core | ~125ms per KB | ~580MB | Explosion blog |
| Google Cloud DLP API | Network-bound (~0.1 MB/s) | 50–200ms per request | N/A (cloud) | API latency benchmarks |

### Realistic targets for Gaze pipe mode

For a Rust CLI using Aho-Corasick + regex with 10–20 PII patterns, realistic single-threaded targets are **100–500 MB/s** for regex-only detection including replacement, dropping to **50–200 MB/s** with checksum validation (Luhn for credit cards, etc.), and **1–5 MB/s** with optional Candle-based NER. These numbers are orders of magnitude faster than Python alternatives.

**Latency budget for pipe mode**: interactive developer tools require **<10ms per line** for imperceptible delay. For a typical 200-byte log line, regex-only processing adds <0.01ms — effectively zero overhead. The bottleneck will be I/O syscalls, not PII detection. Using 64KB block buffers (matching Linux pipe buffer size) with newline-delimited processing minimizes syscall overhead.

**Buffer boundary handling**: the recommended approach is reading blocks but processing only up to the last complete newline delimiter, carrying the remainder to the next block. For arbitrary text without delimiters, maintain an overlap window of 256–512 bytes (longer than any expected PII token) and re-scan the overlap region with each new block. Rust's `AhoCorasick::stream_find_iter()` supports stream searching without loading entire input into memory.

**Memory budget**: <50MB RSS for regex-only mode (pattern automata + I/O buffers), <500MB with NER model loaded. The `redact-core` Rust crate achieves **20–50MB** for its complete PII engine versus ~300MB for Presidio.

Key optimization: pre-compile all regex patterns into a single `RegexSet` or Aho-Corasick automaton at startup. For JSON, use `simd-json` or `serde_json` to parse, walk string values only, and reconstruct with byte-offset patching to avoid full re-serialization.

---

## 6. Privacy risks unique to AI agents

### Inference-driven re-identification is a validated threat

A landmark 2026 paper ("From Weak Cues to Real Identities," arXiv 2603.18382) demonstrates that **LLM agents can reconstruct specific real-world identities by combining non-identifying cues from anonymized artifacts with corroborating signals**. Using anonymized AOL search query histories with all self-PII filtered out, agents still linked records to real identities through indirect signals. The critical finding: existing agent privacy evaluations (PrivacyLens, AgentDAM) track whether agents access or disclose sensitive information but are "not designed to measure inference" — meaning current guardrails miss this attack class entirely.

This directly threatens any HMAC-based pseudonymization system. If `TOKEN_7f3a` consistently appears alongside "Berlin," "senior engineer," and "joined 2019," an agent can cross-reference against public LinkedIn data to infer identity, even though no single token is PII. **Gaze should consider adding configurable quasi-identifier suppression** — removing or generalizing attributes like city, job title, and tenure that enable inference when combined.

### Compositional attacks across tool calls

"The Sum Leaks More Than Its Parts" (arXiv 2509.14284, September 2025) formalizes the compositional privacy attack: an agent issues multiple individually harmless queries whose results collectively reconstruct PII. Separately obtaining customer ID mappings, purchase logs, and insurance claims from different tool calls yields full profiles. Tested defenses using GPT-5 and Gemini-2.5-pro show that **baseline Chain-of-Thought defenses maintain 64–76% benign task success but offer limited protection against compositional attacks**. Google DeepMind's research shows **86% success rates** for compositional fragment attacks.

For Gaze, this means per-query sanitization is necessary but insufficient. Practical defenses include per-session query budgets (limit total queries), minimum result-set cardinality (reject queries returning fewer than k records), and cross-query pattern analysis (detect narrowing predicates across queries in a session).

### Context window exposure and provider retention

LLM context windows have **no privilege separation** — all input is processed with equivalent authority, unlike operating systems with user/kernel boundaries. Data sent via MCP tools is subject to provider retention: **Anthropic retains API data for 7 days**, OpenAI for 30 days, Google for 55 days for abuse monitoring. None train on API data by default. Zero-data-retention options exist for all three providers.

The underappreciated risk is MCP server-side logging. Without a gateway, "communication between MCP clients and MCP servers is generally unseen to users" — creating a governance blind spot. Gaze's architecture of filtering before data leaves the security boundary is the correct pattern, and the SQLite audit log should explicitly avoid storing raw PII.

### MCP has no protocol-level privacy guarantees

The MCP specification explicitly states it "cannot enforce these security principles at the protocol level." Privacy is entirely an implementation responsibility. No dedicated privacy sub-specification exists. This creates both risk (no standardized protection) and opportunity (Gaze can establish the pattern). An arXiv paper mapping MCP controls to ISO/IEC 27001 recommends "inline inspection, redaction, and blocking of prompts and tool responses before they cross trust boundaries" — precisely Gaze's approach.

### GDPR legal basis for debugging via AI agent

**Art. 6(1)(f) legitimate interest** is the most appropriate legal basis for AI-assisted debugging, per EDPB Opinion 28/2024. The three-step Legitimate Interest Assessment requires: (1) identifying a lawful, clearly defined interest, (2) demonstrating that processing is strictly necessary (the EDPB "sets a high bar for necessity in relation to the volume of personal data involved"), and (3) balancing against data subject rights. A documented LIA showing that pseudonymized debugging access with session-scoped keys minimizes privacy impact while serving a legitimate operational interest should satisfy this test. Art. 6(1)(b) contract performance is available only when debugging is directly necessary for contractual service delivery (e.g., SLA commitments), not merely useful.

---

## 7. What not to do, with examples of failures

### Detection anti-patterns

**Regex-only detection creates a false sense of coverage.** Regex achieves ~0.65 recall in studies — it misses names, contextual identifiers, natural language numbers ("call me at five five five one two 88"), and PII in non-English formats. German phone numbers with +49 country codes and varying digit groupings, dates in 16+ international formats, and names with umlauts (ä, ö, ü, ß) break US-centric regex patterns. The GDPR Art. 25 "state of the art" obligation means regex-only is likely insufficient in 2026.

**Ignoring PII in SQL query text** is a significant gap. Most tools scan result sets but not the queries themselves. WHERE clause values (`WHERE email = 'john@example.com'`) and INSERT literals contain PII that bypasses result-set masking entirely. Gaze should use `sqlparser-rs` to extract string literals from queries for scanning.

**Not handling buffer boundaries** causes split-token misses. An email address split across two buffer reads — `john.doe@` in chunk 1, `example.com` in chunk 2 — is invisible to per-chunk scanning. The fix: process up to the last complete delimiter, carry the remainder forward.

### Anonymization failures with real consequences

The **Netflix Prize** (2007): removing names and adding noise to 10M movie ratings was insufficient. UT Austin researchers re-identified users by correlating with public IMDb reviews — knowing just 2 movies a user reviewed with approximate dates gave **68% re-identification success**. A class-action lawsuit followed, and Netflix cancelled its planned second competition.

The **NYC Taxi dataset** (2014): medallion numbers "pseudonymized" via MD5 hashing were trivially reversed because medallion numbers come from a known, finite set. Hashing all possible values and matching was trivial. **Lesson: deterministic hashing of low-entropy inputs is not pseudonymization.** This applies directly to database IDs, phone numbers, and any enumerable value set.

The **Massachusetts health records** (1997): Latanya Sweeney re-identified Governor Weld's medical records from "anonymized" data by cross-referencing with voter registration using just ZIP code, birth date, and sex. Her research showed **87% of the US population** can be uniquely identified by these three quasi-identifiers alone.

### Security anti-patterns for tokenization systems

**Storing the tokenization key alongside tokens** defeats the purpose entirely. Under EDPB Guidelines 01/2025, the "additional information" (keys, mapping tables) must be kept separate from those who should not identify individuals. Gaze's approach of holding the HMAC key only in process memory with zeroization on exit is correct.

**Not zeroizing sensitive memory** is dangerous because standard `memset()` calls can be silently removed by optimizing compilers as "dead stores." Rust's `zeroize` crate implements the `Zeroize` trait with `Drop`-based auto-clearing that the compiler cannot optimize away. Additionally, `mlock()` should prevent memory pages from being swapped to disk, and core dumps should be disabled.

**Logging the token mapping table** creates a persistent, often unprotected copy of the re-identification key in log files that are typically aggregated into centralized systems with broad access. Gaze's audit log must record entity types detected and session metadata, never token-to-value mappings.

### GDPR compliance theater

The most common failure pattern: treating GDPR as a one-time checkbox exercise rather than continuous operational compliance. Organizations produce privacy policies and consent forms but don't implement technical measures that actually reduce risk. **Transparent Database Encryption (TDE) alone** is a frequent example — it protects data at rest but decrypts transparently for any authorized database user, providing zero protection against insider threats or application-level access. Weak pseudonymization methods (simple hashing, predictable algorithms) treated as meaningful protection are arguably worse than no pseudonymization, because they encourage risky data sharing under false confidence.

---

## 8. What to build first versus later

### v0.2 pipe mode (immediate priorities)

**Buffer management with boundary awareness** should be the first pipe mode implementation focus. Use 64KB block buffers matching Linux pipe capacity, process up to the last complete newline, carry remainder forward. Detect `isatty()` for automatic line-buffered (TTY) versus block-buffered (pipe) mode selection. This alone enables `mysql ... | gaze clean | agent` workflows.

**SQL literal scanning** is the highest-value gap to fill. No existing tool does this well. Use `sqlparser-rs` to extract string literals from WHERE clauses and INSERT VALUES, run them through the PII scanner, and replace in the reconstructed query. This catches PII that result-set scanning misses entirely.

**Structured JSON walking** should replace text scanning for JSON output. Parse with `serde_json`, recursively walk string values, apply per-path TOML policies, reconstruct with minimal allocation. This preserves JSON structure and enables schema-aware column-level masking.

### v0.3–v0.4 (medium-term enhancements)

**Format-preserving tokenization output** that makes masked emails look like valid emails and masked IPs look like valid IPs. This doesn't require FPE (with its patent concerns); it can be achieved by generating structurally valid fake values deterministically from the HMAC hash. The NoPII research shows this improves LLM reasoning quality compared to opaque tokens.

**Query budget and cardinality guards** against compositional attacks. Track per-session query patterns in the SQLite audit log, enforce minimum result-set sizes (k≥5), and optionally limit total queries per session. This addresses the most critical agent-specific risk.

**Candle-based optional NER** as a compile-time feature flag for when users need higher recall than regex provides. Target 1–5 MB/s throughput — still 100x faster than Python alternatives. Default to regex-only for pipe mode, with `--deep-scan` flag to enable NER.

### vFuture (strategic capabilities)

**Differential privacy for aggregate queries** — add calibrated Laplace noise to COUNT/AVG/SUM responses before returning to agents. This requires a privacy budget accounting system per session. Start with high-epsilon (ε=8) and tighten over time based on operational experience.

**Progressive disclosure API** — when agents request data, first return schema metadata and aggregate summaries; require explicit justification (logged) for row-level access. This implements data minimization as an architectural constraint rather than a policy hope.

**Cross-session pseudonym stability** (optional, opt-in) for use cases requiring correlation across debugging sessions. This requires persistent key storage with HSM-backed key management and explicitly documented GDPR basis — a significantly higher compliance burden than session-scoped keys.

**Quasi-identifier suppression** — configurable generalization of attributes (city → region, exact age → age range, job title → department) that prevent inference-driven re-identification when combined. This addresses the validated LLM inference attack described in the 2026 arXiv paper.

---

## 9. Key resources for implementation

### Rust crates directly relevant to Gaze

The **worka-ai/pii** crate (crates.io/crates/pii) provides deterministic PII detection with stable byte offsets, CPU-only design, capability-aware pipeline degradation, and optional Candle-based NER. **redact-core** (lib.rs/crates/redact-core) offers 36+ pattern types with ONNX Runtime integration, 20–50MB memory footprint, and 2ms processing time for basic text. **pii-vault** (cargo add pii-vault) provides cross-language reversible tokenization with 29 recognizers across 15 countries. **sqlparser-rs** enables SQL AST parsing for literal extraction. **zeroize** implements compiler-safe memory clearing.

### Research papers defining the threat model

"From Weak Cues to Real Identities" (arXiv 2603.18382, 2026) validates inference-driven de-anonymization by LLM agents. "The Sum Leaks More Than Its Parts" (arXiv 2509.14284, September 2025) formalizes compositional privacy attacks across tool calls. "Differential Privacy in Generative AI Agents" (arXiv 2603.17902, March 2026) provides the formal privacy-utility tradeoff framework. "Securing the Model Context Protocol" (arXiv 2511.20920) maps MCP controls to ISO 27001 Annex A. The USENIX Security 2025 paper demonstrates GPT-4o's **65.6% attack success rate** for PII extraction.

### Standards and regulatory guidance

**EDPB Guidelines 01/2025 on Pseudonymisation** (adopted January 16, 2025) define the three-step test: pseudonymising transformation, separation of additional information, and technical/organizational measures. **ISO/IEC 27701:2025** now addresses AI-related processing with 29 required controls mapped to GDPR. The **EU AI Act** classifies any AI system performing profiling as automatically high-risk with obligations effective August 2026. **EDPB Opinion 28/2024** provides the legitimate interest assessment framework for AI systems processing personal data.

### Open-source reference architectures

MCP Server Conceal (github.com/gbrigandi/mcp-server-conceal) is the closest Rust-based MCP privacy proxy. LLM Guard (github.com/protectai/llm-guard) provides the Anonymize/Deanonymize + Vault pattern in Python. Microsoft Presidio remains the reference architecture for pluggable PII detection pipelines. GreenMask (greenmask.io) demonstrates declarative PostgreSQL anonymization with deterministic transformers. The Edge Delta benchmark (50 MB/s regex PII masking on 12-core Apple Silicon) provides a realistic performance reference point for production regex-based scanning.

---

## Conclusion: where Gaze sits in the emerging landscape

Gaze occupies a genuinely underserved architectural position. The existing privacy proxy market is dominated by heavyweight enterprise platforms (Skyflow, Protegrity, Immuta) designed for data-at-rest governance, and the emerging MCP privacy middleware (Promptfoo, MCP Manager) operates as cloud gateways. **No production tool currently provides a single-binary, zero-network-dependency, session-scoped privacy proxy specifically for AI coding agents accessing local production data sources.** MCP Server Conceal comes closest but lacks Gaze's TOML policy layer, pipe mode, and Ghostwriter restore capability.

The three highest-impact gaps in Gaze's current architecture are: compositional attack defense (query budgets/cardinality guards), SQL literal scanning, and format-preserving token output. The session-scoped HMAC with key destruction is architecturally superior to persistent vault approaches for the debugging use case — it's both simpler and more GDPR-defensible, because the "additional information" ceases to exist when the session ends.

The fundamental unsolved problem in this space is that **individual-query privacy does not compose**. Every system reviewed — from Protegrity to Presidio to the latest academic defenses — struggles with the fact that an intelligent agent running queries in a loop can reconstruct what no single query reveals. This is the frontier where Gaze's v0.3+ roadmap should focus, and where the tool has an opportunity to establish patterns that the broader ecosystem will eventually adopt.

---

## TODO: Investigate implications for Gaze

- [ ] **Compositional attack surface**: How vulnerable is Gaze v0.1 to multi-query reconstruction? Prototype a narrowing-predicate attack against current HMAC tokenization to quantify risk.
- [ ] **SQL literal scanning gap**: Audit current `gaze clean` path — does it scan query text or only result sets? If only results, prioritize `sqlparser-rs` integration for v0.2.
- [ ] **Low-entropy HMAC weakness**: NYC Taxi lesson applies to database IDs and phone numbers. Test whether Gaze's HMAC tokens for enumerable value sets (e.g., AU phone numbers) are brute-forceable within session lifetime.
- [ ] **Buffer boundary handling**: Verify current pipe implementation handles split PII across read boundaries. Write targeted test with email split across 64KB boundary.
- [ ] **EDPB Guidelines 01/2025 compliance**: Review Gaze's key lifecycle against the three-step pseudonymisation test. Document in README or compliance doc.
- [ ] **Format-preserving output feasibility**: Spike generating structurally-valid fake values from HMAC hash (email→email, IP→IP) without FPE. Measure impact on agent reasoning quality.
- [ ] **Quasi-identifier suppression**: Evaluate whether TOML policy can express generalization rules (city→region) or if new policy syntax needed for v0.3.
- [ ] **MCP Server Conceal comparison**: Clone and benchmark against Gaze on same corpus. Identify features worth adopting vs. architectural divergences.
- [ ] **redact-core evaluation**: Compare detection coverage and performance against worka-ai/pii crate. Determine if either supplements Gaze's built-in scanner.
- [ ] **EU AI Act August 2026 deadline**: Assess whether Gaze's use case (agent accessing prod data) triggers high-risk classification under profiling provisions.
