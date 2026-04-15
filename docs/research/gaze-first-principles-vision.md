# Gaze — First Principles Vision

**Date:** 2026-04-14

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
