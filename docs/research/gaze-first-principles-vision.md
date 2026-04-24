# Gaze — First Principles Vision

**Date:** 2026-04-14

---

## North Star (locked 2026-04-24)

> **Gaze is the most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the manifest contract is a critical defect.**

Verbatim user directive (2026-04-24): *"for the project set a north star to be focused on never leaking any PII data and making this lib the best PII obscuring/<enter better word here> there is for agentic interaction with information"*. The placeholder resolves to **pseudonymization**.

### Word choice

- **"Pseudonymization"** — precise GDPR / EDPB Art. 4(5) term for reversible substitution with tokens. Chosen over:
  - *Redaction* — implies one-way removal, loses the restore moat.
  - *Obscuring* — too vague, loses legal/compliance resonance.
  - *Tokenization* — narrower, shares airtime with payment-tokenization.
- **"Runtime"** (not *library* or *tool*) — emphasizes Gaze sits in the request path as infrastructure, not as a utility called occasionally.

### Five axes every decision MUST evaluate against

1. **Reliability (never leak).** Fail-closed always. Defense in depth (regex + NER + dictionary + optional neural safety net). Every known detection gap = todo. Every leak incident = postmortem + fix pattern baked into skill/memory.
2. **Reversibility.** Manifest-first restore (already locked). Format-preserving tokens stay restorable. No one-way primitives in the core contract. Anything that breaks restore round-trip is a design regression.
3. **Agentic-first.** Decisions prioritize agent workflow needs over generic text handling: tool-call JSON embedding, streaming LLM, multi-turn sessions with evolving context, tenant-specific PII (songs, order IDs, artist names — the Dashboard adopter case). Generic redaction tools don't close these; Gaze does.
4. **Trust (auditable + deterministic).** Rule-based detectors preferred over neural for precise classes. Neural is an addon (safety net, free-text NER), not the floor. Every token emission traceable to a rule/recognizer. Typed exceptions + closed error-variant set. No silent mismatches. Every leak has a root-causable decision path.
5. **Adopter ergonomics.** Low-friction integration (Laravel adapter pattern, clear TOML policy, sane defaults). Framework adapters pave the 80% case; library API serves the 20% power case. Adopter can pick Gaze up in under a day without deep PII domain expertise.

### North-star reframes active decisions

- **v0.4 F6 Dictionary detector** — this IS north-star work. Tenant PII (songs, order IDs, artists) is what generic detectors miss. F6 elevates from "adopter feature request" to "core Gaze capability". Keep Phase 1 priority.
- **v0.4.1 F8 OpenAI-filter Pass-3 safety net (#65)** — elevates from "nice-to-have" to "reliability axis #1 work". If cost-acceptable, this closes the "never leak" promise at the edge. Move up in priority if v0.4.1 has room.
- **v0.4 Phase 5 Q2 CI sanity gate (#66)** — directly supports the reliability axis. Keep.
- **Q7.1 = Y (session-scoped tokens)** — aligns with the trust axis (deterministic grammar beats fingerprint heuristic). Correct call.
- **Markus's `gaze-laravel` adapter** — exemplifies the adopter-ergonomics axis. Pattern to replicate for Python, Node, Go adapters in v0.5+.

### What the north star REJECTS

- **Shipping partial-closure solutions as "done".** Every PII class must be fully round-trippable or explicitly flagged as "detection-only" (Phase 3 safety-net use only). No middle ground.
- **Making detection-mode the primary value prop.** OpenAI shipped their privacy filter 2026-04-22 — free, open-source, good detection. Gaze does not compete on detection. Gaze's moat is the REVERSIBLE + AUDITABLE + AGENTIC stack.
- **Generic "data scrubbing" positioning.** Gaze is for agent/LLM workflows specifically. Don't dilute to "general-purpose data masking" — that's a different product.
- **Performance before correctness.** A 2× speedup that introduces a leak edge case is a regression. Correctness axes 1–4 always beat speed.

### Measuring drift

Every major phase completion (Phase 1, 2, …) does a north-star audit:

- Does what shipped this phase strengthen one of the 5 axes?
- Does anything shipped this phase weaken an axis?
- Any items that now conflict with "never leak"?

If an axis slipped → postmortem drawer + fix plan.

Source of truth: MemPalace drawer `drawer_gaze_decisions_ba559e1cf1fbca5c1098b12f` (wing=gaze, room=decisions).

---

## The Premise

Input can take two forms: **structured** (database rows, JSON, CSV) or **unstructured** (logs, emails, free text).

Input can be accessed through multiple **channels**: databases, log files, emails, APIs, file systems, application output.

An AI agent needs to work with this information but must not see the PII it contains. DSGVO/GDPR makes this a legal requirement, not a preference.

## The Core Abstraction

Gaze is the **black marker** — the interceptor that sits between an agent and the information the agent wants to inspect. From first principles, the entire system reduces to three operations:

```
detect → anonymize → restore
```

1. **Detect** — find PII in any input (structured or unstructured)
2. **Anonymize** — replace PII with session-scoped tokens the agent can reason about
3. **Restore** — resolve tokens back to real values when action is needed

Everything else is a consumer concern. Databases, MCP protocol, CLI, TOML config, log parsing — all of it is plumbing around these three operations.

## Two Composable Extension Points

From this first-principles view, two concepts emerge naturally:

**Detector** — answers "is this PII?" Multiple detection strategies exist (regex, NER, domain-specific indexes, bloom filters, fuzzy matching). They all share one interface: take text in, return spans with PII classifications. A pipeline stacks detectors — each one catches what the others miss.

**Rule** — answers "what do I do with this PII?" Different contexts demand different actions: tokenize (preserve correlation), redact (destroy), format-preserve (fake email that looks like email), generalize (city → region), or preserve (explicitly safe). Rules compose — first match wins.

These two concepts are sufficient to express any PII handling strategy. A debug proxy uses column-aware rules and regex+NER detection. A customer support sanitizer uses known-context detection and domain-specific indexes. A compliance scanner uses aggressive detection and default-redact rules. Same core, different composition.

## The Product Landscape

Gaze core is a library. Products are built on top:

| Product | Channel | Detects with | Acts via |
|---------|---------|-------------|----------|
| **Debug proxy** | MySQL + Laravel logs | Column rules + regex + NER | MCP tools (read-only) |
| **Ghostwriter** | Customer messages | Known context + domain index + NER | Sanitize/restore for LLM |
| **Pipe mode** (v0.3) | stdin/stdout | Regex + NER | Unix pipes |
| **Operations proxy** (future) | Any | Inherits from read-side | Agent-directed actions |
| **Desktop app** (vFuture) | Any | Any | Native UI |

Each product composes detectors and rules for its use case. Core doesn't know or care which product is using it.

## The Agent Interaction Problem

The first-principles view reveals a deeper challenge. Gaze v0.1 and v0.2 solve the **read side**: agent sees anonymized data, reasons about it safely. But agents don't just read — they **act**.

An agent debugging a production issue might need to:
- Query a database (read — solved by debug proxy)
- Search logs for a customer's activity (read — solved by debug proxy)
- Send a notification to the affected customer (write — **unsolved**)
- Update a record to fix the issue (write — **unsolved**)
- Execute a script that processes customer data (execute — **unsolved**)

The moment an agent needs to act, it either uses raw PII (DSGVO violation) or can't act at all (useless). The read side gives the agent context; the write side gives it agency. Without both, the agent is an observer, not an operator.

### Tokenized Handles as Agent Primitives

The solution extends naturally from the core abstraction. If the agent works with tokens (`Person_7`, `Email_3`, `Order_AF-20458`), these tokens are **opaque handles** to real data. Gaze can resolve handles and execute actions without the agent ever seeing the underlying values:

```
Agent sees:     "Person_7 has 3 failed orders (Order_42, Order_43, Order_44)"
Agent decides:  "Notify Person_7 about order resolution"
Agent calls:    send_notification(to: "Person_7", about: ["Order_42", "Order_43"])
Gaze resolves:  Person_7 → john@example.com, Order_42 → AF-20458, ...
Gaze executes:  sends real email with real order numbers
Agent receives: "Notification sent to Person_7 about Order_42, Order_43 ✓"
```

The agent has full reasoning capability and full action capability. Zero PII in the LLM context window at any point.

### The ACL Layer: What Agents Can See vs. Do

This creates a new axis of control: **what can the agent do with a handle at each stage?**

| Capability | Read phase | Action phase |
|-----------|-----------|-------------|
| See token | ✓ | ✓ |
| See raw value | ✗ | ✗ |
| Use token in query | ✓ | ✓ |
| Execute action via token | ✗ | ✓ |
| See action's raw output | ✗ | ✗ |
| See action's clean result | ✗ | ✓ |

This is an ACL system for agents — not file-level permissions, but **data-level capability control**. The agent has capabilities over handles, not over the data the handles point to.

### Kernel-Level Enforcement: nono

Software-level ACLs are only as strong as the process boundary. An agent running code could read `/proc/self/mem` or intercept system calls. Real enforcement needs kernel-level sandboxing.

[nono](https://github.com/always-further/nono) provides this via Landlock (Linux) and Seatbelt (macOS):

```
caps.allow_exec("/usr/bin/gaze")?;    // agent can invoke gaze
caps.allow_read(clean_output_path)?;  // agent can read anonymized results
// NO allow_read on raw data, DB connections, email contents
```

The pattern:
1. Agent runs inside nono sandbox with restricted capabilities
2. Agent calls gaze binary (allowed by sandbox)
3. Gaze resolves tokens → executes action with real values (outside sandbox restrictions — gaze is trusted)
4. Gaze re-anonymizes output → writes to clean output path
5. Agent reads clean result (allowed by sandbox)
6. Agent never has read access to raw PII at any point — enforced by kernel, not by convention

This combines Gaze's token resolution with nono's capability-based security to create a complete privacy boundary for agent execution.

### The Execution Pattern

```
Agent (sandboxed by nono)
    │
    │  "send email to Email_3 about Order_42"
    │
    ▼
Gaze CLI: gaze exec --script notify.sh --args '{"to":"Email_3","re":"Order_42"}'
    │
    ├─ session.restore("Email_3") → john@example.com
    ├─ session.restore("Order_42") → AF-20458
    ├─ execute notify.sh with real values
    ├─ capture output
    ├─ pipeline.redact(output) → clean result
    │
    ▼
Agent receives: "Email sent to Email_3 regarding Order_42 ✓"
```

The agent writes the *intent* ("notify this person about these orders"). Gaze handles the *execution* (resolve, act, re-anonymize). The agent never touches PII — not in reading, not in writing, not in execution.

## The Roadmap Through This Lens

| Version | Capability | First Principle |
|---------|-----------|----------------|
| v0.1 | Read structured data (MySQL) + unstructured (logs) | detect → anonymize |
| v0.2 | Unified core engine, composable pipeline | detect → anonymize → restore (channel-agnostic) |
| v0.3 | Pipe mode, format-preserving output | Same core, new consumer (Unix pipes) |
| v0.4 | Operations proxy | restore → execute → re-anonymize |
| v0.5 | nono integration, agent ACLs | Kernel-enforced privacy boundary |
| vFuture | Desktop app, compositional attack defense | Native UI + advanced threat model |

Each version extends naturally from the previous one. No architectural rewrites — the composable core (`Detector` + `Rule` + `Session`) is sufficient for all of them.

## Summary

Gaze is not a database proxy. Gaze is not a log scanner. Gaze is not an MCP server.

Gaze is the black marker. It sits between an agent and information, ensuring the agent can reason about and act on data without ever seeing what's underneath the marker. The marker is composable (different detection strategies), configurable (different anonymization actions), and reversible (session-scoped tokens that resolve back to real values when trusted code needs them).

The products built on Gaze determine the channels (DB, logs, email, files), the protocols (MCP, pipes, CLI), and the user experience. The core just marks things black.
