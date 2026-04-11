# Ghostwriter Sanitization Design

**Status:** Design / Pre-v0.1
**Author:** Krishan Koenig
**Date:** 2026-04-11

---

## Problem

Artistfy's Ghostwriter flow needs to send inbound customer messages to an LLM without leaking personal data unnecessarily. The app already knows the primary customer identity from its own database context, but the raw email body can still contain direct identifiers such as names, emails, phone numbers, addresses, signatures, and third-party contact details.

The current debugging-oriented Gaze design is not the right abstraction for this job:

- Ghostwriter is not a live debugging proxy.
- Ghostwriter does not need DB filter round-tripping.
- Ghostwriter does not need MCP tools.
- Ghostwriter does need deterministic sanitization of text plus deterministic rehydration of exact placeholders in the model's draft.

This design defines a separate Ghostwriter-focused sanitization component that can later share primitives with Gaze Debug, without forcing the debug product's session pseudonym architecture onto a simpler text workflow.

## Goal

Build a transport-agnostic Rust core that:

1. Accepts inbound raw message text plus minimal known customer identity context
2. Produces sanitized text safe to send to an LLM
3. Produces a session blob containing the exact restoration mapping for that message
4. Restores exact placeholders in an LLM-produced draft back to raw values
5. Avoids heuristic guessing during restoration

The immediate consumer is Markus's Laravel integration in Artistfy. Laravel will own storage, queueing, and encryption of the session blob outside the Rust process.

## Non-Goals

- Not an MCP tool surface
- Not a DB/log debugging proxy
- Not cross-message pseudonymous identity tracking
- Not heuristic entity resolution or fuzzy restoration
- Not queue/storage/session management in Laravel
- Not full business-object normalization for order IDs, invoice IDs, or tracking numbers in v1

## Product Boundary

Ghostwriter is a text sanitization and rehydration component for customer communications.

It is separate from the in-flight Gaze debug product:

- **Gaze Debug** remains the MCP/data-access product with session pseudonyms, `RawRow`/`CleanRow`, and `restore()` for filter values.
- **Ghostwriter** is a one-message sanitization product that turns raw text into placeholder-rich clean text and later restores only exact placeholders.

This split is intentional. The debugging product needs cross-query correlation and reversible pseudonyms across structured data. Ghostwriter only needs deterministic text cleaning and exact token restoration for a single inbound/outbound message workflow.

## Core Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Placeholder style | Hybrid semantic + typed | Preserve known customer meaning while keeping unknown PII generic |
| Context model | Hybrid-minimal | Laravel passes only primary customer identity; Ghostwriter discovers the rest from text |
| Restoration model | Strict token restoration | Deterministic and safe; no guessing identity on the way out |
| Unknown placeholder stability | Stable per message | Repeated unknown values should map to the same placeholder within one sanitize call |
| Business identifiers | Leave as-is in v1 | Avoid overextending scope into order/invoice/tracking classification before real usage proves need |
| Storage/queueing | Laravel-owned | Gaze/Ghostwriter stays transport-agnostic; Markus can wire persistence manually |
| Detection engine | Worka `pii` | Good fit for text detection; Ghostwriter owns placeholder assignment and restoration |

## High-Level Flow

### Sanitize

```text
Laravel
  -> sends raw text + known customer identity
  -> Ghostwriter replaces known customer values first
  -> Ghostwriter runs PII detection on remaining text
  -> Ghostwriter assigns generic typed placeholders for remaining PII
  -> Ghostwriter returns clean_text + session_blob + warnings/metadata
```

### Rehydrate

```text
Laravel
  -> sends LLM draft + session_blob
  -> Ghostwriter restores exact placeholder tokens only
  -> Ghostwriter returns restored_text + warnings
```

No heuristic matching is allowed during restore. If the model paraphrases or invents text, that text stays as written unless it contains an exact placeholder emitted during sanitize.

## Input Contract

The minimum sanitize input is:

```json
{
  "text": "raw inbound customer text",
  "context": {
    "customer_name": "Markus Mueller",
    "customer_email": "mueller.markus@icloud.com",
    "customer_phone": "+49 151 23456789"
  }
}
```

Rules:

- `text` is required.
- `context` is required, but each field inside it is optional.
- v1 context is limited to primary customer identity:
  - `customer_name`
  - `customer_email`
  - `customer_phone`
- Laravel does **not** need to pass order IDs, shop name, thread ID, agent identity, or prior conversation state in v1.

The minimum restore input is:

```json
{
  "text": "LLM-produced draft containing placeholders",
  "session_blob": "opaque application-stored blob"
}
```

## Output Contract

### Sanitize Response

Ghostwriter returns:

```json
{
  "clean_text": "sanitized text with placeholders",
  "session_blob": "opaque blob needed for exact restoration",
  "warnings": [],
  "metadata": {
    "placeholders": [
      "<CUSTOMER_NAME>",
      "<CUSTOMER_EMAIL>",
      "<EMAIL_1>",
      "<PHONE_1>"
    ]
  }
}
```

### Restore Response

Ghostwriter returns:

```json
{
  "restored_text": "draft with exact placeholders restored",
  "warnings": [
    "placeholder <EMAIL_1> was not used",
    "unknown placeholder <EMAIL_9> left unchanged"
  ]
}
```

Warnings are informational. They do not fail restore unless the Rust API itself receives invalid input or a corrupt blob.

## Placeholder Strategy

### Known Customer Identity

Known customer identity supplied by Laravel is always handled first and gets semantic placeholders:

- `customer_name` -> `<CUSTOMER_NAME>`
- `customer_email` -> `<CUSTOMER_EMAIL>`
- `customer_phone` -> `<CUSTOMER_PHONE>`

This stage runs before generic detection so that obvious customer-specific values do not get consumed by generic placeholders like `<EMAIL_1>`.

Matching rules:

- Exact string matches are replaced.
- Matching is repeated across the full text, not only the first occurrence.
- The same raw known value always maps to the same semantic placeholder within the message.
- If a context field is absent, no placeholder for it is created.

### Remaining PII

After known customer values are replaced, Ghostwriter runs text detection over the remaining text.

Detected entity types map to generic typed placeholders:

- email -> `<EMAIL_1>`, `<EMAIL_2>`, ...
- phone -> `<PHONE_1>`, `<PHONE_2>`, ...
- person name -> `<NAME_1>`, `<NAME_2>`, ...
- address -> `<ADDRESS_1>`, `<ADDRESS_2>`, ...
- other supported text PII classes may follow the same pattern

Rules:

- Placeholder numbering is stable within one sanitize call.
- Repeated occurrences of the same raw value reuse the same placeholder.
- Numbering scope is per entity type, not global.
- Placeholder numbering does **not** carry across messages in v1.

### Business Identifiers

Business identifiers stay unchanged in v1:

- order numbers
- invoice numbers
- tracking IDs
- internal ticket IDs

Reason: these identifiers are often operationally useful to the model and are not the immediate privacy problem this design is solving. If real usage later shows they should be tokenized, that becomes a separate design.

## Sanitization Semantics

Ghostwriter sanitization is two-stage:

### Stage 1: Known Context Replacement

Replace known customer values from Laravel context before running generic detection.

This gives the model higher-quality prompts. Example:

Raw:

```text
Hi Artistfy team,

Can you send it to markus.mueller@example.de instead of mueller.markus@icloud.com?
Thanks,
Markus Mueller
```

Sanitized:

```text
Hi Artistfy team,

Can you send it to <EMAIL_1> instead of <CUSTOMER_EMAIL>?
Thanks,
<CUSTOMER_NAME>
```

### Stage 2: Generic Detection and Placeholder Assignment

Run Worka `pii` on the remaining text and replace detected spans with stable per-message typed placeholders.

Ghostwriter uses `pii` as a detector, not as the final anonymizer. The placeholder values are Ghostwriter-owned because Ghostwriter must preserve a reversible mapping for strict restoration.

## Restoration Semantics

Restoration is strict and exact:

- Restore only placeholders that exist in the session blob.
- Restore only exact placeholder tokens.
- Leave all other text untouched.
- Do not infer identity from nearby words.
- Do not attempt fuzzy matching for paraphrases.

Examples:

Sanitized input to LLM:

```text
Hello <CUSTOMER_NAME>, we can resend the files to <CUSTOMER_EMAIL>.
```

Model draft:

```text
Hello <CUSTOMER_NAME>, we can resend the files to <CUSTOMER_EMAIL> today.
```

Restored:

```text
Hello Markus Mueller, we can resend the files to mueller.markus@icloud.com today.
```

If the model writes:

```text
Hello Markus, we can resend the files to the address you mentioned.
```

Ghostwriter restores nothing in that sentence because there are no exact known placeholders.

## Session Blob

The session blob is an opaque payload from Laravel's perspective. Laravel stores and transports it, but Ghostwriter owns its schema.

The blob must contain enough information to:

- map placeholders back to raw values
- preserve message-local numbering for unknown placeholders
- reject malformed or corrupt payloads

The blob does **not** need to support:

- cross-message continuity
- DB filter round-tripping
- long-lived pseudonymous identities

Laravel is responsible for:

- storing the blob between sanitize and restore
- encrypting it before durable storage or queueing
- passing the blob back verbatim during restore

Ghostwriter is responsible for:

- generating the blob
- validating it on restore
- refusing to guess when the blob and text do not line up

## Error Handling

### Sanitize Errors

Sanitize fails only for structural or processing failures:

- empty/invalid request payload
- invalid UTF-8 or unsupported input shape
- detector failure
- internal placeholder-mapping failure

Sanitize should not fail merely because no PII was detected. In that case it returns the original text, an empty or minimal blob, and optional warnings.

### Restore Errors

Restore fails for:

- missing session blob
- unreadable or corrupt session blob
- structurally invalid request payload

Restore does **not** fail merely because placeholders are unused or absent from the draft. That condition returns warnings instead.

### Warnings

Warnings are used for non-fatal situations:

- no placeholders found in draft
- placeholder existed in blob but was not used
- unknown placeholder-shaped token appeared in output text
- detector confidence or unsupported-entity issues worth surfacing for review

## Security Model

This product is a minimization layer, not a claim of perfect anonymity.

Security properties in v1:

- Raw inbound text never needs to be sent to the LLM.
- The LLM can only trigger restoration by replaying exact placeholders it was given.
- The model cannot force Ghostwriter to restore arbitrary guessed real values.
- Laravel controls encryption and persistence of the session blob outside the Rust process.

Security properties intentionally deferred:

- cross-message session identity
- fuzzy or semantic restoration
- business-object normalization
- transport/storage guarantees inside Laravel

## Why Worka `pii` Still Fits

Worka `pii` is still a good dependency here because Ghostwriter needs text detection, not the full debugging-proxy semantics:

- detect PII spans in inbound text
- provide stable offsets
- support deterministic text scanning across freetext

Ghostwriter should **not** use `pii`'s built-in final anonymization output directly for this product. Instead:

- `pii` detects spans
- Ghostwriter decides the placeholder string
- Ghostwriter owns the exact placeholder-to-raw mapping for restore

This split keeps `pii` within its natural scope while still making it useful.

## Laravel Integration Boundary

Markus's Laravel work for v1 is intentionally small.

Laravel must:

1. Gather raw inbound text
2. Gather known primary customer identity if available
3. Call Ghostwriter sanitize
4. Send `clean_text` to the LLM
5. Persist the returned `session_blob` however the app chooses
6. Call Ghostwriter restore with the model draft and the same blob

Laravel does not need to:

- understand placeholder numbering
- inspect the blob internals
- implement heuristic restoration
- pass full business context into v1

## Example End-to-End

Sanitize request:

```json
{
  "text": "Hi Artistfy, Markus Mueller here. Please resend to mueller.markus@icloud.com. If needed call +49 151 23456789. Alternate email: markus.mueller@example.de",
  "context": {
    "customer_name": "Markus Mueller",
    "customer_email": "mueller.markus@icloud.com",
    "customer_phone": "+49 151 23456789"
  }
}
```

Sanitize response:

```json
{
  "clean_text": "Hi Artistfy, <CUSTOMER_NAME> here. Please resend to <CUSTOMER_EMAIL>. If needed call <CUSTOMER_PHONE>. Alternate email: <EMAIL_1>",
  "session_blob": "<opaque>",
  "warnings": []
}
```

LLM draft:

```text
Hello <CUSTOMER_NAME>, we will resend the files to <CUSTOMER_EMAIL> today. If needed we will contact you at <CUSTOMER_PHONE>.
```

Restore response:

```json
{
  "restored_text": "Hello Markus Mueller, we will resend the files to mueller.markus@icloud.com today. If needed we will contact you at +49 151 23456789.",
  "warnings": [
    "placeholder <EMAIL_1> was not used"
  ]
}
```

## Open Questions Deferred

These are explicitly out of v1 rather than unresolved:

- Whether business identifiers should later become placeholders
- Whether cross-message continuity should exist for a whole email thread
- Whether outbound drafts should support richer semantic placeholders such as `<DELIVERY_EMAIL>` or `<SHOP_NAME>`
- Whether the blob schema should later be shared with Gaze Debug pipe mode
- Whether Laravel should eventually pass richer context than primary customer identity

## Success Criteria

The design is successful when:

1. Markus can implement the Laravel side with only customer identity plus blob pass-through
2. Inbound customer emails reach the LLM without raw customer identity in the text body
3. LLM drafts can be restored deterministically when they reuse exact placeholders
4. The system does not guess on restore
5. The design remains separate from the in-flight debug Gaze v0.1 implementation
