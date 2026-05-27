# GDPR / DPIA Guidance for Adopters

This document is written for developers and Data Protection Officers (DPOs) who are evaluating Gaze for a workflow that processes personal data under the EU General Data Protection Regulation (GDPR).

It is **not legal advice**. Whether a specific deployment of Gaze satisfies your organisation's GDPR obligations depends on your processing purpose, legal basis, and risk profile — questions that only your DPO or counsel can answer for you. This guidance describes how Gaze behaves so that you can plug those facts into your own assessment.

---

## 1. Controller / Processor relationship

GDPR distinguishes between the *controller* (who decides why and how personal data is processed) and the *processor* (who processes data on behalf of the controller).

When you deploy Gaze inside your own application:

- **You (the adopter) remain the controller** for the personal data that flows through your system. You decide which messages to redact, which third-party LLM to call, and how long to retain the restore manifest.
- **Gaze, as a library or local binary, runs entirely inside your trust boundary.** The Gaze maintainers do not see, store, or have any access to the personal data your application processes. There is no Gaze-operated cloud service in the open-source distribution; the runtime executes in your process, your container, or your VM.
- The third-party LLM provider (OpenAI, Anthropic, Google, etc.) typically acts as a *processor* for the data you send it — but you only send it pseudonymised tokens, not raw personal data, which is the entire point of using Gaze.

Because Gaze does not phone home, transmit telemetry, or aggregate adopter data, there is no Gaze-side controller or processor role for the open-source product. If you later subscribe to a hosted commercial offering operated by the maintainers, that offering will have its own Data Processing Agreement (DPA) covering the controller-processor relationship explicitly.

## 2. Pseudonymisation vs anonymisation

GDPR Art. 4(5) defines pseudonymisation as:

> "The processing of personal data in such a manner that the personal data can no longer be attributed to a specific data subject without the use of additional information, provided that such additional information is kept separately and is subject to technical and organisational measures..."

Gaze is **pseudonymisation, not anonymisation**.

- Tokens emitted by Gaze (`<Email_1>`, `<Name_1>`, `<OrderId_1>`) cannot, on their own, be attributed to a specific individual.
- The restore manifest is the "additional information" that maps tokens back to the original values. Gaze keeps the manifest separately from the redacted text by design — the manifest lives on your server; only tokens cross the boundary to the LLM.
- Because the manifest exists and can be used to recover the original values, the redacted output is **still personal data under GDPR**, just personal data subject to reduced risk because pseudonymisation has been applied.

**Anonymisation** would mean the original values cannot be recovered by *anyone* by any means — including by you. Gaze deliberately does not do that. Anonymisation would prevent the legitimate downstream use cases Gaze is designed for (rehydrating an LLM-generated draft so the support agent or end user actually sees the customer's name).

Practical consequence: pseudonymised data remains in scope for GDPR. Your lawful basis, transparency obligations, data-subject rights, and retention duties still apply to the manifest. They apply with **reduced risk** (and qualify for the favourable treatment Art. 32 grants to pseudonymisation as a technical measure), but they do not disappear.

## 3. Retention defaults

Gaze does not impose a fixed retention period on the restore manifest. The right value depends on your workflow — a synchronous support-reply flow may need the manifest for seconds; a multi-day agent investigation may need it for days.

What Gaze provides:

- **Ephemeral sessions** (`Scope::Ephemeral` in the library API) — the namespace exists only while the `Session` is held in memory and is dropped when the `Session` is dropped. No persistence, no on-disk artifact. See [`docs/architecture/session-contract.md`](../architecture/session-contract.md).
- **Daemon-mode session eviction** — when Gaze runs as a long-lived daemon (`gaze daemon`), sessions are evicted by least-recently-used policy when the configured cap is exceeded, and idle sessions older than `--session-idle-timeout` are evicted automatically. Eviction drops the restore map for that session. See [`docs/architecture/daemon-mode.md`](../architecture/daemon-mode.md).
- **No implicit persistence** — Gaze does not write the manifest to disk unless you explicitly opt into a persistent backend in your adopter integration. The audit log (`gaze-audit`) records metadata about what was tokenised, never the raw value or token-to-value pair.

**Recommended adopter defaults:**

- Keep the manifest in process memory for the lifetime of the current request or agent turn, not longer.
- If you must persist a manifest across requests (e.g. for a multi-step agent workflow), set the shortest TTL that the workflow can tolerate, and have your DPO sign off on the chosen retention.
- Treat the restore manifest as **personal data of the highest sensitivity** in your retention policy — it is the key that re-identifies the entire pseudonymised dataset.

## 4. Restore-authorisation boundary

Re-materialising original values from tokens is a privileged operation. Gaze enforces this with what we call the *restore boundary* — see [`docs/architecture/restore-boundary.md`](../architecture/restore-boundary.md) for the full contract. In short:

- A token can be restored to its original value **only if** the currently-active manifest authorises that specific token-to-value mapping.
- Unknown tokens, tokens from another session, tokens from another tenant, and malformed tokens all fail closed (returns a typed restore failure, no guessing, no fallback).
- Restore decisions are auditable: each restore call can be logged with metadata (`recognizer_id`, `recognizer_version_id`, session, timestamp) into the SQLite audit log without ever logging the raw value.

For adopters this means:

- **Decide explicitly who and what can call `restore()` in your application.** Treat it the same way you treat your database read credentials — narrow scope, audited path.
- **Do not expose `restore()` to the LLM.** The LLM should never receive a restored value, only tokens. Restore happens after the LLM has produced its output and before that output reaches the human who is authorised to see the original data (e.g. the support agent reviewing a draft reply).
- **Log restore calls.** The audit log is metadata-only by design — turn it on so you have a defensible trail of when, by whom, and for which session a restore happened.

## 5. Dual-use and misuse risks

Privacy tooling can be misused. We want to be honest about where Gaze could go wrong if deployed without care:

- **Hiding identity from the human reviewer, not from the model.** If an adopter pseudonymises data so well that even the eventual human reviewer cannot tell who they are accountable to, that may produce GDPR-compliant LLM traffic but irresponsible decision-making downstream. Gaze restores values for the authorised human; restoring should be the default for the human-facing surface.
- **Using pseudonymisation as a substitute for lawful basis.** Pseudonymisation is a *technical measure*, not a lawful basis under Art. 6. If you have no lawful basis to process personal data, pseudonymising it does not create one.
- **Re-using session-bound tokens across sessions to "track" individuals.** Gaze's tokens are session-scoped and counter-based (`<Email_1>`, `<Email_2>`) precisely so they cannot be used as a covert stable identifier across sessions. Do not concatenate or persist tokens across sessions to reconstruct a pseudo-stable ID — that would defeat the design and likely constitute a separate processing purpose requiring its own legal basis.
- **Disabling the SafetyNet in production.** The optional second-pass detector (Pass-3 SafetyNet) catches bytes the rule layer missed. Adopters who turn it off for latency are reducing detection coverage; the resulting deployment is less safe than the default.
- **Using Gaze to redact AI output that has already been generated *without* tokenising the input.** Output-only redaction (after the LLM has seen the raw data) does not protect against the boundary leak — the model provider already received the data. Gaze is designed to be applied **before** the model boundary.

If you discover a deployment pattern that creates a privacy risk we did not anticipate, please file a GitHub issue or contact the maintainers via `SECURITY.md`.

## 6. Limits of pseudonymisation

Pseudonymisation reduces re-identification risk; it does not eliminate it. Adopters should be aware of these limits:

- **Quasi-identifier combinations.** Gaze tokenises explicit identifiers (names, emails, phone numbers, national IDs). It does *not* automatically tokenise combinations of non-identifying facts that, taken together, could uniquely identify someone (e.g. "32-year-old female lawyer in Galway with a peanut allergy and a 2018 Volvo"). If your free-text payload is rich in this kind of context, you may need an additional anonymisation pass, a different consent model, or a smaller payload — pseudonymising the email alone is not enough.
- **Re-identification by linkage.** If the same individual's data passes through the LLM in multiple separate sessions, an adversary with access to those sessions could in principle link them by the surrounding context (writing style, recurring topics, indirect identifiers). Session-scoped tokens prevent token-based linkage; they cannot prevent linkage by other means.
- **NER coverage gaps.** Free-text name detection depends on the NER backend you configure. No NER is perfect — unusual names, transliterations, and culture-specific naming conventions can be missed. The roadmap-funded coverage evaluation harness exists to measure exactly this gap; for now, adopters processing high-stakes free text should not rely on NER alone.
- **Recognizer false negatives.** A recognizer that does not match emits no token. The optional SafetyNet exists to catch this case, but no detection layer is exhaustive. Treat the detection coverage table in the project README as a floor, not a guarantee, and configure your policy for your specific data classes.
- **Pseudonymisation does not anonymise audit logs.** The `gaze-audit` SQLite log records *that* a token was emitted (class, recognizer, version, timestamp) but **never** the raw value or the token-to-value mapping. The audit log is metadata-only by design. Treat the manifest, not the audit log, as the sensitive artefact.
- **No protection against compromise of your own host.** Gaze runs in your process. If your host is compromised, the attacker has access to the manifest and can restore everything. Gaze is a boundary-protection tool against third-party LLM exposure, not an in-host security tool.

---

## 7. Suggested DPIA checklist

If you are completing a Data Protection Impact Assessment for a Gaze deployment, the following items will probably come up:

- [ ] **Processing purpose** — what is the LLM call actually for? Is the purpose proportionate to the personal data involved?
- [ ] **Lawful basis** — Art. 6 ground for the underlying processing? Special-category data (Art. 9) involved?
- [ ] **Data minimisation** — is the payload to the LLM the smallest set of attributes needed to answer the question?
- [ ] **Pseudonymisation scope** — which classes does your policy enable? Which are left detected-but-not-tokenised, and why?
- [ ] **Manifest retention** — how long does the manifest live? Where is it stored? Who can access it?
- [ ] **Restore authorisation** — which application paths are allowed to call restore? Are restore calls logged?
- [ ] **Audit log access** — who can read the audit log? Is access itself logged?
- [ ] **Third-party LLM provider** — what does their DPA cover? Are they an adequate-country provider or covered by a transfer mechanism (SCCs, etc.)?
- [ ] **Data-subject rights** — how do you handle access / erasure requests? Erasing the manifest entry effectively re-anonymises a session's tokenised output post-hoc, which is useful for Art. 17.
- [ ] **Breach response** — if the manifest is exfiltrated, what is your notification plan?

This list is not exhaustive and is not a substitute for engaging your DPO.

---

## 8. Reporting privacy concerns

If you find a Gaze behaviour that creates or worsens a privacy risk:

- **Privacy-sensitive bugs** — file a GitHub issue with the `privacy` label, or use the channels in `SECURITY.md` for vulnerabilities that should not be disclosed publicly until a fix lands.
- **Deployment-pattern feedback** — open a GitHub Discussion. The maintainers want to learn from real-world adopter experience so this guidance can improve.
